//! Postgres-адаптеры контекста messaging: диалоги, сообщения и read-модели
//! (входящие, тред). Пара участников канонизируется как в домене (меньший UUID
//! первым) — БД хранит (user_lo, user_hi) с UNIQUE.

use async_trait::async_trait;
use babangida_application::query::{
    ConversationView, InboxReadModel, MessageView, ThreadReadModel,
};
use babangida_domain::RepositoryError;
use babangida_domain::identity::UserId;
use babangida_domain::messaging::{
    Conversation, ConversationId, ConversationRepository, Message, MessageRepository,
};
use babangida_shared::{Id, Timestamp};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::map_sqlx;
use crate::pool::Db;

/// Канонический порядок пары (как в домене): меньший UUID первым.
fn canonical(a: UserId, b: UserId) -> (Uuid, Uuid) {
    let (x, y) = (a.as_uuid(), b.as_uuid());
    if x <= y { (x, y) } else { (y, x) }
}

fn row_to_conversation(
    id: Uuid,
    lo: Uuid,
    hi: Uuid,
    opened_at: OffsetDateTime,
) -> Result<Conversation, RepositoryError> {
    // Реконституция через доменный API (patterns/repository); пара уже канонична.
    let (conv, _event) = Conversation::open(
        Id::from_uuid(id),
        Id::from_uuid(lo),
        Id::from_uuid(hi),
        Timestamp::from_offset(opened_at),
    )
    .map_err(|_| RepositoryError::Unavailable("повреждённый диалог в БД".to_owned()))?;
    Ok(conv)
}

/// Репозиторий диалогов на Postgres.
pub struct PgConversationRepository {
    db: Db,
}

impl PgConversationRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ConversationRepository for PgConversationRepository {
    async fn find_by_id(
        &self,
        id: ConversationId,
    ) -> Result<Option<Conversation>, RepositoryError> {
        let row: Option<(Uuid, Uuid, OffsetDateTime)> =
            sqlx::query_as("SELECT user_lo, user_hi, opened_at FROM conversations WHERE id = $1")
                .bind(id.as_uuid())
                .fetch_optional(&self.db)
                .await
                .map_err(map_sqlx)?;
        row.map(|(lo, hi, ts)| row_to_conversation(id.as_uuid(), lo, hi, ts))
            .transpose()
    }

    async fn find_between(
        &self,
        a: UserId,
        b: UserId,
    ) -> Result<Option<Conversation>, RepositoryError> {
        let (lo, hi) = canonical(a, b);
        let row: Option<(Uuid, OffsetDateTime)> = sqlx::query_as(
            "SELECT id, opened_at FROM conversations WHERE user_lo = $1 AND user_hi = $2",
        )
        .bind(lo)
        .bind(hi)
        .fetch_optional(&self.db)
        .await
        .map_err(map_sqlx)?;
        row.map(|(id, ts)| row_to_conversation(id, lo, hi, ts))
            .transpose()
    }

    async fn save(&self, conversation: &Conversation) -> Result<(), RepositoryError> {
        let (lo, hi) = conversation.participants();
        // ON CONFLICT по канонической паре: при гонке создания второй вызов — no-op,
        // use-case затем перечитывает диалог и пишет сообщение в канонический.
        sqlx::query(
            "INSERT INTO conversations (id, user_lo, user_hi, opened_at) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (user_lo, user_hi) DO NOTHING",
        )
        .bind(conversation.id().as_uuid())
        .bind(lo.as_uuid())
        .bind(hi.as_uuid())
        .bind(conversation.opened_at().into_offset())
        .execute(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }
}

/// Репозиторий сообщений на Postgres (только запись; чтение — read-модель).
pub struct PgMessageRepository {
    db: Db,
}

impl PgMessageRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl MessageRepository for PgMessageRepository {
    async fn append(&self, message: &Message) -> Result<(), RepositoryError> {
        sqlx::query(
            "INSERT INTO messages (id, conversation_id, author_id, body, sent_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(message.id().as_uuid())
        .bind(message.conversation().as_uuid())
        .bind(message.author().as_uuid())
        .bind(message.body().as_str())
        .bind(message.sent_at().into_offset())
        .execute(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }
}

/// Read-модель входящих: диалоги юзера с последним сообщением (ADR-0004).
pub struct PgInboxReadModel {
    db: Db,
}

impl PgInboxReadModel {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl InboxReadModel for PgInboxReadModel {
    async fn for_user(
        &self,
        user: UserId,
        limit: u32,
    ) -> Result<Vec<ConversationView>, RepositoryError> {
        let rows: Vec<(Uuid, String, String, OffsetDateTime)> = sqlx::query_as(
            "SELECT c.id, cp.handle, lm.body, lm.sent_at \
             FROM conversations c \
             JOIN LATERAL ( \
                 SELECT m.body, m.sent_at FROM messages m \
                 WHERE m.conversation_id = c.id \
                 ORDER BY m.sent_at DESC, m.id DESC LIMIT 1 \
             ) lm ON TRUE \
             JOIN users cp ON cp.id = CASE WHEN c.user_lo = $1 THEN c.user_hi ELSE c.user_lo END \
             WHERE c.user_lo = $1 OR c.user_hi = $1 \
             ORDER BY lm.sent_at DESC LIMIT $2",
        )
        .bind(user.as_uuid())
        .bind(i64::from(limit))
        .fetch_all(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(rows
            .into_iter()
            .map(
                |(id, counterpart_handle, last_message, ts)| ConversationView {
                    conversation_id: Id::from_uuid(id),
                    counterpart_handle,
                    last_message,
                    last_at: Timestamp::from_offset(ts),
                },
            )
            .collect())
    }
}

/// Read-модель треда: сообщения диалога. Видимость — только участнику (фильтр в
/// `WHERE`: чужой наблюдатель получает пусто).
pub struct PgThreadReadModel {
    db: Db,
}

impl PgThreadReadModel {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ThreadReadModel for PgThreadReadModel {
    async fn messages(
        &self,
        conversation: ConversationId,
        viewer: UserId,
        limit: u32,
    ) -> Result<Vec<MessageView>, RepositoryError> {
        let rows: Vec<(Uuid, String, String, OffsetDateTime)> = sqlx::query_as(
            "SELECT m.id, u.handle, m.body, m.sent_at \
             FROM messages m \
             JOIN users u ON u.id = m.author_id \
             JOIN conversations c ON c.id = m.conversation_id \
             WHERE m.conversation_id = $1 AND (c.user_lo = $2 OR c.user_hi = $2) \
             ORDER BY m.sent_at ASC, m.id ASC LIMIT $3",
        )
        .bind(conversation.as_uuid())
        .bind(viewer.as_uuid())
        .bind(i64::from(limit))
        .fetch_all(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(rows
            .into_iter()
            .map(|(id, author_handle, body, ts)| MessageView {
                message_id: Id::from_uuid(id),
                author_handle,
                body,
                sent_at: Timestamp::from_offset(ts),
            })
            .collect())
    }
}
