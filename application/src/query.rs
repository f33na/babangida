//! Запросная сторона CQRS: read-модели под экраны, отдельные от агрегатов записи
//! (ADR-0004). View-DTO собираются адаптером (`infrastructure`) прямо из БД, не
//! реконструируя агрегаты. Сериализация в JSON добавится на границе `api`.

use babangida_domain::content::PostId;
use babangida_domain::identity::UserId;
use babangida_shared::Timestamp;

use crate::ApplicationError;

/// Элемент ленты под экран.
#[derive(Debug, Clone)]
pub struct FeedItemView {
    pub post_id: PostId,
    pub author: UserId,
    pub author_handle: String,
    pub body: String,
    pub created_at: Timestamp,
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
}
