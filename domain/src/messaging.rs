//! Контекст messaging: личная переписка (DM) между двумя участниками. Доступна
//! всем без верификации (ADR-0010: лента/профиль/переписка — для casual). Групповые
//! сообщения — это уже сообщества ([`crate::community`]), не этот контекст. Тред
//! читается read-моделью CQRS (`application`); здесь — запись и её инвариант.

use async_trait::async_trait;
use babangida_shared::{Id, Timestamp};

use crate::RepositoryError;
use crate::identity::UserId;

/// Тело сообщения. 1..=4000 символов после обрезки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageBody(String);

impl MessageBody {
    /// Максимальная длина.
    pub const MAX_LEN: usize = 4000;

    /// Распарсить тело сообщения.
    ///
    /// # Errors
    /// [`MessageBodyError`], если пусто или длиннее [`MessageBody::MAX_LEN`].
    pub fn parse(input: &str) -> Result<Self, MessageBodyError> {
        let body = input.trim();
        if body.is_empty() {
            return Err(MessageBodyError::Empty);
        }
        let len = body.chars().count();
        if len > Self::MAX_LEN {
            return Err(MessageBodyError::TooLong { len });
        }
        Ok(Self(body.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Нарушение правил [`MessageBody`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MessageBodyError {
    #[error("сообщение пустое")]
    Empty,
    #[error("сообщение слишком длинное: {len} символов")]
    TooLong { len: usize },
}

/// Фантомный маркер для типизированного [`ConversationId`].
pub enum ConversationMarker {}
/// Идентификатор диалога.
pub type ConversationId = Id<ConversationMarker>;

/// Фантомный маркер для типизированного [`MessageId`].
pub enum MessageMarker {}
/// Идентификатор сообщения.
pub type MessageId = Id<MessageMarker>;

/// Диалог двух участников (DM) — корень агрегата. Пара участников канонизируется
/// (меньший UUID первым), поэтому `(a, b)` и `(b, a)` — один и тот же диалог;
/// уникальность пары на хранилище держит индекс в `infrastructure`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    id: ConversationId,
    lower: UserId,
    higher: UserId,
    opened_at: Timestamp,
}

impl Conversation {
    /// Открыть диалог между `a` и `b`.
    ///
    /// # Errors
    /// [`MessagingError::SelfConversation`], если `a == b`.
    pub fn open(
        id: ConversationId,
        a: UserId,
        b: UserId,
        now: Timestamp,
    ) -> Result<(Self, ConversationOpened), MessagingError> {
        if a == b {
            return Err(MessagingError::SelfConversation);
        }
        let (lower, higher) = Self::canonical(a, b);
        let conversation = Self {
            id,
            lower,
            higher,
            opened_at: now,
        };
        let event = ConversationOpened {
            conversation: id,
            participants: (lower, higher),
            opened_at: now,
        };
        Ok((conversation, event))
    }

    /// Отправить сообщение в диалог. Автор обязан быть участником — единственный
    /// инвариант записи переписки.
    ///
    /// # Errors
    /// [`MessagingError::NotParticipant`], если `author` не в этом диалоге.
    pub fn send(
        &self,
        author: UserId,
        body: MessageBody,
        message_id: MessageId,
        now: Timestamp,
    ) -> Result<(Message, MessageSent), MessagingError> {
        if !self.has_participant(author) {
            return Err(MessagingError::NotParticipant);
        }
        let message = Message {
            id: message_id,
            conversation: self.id,
            author,
            body,
            sent_at: now,
        };
        let event = MessageSent {
            message_id,
            conversation: self.id,
            author,
            sent_at: now,
        };
        Ok((message, event))
    }

    fn canonical(a: UserId, b: UserId) -> (UserId, UserId) {
        if a.as_uuid() <= b.as_uuid() {
            (a, b)
        } else {
            (b, a)
        }
    }

    /// Участвует ли юзер в диалоге.
    #[must_use]
    pub fn has_participant(&self, user: UserId) -> bool {
        self.lower == user || self.higher == user
    }

    /// Собеседник относительно `user` (если он участник).
    #[must_use]
    pub fn counterpart(&self, user: UserId) -> Option<UserId> {
        if user == self.lower {
            Some(self.higher)
        } else if user == self.higher {
            Some(self.lower)
        } else {
            None
        }
    }

    #[must_use]
    pub const fn id(&self) -> ConversationId {
        self.id
    }

    /// Канонизированная пара участников (меньший UUID первым).
    #[must_use]
    pub const fn participants(&self) -> (UserId, UserId) {
        (self.lower, self.higher)
    }

    #[must_use]
    pub const fn opened_at(&self) -> Timestamp {
        self.opened_at
    }
}

/// Сообщение — запись в диалоге. Создаётся только через [`Conversation::send`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    id: MessageId,
    conversation: ConversationId,
    author: UserId,
    body: MessageBody,
    sent_at: Timestamp,
}

impl Message {
    #[must_use]
    pub const fn id(&self) -> MessageId {
        self.id
    }

    #[must_use]
    pub const fn conversation(&self) -> ConversationId {
        self.conversation
    }

    #[must_use]
    pub const fn author(&self) -> UserId {
        self.author
    }

    #[must_use]
    pub fn body(&self) -> &MessageBody {
        &self.body
    }

    #[must_use]
    pub const fn sent_at(&self) -> Timestamp {
        self.sent_at
    }
}

/// Нарушение правил переписки.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MessagingError {
    #[error("нельзя открыть диалог с самим собой")]
    SelfConversation,
    #[error("автор не участник диалога")]
    NotParticipant,
}

/// Диалог открыт.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationOpened {
    pub conversation: ConversationId,
    pub participants: (UserId, UserId),
    pub opened_at: Timestamp,
}

/// Сообщение отправлено.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageSent {
    pub message_id: MessageId,
    pub conversation: ConversationId,
    pub author: UserId,
    pub sent_at: Timestamp,
}

/// Доменное событие контекста messaging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessagingEvent {
    ConversationOpened(ConversationOpened),
    MessageSent(MessageSent),
}

impl From<ConversationOpened> for MessagingEvent {
    fn from(event: ConversationOpened) -> Self {
        Self::ConversationOpened(event)
    }
}

impl From<MessageSent> for MessagingEvent {
    fn from(event: MessageSent) -> Self {
        Self::MessageSent(event)
    }
}

/// Хранилище диалогов (порт; реализация — в `infrastructure`).
#[async_trait]
pub trait ConversationRepository: Send + Sync {
    async fn find_by_id(&self, id: ConversationId)
    -> Result<Option<Conversation>, RepositoryError>;
    /// Найти диалог между двумя юзерами (порядок не важен — пара канонизируется).
    async fn find_between(
        &self,
        a: UserId,
        b: UserId,
    ) -> Result<Option<Conversation>, RepositoryError>;
    async fn save(&self, conversation: &Conversation) -> Result<(), RepositoryError>;
}

/// Хранилище сообщений (порт записи; чтение треда — read-модель в `application`).
#[async_trait]
pub trait MessageRepository: Send + Sync {
    async fn append(&self, message: &Message) -> Result<(), RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid() -> UserId {
        Id::generate()
    }

    fn body(text: &str) -> MessageBody {
        MessageBody::parse(text).expect("валидное тело")
    }

    #[test]
    fn message_body_trims_and_limits() {
        assert_eq!(MessageBody::parse("  йоу  ").unwrap().as_str(), "йоу");
        assert_eq!(MessageBody::parse("   "), Err(MessageBodyError::Empty));
        assert!(matches!(
            MessageBody::parse(&"a".repeat(4001)),
            Err(MessageBodyError::TooLong { len: 4001 })
        ));
    }

    #[test]
    fn open_rejects_self_conversation() {
        let me = uid();
        assert_eq!(
            Conversation::open(Id::generate(), me, me, Timestamp::now()).unwrap_err(),
            MessagingError::SelfConversation
        );
    }

    #[test]
    fn open_canonicalizes_participants_order() {
        let now = Timestamp::now();
        let a = uid();
        let b = uid();
        let (ab, _) = Conversation::open(Id::generate(), a, b, now).unwrap();
        let (ba, _) = Conversation::open(Id::generate(), b, a, now).unwrap();
        assert_eq!(ab.participants(), ba.participants());
        assert!(ab.has_participant(a) && ab.has_participant(b));
    }

    #[test]
    fn send_by_participant_produces_message_and_event() {
        let now = Timestamp::now();
        let a = uid();
        let b = uid();
        let (conv, _) = Conversation::open(Id::generate(), a, b, now).unwrap();
        let (message, event) = conv
            .send(a, body("здарова"), Id::generate(), now)
            .expect("участник может писать");
        assert_eq!(message.author(), a);
        assert_eq!(message.conversation(), conv.id());
        assert_eq!(event.author, a);
        assert_eq!(conv.counterpart(a), Some(b));
    }

    #[test]
    fn send_by_outsider_is_rejected() {
        let now = Timestamp::now();
        let (conv, _) = Conversation::open(Id::generate(), uid(), uid(), now).unwrap();
        let outsider = uid();
        assert_eq!(
            conv.send(outsider, body("вброс"), Id::generate(), now)
                .unwrap_err(),
            MessagingError::NotParticipant
        );
        assert_eq!(conv.counterpart(outsider), None);
    }
}
