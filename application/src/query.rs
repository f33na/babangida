//! Запросная сторона CQRS: read-модели под экраны, отдельные от агрегатов записи
//! (ADR-0004). View-DTO собираются адаптером (`infrastructure`) прямо из БД, не
//! реконструируя агрегаты. Сериализация в JSON добавится на границе `api`.

use babangida_domain::community::GroupId;
use babangida_domain::content::PostId;
use babangida_domain::identity::UserId;
use babangida_domain::marketplace::ListingId;
use babangida_domain::messaging::{ConversationId, MessageId};
use babangida_domain::music::TrackId;
use babangida_domain::verification::VerificationRequestId;
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

/// Read-модель ленты. `viewer` — текущий юзер для viewer-aware выдачи (посты его
/// закрытых групп видны); `None` — анонимная лента (только публичное).
#[async_trait::async_trait]
pub trait FeedReadModel: Send + Sync {
    async fn recent(
        &self,
        viewer: Option<UserId>,
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

/// Запрос свежей ленты. `viewer` — `Some` для viewer-aware ленты залогиненного
/// (видит посты своих закрытых групп), `None` — анонимная (только публичное).
pub struct RecentFeed {
    pub viewer: Option<UserId>,
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
        self.feed
            .recent(query.viewer, query.limit)
            .await
            .map_err(Into::into)
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

// --- marketplace: общий раздел и товары продавца (ADR-0010/0014) ---

/// Карточка товара под экран маркета/профиля.
#[derive(Debug, Clone)]
pub struct ListingView {
    pub listing_id: ListingId,
    pub seller: UserId,
    pub seller_handle: String,
    pub title: String,
    pub price_rubles: u64,
    pub description: Option<String>,
    pub status: String,
    pub created_at: Timestamp,
}

/// Read-модель товаров.
#[async_trait::async_trait]
pub trait ListingReadModel: Send + Sync {
    /// Активные товары — общий раздел маркета.
    async fn active(
        &self,
        limit: u32,
    ) -> Result<Vec<ListingView>, babangida_domain::RepositoryError>;
    /// Товары продавца по его handle — на профиль (анти-ВК).
    async fn by_seller(
        &self,
        handle: &str,
        limit: u32,
    ) -> Result<Vec<ListingView>, babangida_domain::RepositoryError>;
}

/// Запрос общего раздела маркета (активные товары).
pub struct MarketBrowse {
    pub limit: u32,
}

/// Use-case чтения маркета.
pub struct MarketQuery<R> {
    listings: R,
}

impl<R: ListingReadModel> MarketQuery<R> {
    pub fn new(listings: R) -> Self {
        Self { listings }
    }

    /// # Errors
    /// [`ApplicationError`] при сбое read-модели.
    pub async fn execute(&self, query: MarketBrowse) -> Result<Vec<ListingView>, ApplicationError> {
        self.listings.active(query.limit).await.map_err(Into::into)
    }
}

/// Запрос товаров продавца по handle.
pub struct SellerListings {
    pub handle: String,
    pub limit: u32,
}

/// Use-case чтения товаров продавца.
pub struct SellerListingsQuery<R> {
    listings: R,
}

impl<R: ListingReadModel> SellerListingsQuery<R> {
    pub fn new(listings: R) -> Self {
        Self { listings }
    }

    /// # Errors
    /// [`ApplicationError`] при сбое read-модели.
    pub async fn execute(
        &self,
        query: SellerListings,
    ) -> Result<Vec<ListingView>, ApplicationError> {
        self.listings
            .by_seller(&query.handle, query.limit)
            .await
            .map_err(Into::into)
    }
}

// --- verification: очередь админа и статус заявки (ADR-0016) ---

/// Заявка в очереди админа (ожидает рассмотрения).
#[derive(Debug, Clone)]
pub struct VerificationRequestView {
    pub request_id: VerificationRequestId,
    pub requester_handle: String,
    pub note: Option<String>,
    pub created_at: Timestamp,
}

/// Состояние заявки юзера под экран/бейдж (последняя по времени).
#[derive(Debug, Clone)]
pub struct MyVerificationView {
    pub request_id: VerificationRequestId,
    pub status: String,
    pub decision_reason: Option<String>,
    pub created_at: Timestamp,
    pub decided_at: Option<Timestamp>,
}

/// Read-модель заявок на верификацию.
#[async_trait::async_trait]
pub trait VerificationReadModel: Send + Sync {
    /// Очередь ожидающих рассмотрения (старые сверху) — экран админа.
    async fn pending(
        &self,
        limit: u32,
    ) -> Result<Vec<VerificationRequestView>, babangida_domain::RepositoryError>;
    /// Последняя заявка юзера, если есть — для статуса/бейджа в его UI.
    async fn latest_for(
        &self,
        requester: UserId,
    ) -> Result<Option<MyVerificationView>, babangida_domain::RepositoryError>;
}

/// Запрос очереди заявок (ожидающих).
pub struct PendingVerifications {
    pub limit: u32,
}

/// Use-case чтения очереди верификации (только админ — проверка прав на границе `api`).
pub struct PendingVerificationsQuery<R> {
    requests: R,
}

impl<R: VerificationReadModel> PendingVerificationsQuery<R> {
    pub fn new(requests: R) -> Self {
        Self { requests }
    }

    /// # Errors
    /// [`ApplicationError`] при сбое read-модели.
    pub async fn execute(
        &self,
        query: PendingVerifications,
    ) -> Result<Vec<VerificationRequestView>, ApplicationError> {
        self.requests.pending(query.limit).await.map_err(Into::into)
    }
}

/// Запрос статуса своей заявки.
pub struct MyVerificationOf {
    pub requester: UserId,
}

/// Use-case чтения статуса заявки текущего юзера.
pub struct MyVerificationQuery<R> {
    requests: R,
}

impl<R: VerificationReadModel> MyVerificationQuery<R> {
    pub fn new(requests: R) -> Self {
        Self { requests }
    }

    /// # Errors
    /// [`ApplicationError`] при сбое read-модели. `None` — заявок не было.
    pub async fn execute(
        &self,
        query: MyVerificationOf,
    ) -> Result<Option<MyVerificationView>, ApplicationError> {
        self.requests
            .latest_for(query.requester)
            .await
            .map_err(Into::into)
    }
}

// --- music: общий раздел и треки артиста (ADR-0010/0017) ---

/// Карточка трека под раздел музыки/профиль.
#[derive(Debug, Clone)]
pub struct TrackView {
    pub track_id: TrackId,
    pub uploader: UserId,
    pub artist_handle: String,
    pub title: String,
    pub audio_url: String,
    pub genre: Option<String>,
    pub status: String,
    pub created_at: Timestamp,
}

/// Read-модель треков.
#[async_trait::async_trait]
pub trait MusicReadModel: Send + Sync {
    /// Опубликованные треки — общий раздел музыки.
    async fn recent(&self, limit: u32)
    -> Result<Vec<TrackView>, babangida_domain::RepositoryError>;
    /// Треки артиста по его handle — на профиль (анти-ВК).
    async fn by_artist(
        &self,
        handle: &str,
        limit: u32,
    ) -> Result<Vec<TrackView>, babangida_domain::RepositoryError>;
}

/// Запрос общего раздела музыки (опубликованные треки).
pub struct MusicBrowse {
    pub limit: u32,
}

/// Use-case чтения раздела музыки.
pub struct MusicQuery<R> {
    tracks: R,
}

impl<R: MusicReadModel> MusicQuery<R> {
    pub fn new(tracks: R) -> Self {
        Self { tracks }
    }

    /// # Errors
    /// [`ApplicationError`] при сбое read-модели.
    pub async fn execute(&self, query: MusicBrowse) -> Result<Vec<TrackView>, ApplicationError> {
        self.tracks.recent(query.limit).await.map_err(Into::into)
    }
}

/// Запрос треков артиста по handle.
pub struct ArtistTracks {
    pub handle: String,
    pub limit: u32,
}

/// Use-case чтения треков артиста.
pub struct ArtistTracksQuery<R> {
    tracks: R,
}

impl<R: MusicReadModel> ArtistTracksQuery<R> {
    pub fn new(tracks: R) -> Self {
        Self { tracks }
    }

    /// # Errors
    /// [`ApplicationError`] при сбое read-модели.
    pub async fn execute(&self, query: ArtistTracks) -> Result<Vec<TrackView>, ApplicationError> {
        self.tracks
            .by_artist(&query.handle, query.limit)
            .await
            .map_err(Into::into)
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
        async fn recent(
            &self,
            _viewer: Option<UserId>,
            limit: u32,
        ) -> Result<Vec<FeedItemView>, RepositoryError> {
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
        let items = q
            .execute(RecentFeed {
                viewer: None,
                limit: 2,
            })
            .await
            .unwrap();
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
