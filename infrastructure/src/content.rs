//! Postgres-адаптеры контекста content и read-модели CQRS (лента, профиль).

use async_trait::async_trait;
use babangida_application::query::{FeedItemView, FeedReadModel, ProfileReadModel, ProfileView};
use babangida_domain::RepositoryError;
use babangida_domain::content::{Post, PostBody, PostId, PostRepository};
use babangida_domain::identity::UserId;
use babangida_shared::{Id, Timestamp};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::map_sqlx;
use crate::pool::Db;

fn row_to_post(
    id: Uuid,
    author: Uuid,
    body: String,
    created_at: OffsetDateTime,
) -> Result<Post, RepositoryError> {
    let body = PostBody::parse(&body)
        .map_err(|_| RepositoryError::Unavailable("повреждённый пост в БД".to_owned()))?;
    Ok(Post::create(
        Id::from_uuid(id),
        Id::from_uuid(author),
        body,
        Timestamp::from_offset(created_at),
    ))
}

/// Репозиторий постов на Postgres.
pub struct PgPostRepository {
    db: Db,
}

impl PgPostRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl PostRepository for PgPostRepository {
    async fn find_by_id(&self, id: PostId) -> Result<Option<Post>, RepositoryError> {
        let row: Option<(Uuid, String, OffsetDateTime)> =
            sqlx::query_as("SELECT author_id, body, created_at FROM posts WHERE id = $1")
                .bind(id.as_uuid())
                .fetch_optional(&self.db)
                .await
                .map_err(map_sqlx)?;
        row.map(|(author, body, ts)| row_to_post(id.as_uuid(), author, body, ts))
            .transpose()
    }

    async fn save(&self, post: &Post) -> Result<(), RepositoryError> {
        sqlx::query(
            "INSERT INTO posts (id, author_id, body, created_at) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(post.id().as_uuid())
        .bind(post.author().as_uuid())
        .bind(post.body().as_str())
        .bind(post.created_at().into_offset())
        .execute(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }
}

/// Read-модель ленты: свежие посты с handle автора (ADR-0004).
pub struct PgFeedReadModel {
    db: Db,
}

impl PgFeedReadModel {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl FeedReadModel for PgFeedReadModel {
    async fn recent(
        &self,
        viewer: Option<UserId>,
        limit: u32,
    ) -> Result<Vec<FeedItemView>, RepositoryError> {
        // Анти-ВК: посты сообществ — в общей ленте (ADR-0012). Личные посты (без
        // связи) и посты пабликов видны всем; пост закрытой группы виден только её
        // участнику ($1) — зеркало доменного read-правила (closed = участники). При
        // анонимной выдаче ($1 IS NULL) закрытые группы отсекаются.
        let rows: Vec<(
            Uuid,
            Uuid,
            String,
            String,
            OffsetDateTime,
            Option<String>,
            Option<String>,
        )> = sqlx::query_as(
            "SELECT p.id, p.author_id, u.handle, p.body, p.created_at, g.slug, g.name \
             FROM posts p \
             JOIN users u ON u.id = p.author_id \
             LEFT JOIN group_posts gp ON gp.post_id = p.id \
             LEFT JOIN groups g ON g.id = gp.group_id \
             WHERE gp.post_id IS NULL \
                OR g.kind = 'public' \
                OR ($1::uuid IS NOT NULL AND EXISTS ( \
                     SELECT 1 FROM group_members m \
                     WHERE m.group_id = g.id AND m.user_id = $1)) \
             ORDER BY p.created_at DESC, p.id DESC LIMIT $2",
        )
        .bind(viewer.map(|v| v.as_uuid()))
        .bind(i64::from(limit))
        .fetch_all(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(rows
            .into_iter()
            .map(
                |(post_id, author, author_handle, body, ts, group_slug, group_name)| FeedItemView {
                    post_id: Id::from_uuid(post_id),
                    author: Id::from_uuid(author),
                    author_handle,
                    body,
                    created_at: Timestamp::from_offset(ts),
                    group_slug,
                    group_name,
                },
            )
            .collect())
    }
}

/// Read-модель профиля по handle (ADR-0004).
pub struct PgProfileReadModel {
    db: Db,
}

impl PgProfileReadModel {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ProfileReadModel for PgProfileReadModel {
    async fn by_handle(&self, handle: &str) -> Result<Option<ProfileView>, RepositoryError> {
        let row: Option<(Uuid, String, String, String, Option<String>, String)> = sqlx::query_as(
            "SELECT u.id, u.handle, p.display_name, p.subculture, p.bio, u.verified \
             FROM users u JOIN profiles p ON p.user_id = u.id WHERE u.handle = $1",
        )
        .bind(handle)
        .fetch_optional(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(row.map(
            |(id, handle, display_name, subculture, bio, verified)| ProfileView {
                user_id: Id::from_uuid(id),
                handle,
                display_name,
                subculture,
                bio,
                verified: verified == "verified",
            },
        ))
    }
}
