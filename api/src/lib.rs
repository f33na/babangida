//! HTTP-слой babangida (Axum) и точка композиции зависимостей. Хендлеры парсят
//! ввод в доменные value objects, зовут use-cases `application` и переводят
//! результат/ошибку в HTTP. Доменные правила (квота/кулдаун) НЕ дублируются —
//! их проверяет домен (ADR-0003/0005). См. `../../babangida-vault/COMMON.md`.

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use babangida_application::ApplicationError;
use babangida_application::command::{
    CreatePost, CreatePostCommand, FoundGroup, FoundGroupCommand, IssueInvite, IssueInviteCommand,
    JoinGroup, JoinGroupCommand, LeaveGroup, LeaveGroupCommand, PostToGroup, PostToGroupCommand,
    Register, RegisterCommand, SendMessage, SendMessageCommand, SetMemberRole,
    SetMemberRoleCommand,
};
use babangida_application::query::{
    FeedQuery, GroupBySlug, GroupQuery, GroupView, InboxOf, InboxQuery, ProfileByHandle,
    ProfileQuery, ProfileView, RecentFeed, ThreadOf, ThreadQuery,
};
use babangida_domain::RepositoryError;
use babangida_domain::community::{
    CommunityError, GroupId, GroupKind, GroupName, GroupSlug, MembershipRole,
};
use babangida_domain::content::PostBody;
use babangida_domain::identity::{Handle, InviteCode, UserId};
use babangida_domain::messaging::{ConversationId, MessageBody, MessagingError};
use babangida_domain::social::{DisplayName, Subculture};
use babangida_infrastructure::{
    Db, PgConversationRepository, PgFeedReadModel, PgGroupMembershipTxFactory,
    PgGroupPostRepository, PgGroupReadModel, PgGroupRepository, PgInboxReadModel,
    PgIssueInviteTxFactory, PgMessageRepository, PgPostRepository, PgProfileReadModel,
    PgRegistrationTxFactory, PgThreadReadModel, RandomInviteCodeFactory, SystemClock,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Состояние приложения: пул БД. Адаптеры собираются в хендлерах из него.
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
}

/// Роутер HTTP-API первого среза.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/invites", post(issue_invite))
        .route("/register", post(register))
        .route("/posts", post(create_post))
        .route("/profiles/{handle}", get(profile))
        .route("/feed", get(feed))
        // messaging
        .route("/messages", post(send_message))
        .route("/inbox", get(inbox))
        .route("/conversations/{id}/thread", get(thread))
        // community (карточка — по слагу; членские действия — по id)
        .route("/groups", post(found_group))
        .route("/groups/{slug}", get(group_view))
        .route("/groups/{id}/join", post(join_group))
        .route("/groups/{id}/leave", post(leave_group))
        .route("/groups/{id}/role", post(set_role))
        .route("/groups/{id}/posts", post(post_to_group))
        .with_state(state)
}

// --- ошибки → HTTP ---

enum ApiError {
    /// Невалидный ввод (нарушение инварианта value object) → 422.
    Validation(String),
    /// Ошибка use-case'а.
    App(ApplicationError),
}

impl From<ApplicationError> for ApiError {
    fn from(err: ApplicationError) -> Self {
        Self::App(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Validation(msg) => (StatusCode::UNPROCESSABLE_ENTITY, msg),
            Self::App(err) => app_error_response(err),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

fn app_error_response(err: ApplicationError) -> (StatusCode, String) {
    match err {
        // Нарушение доменного правила инвайта — конфликт с текущим состоянием.
        ApplicationError::Invite(e) => (StatusCode::CONFLICT, e.to_string()),
        // Переписка: само-диалог — некорректный ввод; чужой диалог — запрещено.
        ApplicationError::Messaging(e @ MessagingError::SelfConversation) => {
            (StatusCode::UNPROCESSABLE_ENTITY, e.to_string())
        }
        ApplicationError::Messaging(e @ MessagingError::NotParticipant) => {
            (StatusCode::FORBIDDEN, e.to_string())
        }
        // Сообщества: нет прав — запрещено; цели нет — 404; остальное — конфликт состояния.
        ApplicationError::Community(e @ CommunityError::NotPermitted) => {
            (StatusCode::FORBIDDEN, e.to_string())
        }
        ApplicationError::Community(
            e @ (CommunityError::TargetNotMember | CommunityError::NotMember),
        ) => (StatusCode::NOT_FOUND, e.to_string()),
        ApplicationError::Community(e) => (StatusCode::CONFLICT, e.to_string()),
        ApplicationError::NotFound(what) => (StatusCode::NOT_FOUND, format!("не найдено: {what}")),
        ApplicationError::Repository(RepositoryError::NotFound) => {
            (StatusCode::NOT_FOUND, "не найдено".to_owned())
        }
        ApplicationError::Repository(RepositoryError::Conflict) => {
            (StatusCode::CONFLICT, "конфликт".to_owned())
        }
        // Деталь хранилища наружу не отдаём.
        ApplicationError::Repository(RepositoryError::Unavailable(_)) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "хранилище недоступно".to_owned(),
        ),
    }
}

fn invalid(err: impl std::fmt::Display) -> ApiError {
    ApiError::Validation(err.to_string())
}

// --- эндпоинты ---

#[derive(Deserialize)]
struct IssueReq {
    inviter: String,
}

#[derive(Serialize)]
struct IssueRes {
    invite_id: String,
    code: String,
    issued_at: i64,
}

async fn issue_invite(
    State(st): State<AppState>,
    Json(req): Json<IssueReq>,
) -> Result<Json<IssueRes>, ApiError> {
    let inviter = UserId::parse(&req.inviter).map_err(invalid)?;
    let uc = IssueInvite::new(
        PgIssueInviteTxFactory::new(st.db.clone()),
        SystemClock,
        RandomInviteCodeFactory,
    );
    let event = uc.execute(IssueInviteCommand { inviter }).await?;
    Ok(Json(IssueRes {
        invite_id: event.invite_id.to_string(),
        code: event.code.as_str().to_owned(),
        issued_at: event.issued_at.into_offset().unix_timestamp(),
    }))
}

#[derive(Deserialize)]
struct RegisterReq {
    code: String,
    handle: String,
    display_name: String,
    subculture: String,
}

#[derive(Serialize)]
struct RegisterRes {
    user_id: String,
    handle: String,
}

async fn register(
    State(st): State<AppState>,
    Json(req): Json<RegisterReq>,
) -> Result<Json<RegisterRes>, ApiError> {
    let cmd = RegisterCommand {
        code: InviteCode::parse(&req.code).map_err(invalid)?,
        handle: Handle::parse(&req.handle).map_err(invalid)?,
        display_name: DisplayName::parse(&req.display_name).map_err(invalid)?,
        subculture: Subculture::parse(&req.subculture).map_err(invalid)?,
    };
    let uc = Register::new(PgRegistrationTxFactory::new(st.db.clone()), SystemClock);
    let user = uc.execute(cmd).await?;
    Ok(Json(RegisterRes {
        user_id: user.id().to_string(),
        handle: user.handle().as_str().to_owned(),
    }))
}

#[derive(Deserialize)]
struct PostReq {
    author: String,
    body: String,
}

#[derive(Serialize)]
struct PostRes {
    post_id: String,
    created_at: i64,
}

async fn create_post(
    State(st): State<AppState>,
    Json(req): Json<PostReq>,
) -> Result<Json<PostRes>, ApiError> {
    let cmd = CreatePostCommand {
        author: UserId::parse(&req.author).map_err(invalid)?,
        body: PostBody::parse(&req.body).map_err(invalid)?,
    };
    let uc = CreatePost::new(PgPostRepository::new(st.db.clone()), SystemClock);
    let post = uc.execute(cmd).await?;
    Ok(Json(PostRes {
        post_id: post.id().to_string(),
        created_at: post.created_at().into_offset().unix_timestamp(),
    }))
}

#[derive(Serialize)]
struct ProfileRes {
    user_id: String,
    handle: String,
    display_name: String,
    subculture: String,
    bio: Option<String>,
    verified: bool,
}

impl From<ProfileView> for ProfileRes {
    fn from(v: ProfileView) -> Self {
        Self {
            user_id: v.user_id.to_string(),
            handle: v.handle,
            display_name: v.display_name,
            subculture: v.subculture,
            bio: v.bio,
            verified: v.verified,
        }
    }
}

async fn profile(
    State(st): State<AppState>,
    Path(handle): Path<String>,
) -> Result<Json<ProfileRes>, ApiError> {
    let uc = ProfileQuery::new(PgProfileReadModel::new(st.db.clone()));
    let view = uc.execute(ProfileByHandle { handle }).await?;
    Ok(Json(view.into()))
}

#[derive(Deserialize)]
struct FeedParams {
    limit: Option<u32>,
}

#[derive(Serialize)]
struct FeedItemRes {
    post_id: String,
    author: String,
    author_handle: String,
    body: String,
    created_at: i64,
    /// Слаг сообщества, если пост опубликован в него (анти-ВК); иначе `null`.
    group_slug: Option<String>,
    group_name: Option<String>,
}

async fn feed(
    State(st): State<AppState>,
    Query(params): Query<FeedParams>,
) -> Result<Json<Vec<FeedItemRes>>, ApiError> {
    let uc = FeedQuery::new(PgFeedReadModel::new(st.db.clone()));
    let items = uc
        .execute(RecentFeed {
            limit: params.limit.unwrap_or(50),
        })
        .await?;
    let out = items
        .into_iter()
        .map(|i| FeedItemRes {
            post_id: i.post_id.to_string(),
            author: i.author.to_string(),
            author_handle: i.author_handle,
            body: i.body,
            created_at: i.created_at.into_offset().unix_timestamp(),
            group_slug: i.group_slug,
            group_name: i.group_name,
        })
        .collect();
    Ok(Json(out))
}

// --- messaging ---

#[derive(Deserialize)]
struct SendMessageReq {
    author: String,
    recipient: String,
    body: String,
}

#[derive(Serialize)]
struct SendMessageRes {
    message_id: String,
    conversation_id: String,
    sent_at: i64,
}

async fn send_message(
    State(st): State<AppState>,
    Json(req): Json<SendMessageReq>,
) -> Result<Json<SendMessageRes>, ApiError> {
    let cmd = SendMessageCommand {
        author: UserId::parse(&req.author).map_err(invalid)?,
        recipient: UserId::parse(&req.recipient).map_err(invalid)?,
        body: MessageBody::parse(&req.body).map_err(invalid)?,
    };
    let uc = SendMessage::new(
        PgConversationRepository::new(st.db.clone()),
        PgMessageRepository::new(st.db.clone()),
        SystemClock,
    );
    let event = uc.execute(cmd).await?;
    Ok(Json(SendMessageRes {
        message_id: event.message_id.to_string(),
        conversation_id: event.conversation.to_string(),
        sent_at: event.sent_at.into_offset().unix_timestamp(),
    }))
}

#[derive(Deserialize)]
struct InboxParams {
    user: String,
    limit: Option<u32>,
}

#[derive(Serialize)]
struct ConversationRes {
    conversation_id: String,
    counterpart_handle: String,
    last_message: String,
    last_at: i64,
}

async fn inbox(
    State(st): State<AppState>,
    Query(params): Query<InboxParams>,
) -> Result<Json<Vec<ConversationRes>>, ApiError> {
    let user = UserId::parse(&params.user).map_err(invalid)?;
    let uc = InboxQuery::new(PgInboxReadModel::new(st.db.clone()));
    let items = uc
        .execute(InboxOf {
            user,
            limit: params.limit.unwrap_or(50),
        })
        .await?;
    let out = items
        .into_iter()
        .map(|c| ConversationRes {
            conversation_id: c.conversation_id.to_string(),
            counterpart_handle: c.counterpart_handle,
            last_message: c.last_message,
            last_at: c.last_at.into_offset().unix_timestamp(),
        })
        .collect();
    Ok(Json(out))
}

#[derive(Deserialize)]
struct ThreadParams {
    viewer: String,
    limit: Option<u32>,
}

#[derive(Serialize)]
struct MessageRes {
    message_id: String,
    author_handle: String,
    body: String,
    sent_at: i64,
}

async fn thread(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<ThreadParams>,
) -> Result<Json<Vec<MessageRes>>, ApiError> {
    let conversation = ConversationId::parse(&id).map_err(invalid)?;
    let viewer = UserId::parse(&params.viewer).map_err(invalid)?;
    let uc = ThreadQuery::new(PgThreadReadModel::new(st.db.clone()));
    let items = uc
        .execute(ThreadOf {
            conversation,
            viewer,
            limit: params.limit.unwrap_or(100),
        })
        .await?;
    let out = items
        .into_iter()
        .map(|m| MessageRes {
            message_id: m.message_id.to_string(),
            author_handle: m.author_handle,
            body: m.body,
            sent_at: m.sent_at.into_offset().unix_timestamp(),
        })
        .collect();
    Ok(Json(out))
}

// --- community ---

#[derive(Deserialize)]
struct FoundGroupReq {
    founder: String,
    slug: String,
    name: String,
    kind: String,
}

#[derive(Serialize)]
struct FoundGroupRes {
    group_id: String,
    slug: String,
}

async fn found_group(
    State(st): State<AppState>,
    Json(req): Json<FoundGroupReq>,
) -> Result<Json<FoundGroupRes>, ApiError> {
    let cmd = FoundGroupCommand {
        founder: UserId::parse(&req.founder).map_err(invalid)?,
        slug: GroupSlug::parse(&req.slug).map_err(invalid)?,
        name: GroupName::parse(&req.name).map_err(invalid)?,
        kind: GroupKind::parse(&req.kind).map_err(invalid)?,
    };
    let uc = FoundGroup::new(PgGroupRepository::new(st.db.clone()), SystemClock);
    let group = uc.execute(cmd).await?;
    Ok(Json(FoundGroupRes {
        group_id: group.id().to_string(),
        slug: group.slug().as_str().to_owned(),
    }))
}

#[derive(Serialize)]
struct GroupRes {
    group_id: String,
    slug: String,
    name: String,
    kind: String,
    member_count: u32,
}

impl From<GroupView> for GroupRes {
    fn from(v: GroupView) -> Self {
        Self {
            group_id: v.group_id.to_string(),
            slug: v.slug,
            name: v.name,
            kind: v.kind,
            member_count: v.member_count,
        }
    }
}

async fn group_view(
    State(st): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<GroupRes>, ApiError> {
    let uc = GroupQuery::new(PgGroupReadModel::new(st.db.clone()));
    let view = uc.execute(GroupBySlug { slug }).await?;
    Ok(Json(view.into()))
}

#[derive(Deserialize)]
struct MemberReq {
    user: String,
}

#[derive(Serialize)]
struct MembershipRes {
    group_id: String,
    user: String,
    role: String,
}

async fn join_group(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<MemberReq>,
) -> Result<Json<MembershipRes>, ApiError> {
    let group = GroupId::parse(&id).map_err(invalid)?;
    let user = UserId::parse(&req.user).map_err(invalid)?;
    let uc = JoinGroup::new(PgGroupMembershipTxFactory::new(st.db.clone()), SystemClock);
    let event = uc.execute(JoinGroupCommand { group, user }).await?;
    Ok(Json(MembershipRes {
        group_id: event.group.to_string(),
        user: event.user.to_string(),
        role: event.role.as_str().to_owned(),
    }))
}

async fn leave_group(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<MemberReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let group = GroupId::parse(&id).map_err(invalid)?;
    let user = UserId::parse(&req.user).map_err(invalid)?;
    let uc = LeaveGroup::new(PgGroupMembershipTxFactory::new(st.db.clone()), SystemClock);
    let event = uc.execute(LeaveGroupCommand { group, user }).await?;
    Ok(Json(json!({
        "group_id": event.group.to_string(),
        "user": event.user.to_string(),
        "left": true,
    })))
}

#[derive(Deserialize)]
struct SetRoleReq {
    actor: String,
    target: String,
    role: String,
}

async fn set_role(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<SetRoleReq>,
) -> Result<Json<MembershipRes>, ApiError> {
    let cmd = SetMemberRoleCommand {
        group: GroupId::parse(&id).map_err(invalid)?,
        actor: UserId::parse(&req.actor).map_err(invalid)?,
        target: UserId::parse(&req.target).map_err(invalid)?,
        role: MembershipRole::parse(&req.role).map_err(invalid)?,
    };
    let uc = SetMemberRole::new(PgGroupMembershipTxFactory::new(st.db.clone()), SystemClock);
    let event = uc.execute(cmd).await?;
    Ok(Json(MembershipRes {
        group_id: event.group.to_string(),
        user: event.user.to_string(),
        role: event.new_role.as_str().to_owned(),
    }))
}

#[derive(Deserialize)]
struct GroupPostReq {
    author: String,
    body: String,
}

async fn post_to_group(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<GroupPostReq>,
) -> Result<Json<PostRes>, ApiError> {
    let cmd = PostToGroupCommand {
        author: UserId::parse(&req.author).map_err(invalid)?,
        group: GroupId::parse(&id).map_err(invalid)?,
        body: PostBody::parse(&req.body).map_err(invalid)?,
    };
    let uc = PostToGroup::new(
        PgGroupRepository::new(st.db.clone()),
        PgGroupPostRepository::new(st.db.clone()),
        SystemClock,
    );
    let post = uc.execute(cmd).await?;
    Ok(Json(PostRes {
        post_id: post.id().to_string(),
        created_at: post.created_at().into_offset().unix_timestamp(),
    }))
}
