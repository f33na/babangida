//! Запросная сторона CQRS: read-модели под экраны, отдельные от агрегатов записи
//! (ADR-0004). View-DTO собираются адаптером (`infrastructure`) прямо из БД, не
//! реконструируя агрегаты. Сериализация в JSON добавится на границе `api`.

use babangida_domain::community::GroupId;
use babangida_domain::content::PostId;
use babangida_domain::identity::UserId;
use babangida_domain::messaging::{ConversationId, MessageId};
use babangida_shared::Timestamp;

use crate::ApplicationError;

/// Элемент ленты под экран. `group_*` заполнены, если пост опубликован в
/// сообщество (анти-ВК: посты сообществ — в общей ленте, ADR-0012).
#[derive(Debug, Clone)]
pub struct FeedItemView {
    pub post_id: PostId,
    pub author: UserId,
    pub author_handle: String,
    pub body: String,
    pub created_at: Timestamp,
    pub group_slug: Option<String>,
    pub group_name: Option<String>,
}

/// Профиль под экран.
#[derive(Debug, Clone)]
pub struct ProfileView {
    pub user_id: UserId,
    pub handle: String,
    pub display_name: String,
    pub subculture: String,
    pub bio: Option<String>,
    pub verified: bool,
}

/// Read-модель ленты.
#[async_trait::async_trait]
pub trait FeedReadModel: Send + Sync {
    async fn recent(
        &self,
        limit: u32,
    ) -> Result<Vec<FeedItemView>, babangida_domain::RepositoryError>;
}

/// Read-модель профиля.
#[async_trait::async_trait]
pub trait ProfileReadModel: Send + Sync {
    async fn by_handle(
        &self,
        handle: &str,
    ) -> Result<Option<ProfileView>, babangida_domain::RepositoryError>;
}

/// Запрос свежей ленты.
pub struct RecentFeed {
    pub limit: u32,
}

/// Use-case чтения ленты.
pub struct FeedQuery<R> {
    feed: R,
}

impl<R: FeedReadModel> FeedQuery<R> {
    pub fn new(feed: R) -> Self {
        Self { feed }
    }

    /// # Errors
    /// [`ApplicationError`] при сбое read-модели.
    pub async fn execute(&self, query: RecentFeed) -> Result<Vec<FeedItemView>, ApplicationError> {
        self.feed.recent(query.limit).await.map_err(Into::into)
    }
}

/// Запрос профиля по handle.
pub struct ProfileByHandle {
    pub handle: String,
}

/// Use-case чтения профиля.
pub struct ProfileQuery<R> {
    profiles: R,
}

impl<R: ProfileReadModel> ProfileQuery<R> {
    pub fn new(profiles: R) -> Self {
        Self { profiles }
    }

    /// # Errors
    /// [`ApplicationError::NotFound`], если профиля нет, либо сбой read-модели.
    pub async fn execute(&self, query: ProfileByHandle) -> Result<ProfileView, ApplicationError> {
        self.profiles
            .by_handle(&query.handle)
            .await?
            .ok_or(ApplicationError::NotFound("profile"))
    }
}

// --- messaging: список диалогов и тред ---

/// Превью диалога в списке переписок.
#[derive(Debug, Clone)]
pub struct ConversationView {
    pub conversation_id: ConversationId,
    pub counterpart_handle: String,
    pub last_message: String,
    pub last_at: Timestamp,
}

/// Сообщение в треде.
#[derive(Debug, Clone)]
pub struct MessageView {
    pub message_id: MessageId,
    pub author_handle: String,
    pub body: String,
    pub sent_at: Timestamp,
}

/// Read-модель списка диалогов юзера (входящие).
#[async_trait::async_trait]
pub trait InboxReadModel: Send + Sync {
    async fn for_user(
        &self,
        user: UserId,
        limit: u32,
    ) -> Result<Vec<ConversationView>, babangida_domain::RepositoryError>;
}

/// Read-модель треда. Видимость ограничена участником: адаптер возвращает
/// сообщения, только если `viewer` — участник диалога (иначе пусто).
#[async_trait::async_trait]
pub trait ThreadReadModel: Send + Sync {
    async fn messages(
        &self,
        conversation: ConversationId,
        viewer: UserId,
        limit: u32,
    ) -> Result<Vec<MessageView>, babangida_domain::RepositoryError>;
}

/// Запрос списка диалогов.
pub struct InboxOf {
    pub user: UserId,
    pub limit: u32,
}

/// Use-case чтения входящих.
pub struct InboxQuery<R> {
    inbox: R,
}

impl<R: InboxReadModel> InboxQuery<R> {
    pub fn new(inbox: R) -> Self {
        Self { inbox }
    }

    /// # Errors
    /// [`ApplicationError`] при сбое read-модели.
    pub async fn execute(&self, query: InboxOf) -> Result<Vec<ConversationView>, ApplicationError> {
        self.inbox
            .for_user(query.user, query.limit)
            .await
            .map_err(Into::into)
    }
}

/// Запрос треда диалога от имени наблюдателя.
pub struct ThreadOf {
    pub conversation: ConversationId,
    pub viewer: UserId,
    pub limit: u32,
}

/// Use-case чтения треда.
pub struct ThreadQuery<R> {
    thread: R,
}

impl<R: ThreadReadModel> ThreadQuery<R> {
    pub fn new(thread: R) -> Self {
        Self { thread }
    }

    /// # Errors
    /// [`ApplicationError`] при сбое read-модели.
    pub async fn execute(&self, query: ThreadOf) -> Result<Vec<MessageView>, ApplicationError> {
        self.thread
            .messages(query.conversation, query.viewer, query.limit)
            .await
            .map_err(Into::into)
    }
}

// --- community: карточка сообщества ---

/// Сообщество под экран.
#[derive(Debug, Clone)]
pub struct GroupView {
    pub group_id: GroupId,
    pub slug: String,
    pub name: String,
    pub kind: String,
    pub member_count: u32,
}

/// Read-модель сообщества.
#[async_trait::async_trait]
pub trait GroupReadModel: Send + Sync {
    async fn by_slug(
        &self,
        slug: &str,
    ) -> Result<Option<GroupView>, babangida_domain::RepositoryError>;
}

/// Запрос сообщества по слагу.
pub struct GroupBySlug {
    pub slug: String,
}

/// Use-case чтения сообщества.
pub struct GroupQuery<R> {
    groups: R,
}

impl<R: GroupReadModel> GroupQuery<R> {
    pub fn new(groups: R) -> Self {
        Self { groups }
    }

    /// # Errors
    /// [`ApplicationError::NotFound`], если сообщества нет, либо сбой read-модели.
    pub async fn execute(&self, query: GroupBySlug) -> Result<GroupView, ApplicationError> {
        self.groups
            .by_slug(&query.slug)
            .await?
            .ok_or(ApplicationError::NotFound("group"))
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use babangida_domain::RepositoryError;
    use babangida_shared::{Id, Timestamp};

    use super::*;

    struct FakeFeed(Vec<FeedItemView>);
    #[async_trait]
    impl FeedReadModel for FakeFeed {
        async fn recent(&self, limit: u32) -> Result<Vec<FeedItemView>, RepositoryError> {
            Ok(self.0.iter().take(limit as usize).cloned().collect())
        }
    }

    struct FakeProfiles(Option<ProfileView>);
    #[async_trait]
    impl ProfileReadModel for FakeProfiles {
        async fn by_handle(&self, _handle: &str) -> Result<Option<ProfileView>, RepositoryError> {
            Ok(self.0.clone())
        }
    }

    fn feed_item() -> FeedItemView {
        FeedItemView {
            post_id: Id::generate(),
            author: Id::generate(),
            author_handle: "rapper_one".to_owned(),
            body: "йоу".to_owned(),
            created_at: Timestamp::now(),
            group_slug: None,
            group_name: None,
        }
    }

    #[tokio::test]
    async fn feed_query_respects_limit() {
        let q = FeedQuery::new(FakeFeed(vec![feed_item(), feed_item(), feed_item()]));
        let items = q.execute(RecentFeed { limit: 2 }).await.unwrap();
        assert_eq!(items.len(), 2);
    }

    #[tokio::test]
    async fn profile_query_missing_is_not_found() {
        let q = ProfileQuery::new(FakeProfiles(None));
        let err = q
            .execute(ProfileByHandle {
                handle: "ghost".to_owned(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ApplicationError::NotFound("profile")));
    }

    struct FakeThread(Vec<MessageView>);
    #[async_trait]
    impl ThreadReadModel for FakeThread {
        async fn messages(
            &self,
            _conversation: ConversationId,
            _viewer: UserId,
            limit: u32,
        ) -> Result<Vec<MessageView>, RepositoryError> {
            Ok(self.0.iter().take(limit as usize).cloned().collect())
        }
    }

    struct FakeGroupRead(Option<GroupView>);
    #[async_trait]
    impl GroupReadModel for FakeGroupRead {
        async fn by_slug(&self, _slug: &str) -> Result<Option<GroupView>, RepositoryError> {
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn thread_query_respects_limit() {
        let msg = MessageView {
            message_id: Id::generate(),
            author_handle: "mc".to_owned(),
            body: "йоу".to_owned(),
            sent_at: Timestamp::now(),
        };
        let q = ThreadQuery::new(FakeThread(vec![msg.clone(), msg.clone(), msg]));
        let items = q
            .execute(ThreadOf {
                conversation: Id::generate(),
                viewer: Id::generate(),
                limit: 2,
            })
            .await
            .unwrap();
        assert_eq!(items.len(), 2);
    }

    #[tokio::test]
    async fn group_query_missing_is_not_found() {
        let q = GroupQuery::new(FakeGroupRead(None));
        let err = q
            .execute(GroupBySlug {
                slug: "ghost".to_owned(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ApplicationError::NotFound("group")));
    }
}
