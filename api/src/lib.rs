//! HTTP-слой babangida (Axum) и точка композиции зависимостей. Хендлеры парсят
//! ввод в доменные value objects, зовут use-cases `application` и переводят
//! результат/ошибку в HTTP. Доменные правила (квота/кулдаун) НЕ дублируются —
//! их проверяет домен (ADR-0003/0005). См. `../../babangida-vault/COMMON.md`.

use axum::Json;
use axum::Router;
use axum::extract::{FromRequestParts, OptionalFromRequestParts, Path, Query, State};
use axum::http::header::{AUTHORIZATION, COOKIE, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use babangida_application::ApplicationError;
use babangida_application::command::{
    ApproveVerification, ApproveVerificationCommand, Authenticate, AuthenticateCommand,
    CreateListing, CreateListingCommand, CreatePost, CreatePostCommand, EstablishCredential,
    EstablishCredentialCommand, FoundGroup, FoundGroupCommand, IssueInvite, IssueInviteCommand,
    JoinGroup, JoinGroupCommand, LeaveGroup, LeaveGroupCommand, LogIn, LogInCommand, LogOut,
    LogOutCommand, MarkListingSold, MarkListingSoldCommand, PostToGroup, PostToGroupCommand,
    Register, RegisterCommand, RejectVerification, RejectVerificationCommand, RequestVerification,
    RequestVerificationCommand, SendMessage, SendMessageCommand, SetMemberRole,
    SetMemberRoleCommand, VerifyUser, VerifyUserCommand, WithdrawListing, WithdrawListingCommand,
};
use babangida_application::query::{
    FeedQuery, GroupBySlug, GroupQuery, GroupView, InboxOf, InboxQuery, ListingView, MarketBrowse,
    MarketQuery, MyVerificationOf, MyVerificationQuery, PendingVerifications,
    PendingVerificationsQuery, ProfileByHandle, ProfileQuery, ProfileView, RecentFeed,
    SellerListings, SellerListingsQuery, ThreadOf, ThreadQuery,
};
use babangida_domain::RepositoryError;
use babangida_domain::auth::{AuthError, Password, SESSION_TTL, SessionToken};
use babangida_domain::community::{
    CommunityError, GroupId, GroupKind, GroupName, GroupSlug, MembershipRole,
};
use babangida_domain::content::PostBody;
use babangida_domain::identity::{
    Handle, InviteCode, UserId, UserRepository, UserRole, VerifiedStatus,
};
use babangida_domain::marketplace::{
    ListingDescription, ListingDraft, ListingId, ListingTitle, MarketplaceError, Price,
};
use babangida_domain::messaging::{ConversationId, MessageBody, MessagingError};
use babangida_domain::social::{DisplayName, Subculture};
use babangida_domain::verification::{DecisionReason, RequestNote, VerificationRequestId};
use babangida_infrastructure::{
    Argon2PasswordHasher, Db, PgConversationRepository, PgCredentialRepository, PgFeedReadModel,
    PgGroupMembershipTxFactory, PgGroupPostRepository, PgGroupReadModel, PgGroupRepository,
    PgInboxReadModel, PgIssueInviteTxFactory, PgListingReadModel, PgListingRepository,
    PgMessageRepository, PgPostRepository, PgProfileReadModel, PgRegistrationTxFactory,
    PgSessionRepository, PgThreadReadModel, PgUserRepository, PgVerificationDecisionTxFactory,
    PgVerificationReadModel, PgVerificationRequestRepository, RandomInviteCodeFactory,
    RandomSessionTokenFactory, SystemClock,
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
        // auth
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me))
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
        // marketplace (продажа — за гейтом верификации; чтение — публичное)
        .route("/listings", post(create_listing))
        .route("/listings/{id}/sold", post(mark_listing_sold))
        .route("/listings/{id}/withdraw", post(withdraw_listing))
        .route("/market", get(market))
        .route("/profiles/{handle}/listings", get(seller_listings))
        // верификация (ADR-0010/0016): заявка от юзера, очередь и решение — у админа;
        // прямой грант остаётся как админ-override.
        .route("/users/{handle}/verify", post(verify_user))
        .route(
            "/verification/requests",
            post(request_verification).get(verification_queue),
        )
        .route("/verification/me", get(my_verification))
        .route(
            "/verification/requests/{id}/approve",
            post(approve_verification),
        )
        .route(
            "/verification/requests/{id}/reject",
            post(reject_verification),
        )
        .with_state(state)
}

/// Bootstrap-пароля сид-админа (ADR-0013): сид-миграция заводит `root` без кредов,
/// и войти он не может. Эта функция на старте, если задан пароль, ставит/обновляет
/// его учётные данные. Идемпотентна. Вызывается из бинаря по env `ADMIN_BOOTSTRAP_*`.
///
/// Возвращает `Ok(true)`, если креды установлены; `Ok(false)`, если админа с таким
/// handle нет (пропуск).
///
/// # Errors
/// Строка с причиной: невалидный handle/пароль или сбой хранилища.
pub async fn bootstrap_admin(db: &Db, handle: &str, raw_password: &str) -> Result<bool, String> {
    let handle = Handle::parse(handle).map_err(|e| format!("ADMIN_BOOTSTRAP_HANDLE: {e}"))?;
    let password =
        Password::parse(raw_password).map_err(|e| format!("ADMIN_BOOTSTRAP_PASSWORD: {e}"))?;
    let Some(admin) = PgUserRepository::new(db.clone())
        .find_by_handle(&handle)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(false);
    };
    EstablishCredential::new(
        PgCredentialRepository::new(db.clone()),
        Argon2PasswordHasher,
        SystemClock,
    )
    .execute(EstablishCredentialCommand {
        user: admin.id(),
        password,
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(true)
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
        // Аутентификация: неверные данные или нет валидной сессии — 401.
        ApplicationError::Auth(e) => (StatusCode::UNAUTHORIZED, e.to_string()),
        // Барахолка: гейт верификации и право продавца — запрещено; неактивный товар — конфликт.
        ApplicationError::Marketplace(
            e @ (MarketplaceError::NotVerified | MarketplaceError::NotSeller),
        ) => (StatusCode::FORBIDDEN, e.to_string()),
        ApplicationError::Marketplace(e @ MarketplaceError::NotActive) => {
            (StatusCode::CONFLICT, e.to_string())
        }
        // Верификация: повторное решение по заявке — конфликт состояния.
        ApplicationError::Verification(e) => (StatusCode::CONFLICT, e.to_string()),
        ApplicationError::NotFound(what) => (StatusCode::NOT_FOUND, format!("не найдено: {what}")),
        ApplicationError::Forbidden(what) => (StatusCode::FORBIDDEN, format!("запрещено: {what}")),
        // Конфликт состояния (уже верифицирован / заявка уже подана).
        ApplicationError::Conflict(what) => (StatusCode::CONFLICT, format!("конфликт: {what}")),
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

// --- аутентификация: текущий юзер из сессии (ADR-0013) ---

/// Текущий аутентифицированный юзер, извлечённый из токена сессии
/// (`Authorization: Bearer ...` либо кука `session`). Отсутствие, невалидность или
/// истечение токена → 401.
struct CurrentUser {
    id: UserId,
    handle: Handle,
    verified: VerifiedStatus,
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, st: &AppState) -> Result<Self, Self::Rejection> {
        let unauth = || ApiError::App(ApplicationError::Auth(AuthError::Unauthenticated));
        let raw = token_from_headers(&parts.headers).ok_or_else(unauth)?;
        let token = SessionToken::parse(&raw).map_err(|_| unauth())?;
        let uc = Authenticate::new(
            PgSessionRepository::new(st.db.clone()),
            PgUserRepository::new(st.db.clone()),
            SystemClock,
        );
        let who = uc.execute(AuthenticateCommand { token }).await?;
        Ok(Self {
            id: who.user,
            handle: who.handle,
            verified: who.verified,
        })
    }
}

/// Опциональный текущий юзер для публичных чтений с viewer-aware выдачей (лента).
/// Нет токена / истёкшая / неизвестная сессия → `None` (аноним); сбой хранилища —
/// наружу как ошибка, чтобы не подменять выдачу залогиненного на анонимную молча.
impl OptionalFromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        st: &AppState,
    ) -> Result<Option<Self>, Self::Rejection> {
        let Some(raw) = token_from_headers(&parts.headers) else {
            return Ok(None);
        };
        let Ok(token) = SessionToken::parse(&raw) else {
            return Ok(None);
        };
        let uc = Authenticate::new(
            PgSessionRepository::new(st.db.clone()),
            PgUserRepository::new(st.db.clone()),
            SystemClock,
        );
        match uc.execute(AuthenticateCommand { token }).await {
            Ok(who) => Ok(Some(Self {
                id: who.user,
                handle: who.handle,
                verified: who.verified,
            })),
            Err(ApplicationError::Auth(_)) => Ok(None),
            Err(e) => Err(ApiError::App(e)),
        }
    }
}

/// Достать токен сессии из заголовков: сперва `Authorization: Bearer`, затем кука
/// `session`.
fn token_from_headers(headers: &HeaderMap) -> Option<String> {
    if let Some(auth) = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok())
        && let Some(token) = auth.strip_prefix("Bearer ")
    {
        return Some(token.trim().to_owned());
    }
    let cookie = headers.get(COOKIE).and_then(|v| v.to_str().ok())?;
    cookie
        .split(';')
        .filter_map(|kv| kv.trim().strip_prefix("session="))
        .map(str::to_owned)
        .next()
}

// --- эндпоинты ---
// Команды атрибутируются текущему юзеру из сессии (`CurrentUser`), не из тела
// запроса (ADR-0013): писать от чужого имени нельзя.

#[derive(Serialize)]
struct IssueRes {
    invite_id: String,
    code: String,
    issued_at: i64,
}

async fn issue_invite(
    State(st): State<AppState>,
    current: CurrentUser,
) -> Result<Json<IssueRes>, ApiError> {
    let uc = IssueInvite::new(
        PgIssueInviteTxFactory::new(st.db.clone()),
        SystemClock,
        RandomInviteCodeFactory,
    );
    let event = uc
        .execute(IssueInviteCommand {
            inviter: current.id,
        })
        .await?;
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
    password: String,
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
        password: Password::parse(&req.password).map_err(invalid)?,
    };
    let uc = Register::new(
        PgRegistrationTxFactory::new(st.db.clone()),
        Argon2PasswordHasher,
        SystemClock,
    );
    let user = uc.execute(cmd).await?;
    Ok(Json(RegisterRes {
        user_id: user.id().to_string(),
        handle: user.handle().as_str().to_owned(),
    }))
}

#[derive(Deserialize)]
struct LoginReq {
    handle: String,
    password: String,
}

#[derive(Serialize)]
struct LoginRes {
    token: String,
    expires_at: i64,
}

async fn login(
    State(st): State<AppState>,
    Json(req): Json<LoginReq>,
) -> Result<Response, ApiError> {
    // Невалидный handle/пароль на входе — те же «неверные данные» (анти-энумерация).
    let bad = || ApiError::App(ApplicationError::Auth(AuthError::InvalidCredentials));
    let cmd = LogInCommand {
        handle: Handle::parse(&req.handle).map_err(|_| bad())?,
        password: Password::parse(&req.password).map_err(|_| bad())?,
    };
    let uc = LogIn::new(
        PgUserRepository::new(st.db.clone()),
        PgCredentialRepository::new(st.db.clone()),
        PgSessionRepository::new(st.db.clone()),
        Argon2PasswordHasher,
        RandomSessionTokenFactory,
        SystemClock,
    );
    let auth = uc.execute(cmd).await?;
    let token = auth.token.as_str().to_owned();
    let expires_at = auth.expires_at.into_offset().unix_timestamp();
    let mut resp = Json(LoginRes {
        token: token.clone(),
        expires_at,
    })
    .into_response();
    // Кука для web (HttpOnly — недоступна JS); api/mobile берут token из тела.
    let cookie = format!(
        "session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
        SESSION_TTL.whole_seconds()
    );
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        resp.headers_mut().insert(SET_COOKIE, value);
    }
    Ok(resp)
}

async fn logout(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    // Идемпотентно: нет/невалиден токен — просто чистим куку.
    if let Some(raw) = token_from_headers(&headers)
        && let Ok(token) = SessionToken::parse(&raw)
    {
        LogOut::new(PgSessionRepository::new(st.db.clone()))
            .execute(LogOutCommand { token })
            .await?;
    }
    let mut resp = Json(json!({ "ok": true })).into_response();
    resp.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_static("session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"),
    );
    Ok(resp)
}

#[derive(Serialize)]
struct MeRes {
    user_id: String,
    handle: String,
    verified: bool,
}

async fn me(current: CurrentUser) -> Json<MeRes> {
    Json(MeRes {
        user_id: current.id.to_string(),
        handle: current.handle.as_str().to_owned(),
        verified: current.verified.is_verified(),
    })
}

#[derive(Deserialize)]
struct PostReq {
    body: String,
}

#[derive(Serialize)]
struct PostRes {
    post_id: String,
    created_at: i64,
}

async fn create_post(
    State(st): State<AppState>,
    current: CurrentUser,
    Json(req): Json<PostReq>,
) -> Result<Json<PostRes>, ApiError> {
    let cmd = CreatePostCommand {
        author: current.id,
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
    // Лента публична, но при наличии валидной сессии — viewer-aware (посты закрытых
    // групп зрителя видны). Аноним получает только публичное (ADR-0012).
    viewer: Option<CurrentUser>,
    Query(params): Query<FeedParams>,
) -> Result<Json<Vec<FeedItemRes>>, ApiError> {
    let uc = FeedQuery::new(PgFeedReadModel::new(st.db.clone()));
    let items = uc
        .execute(RecentFeed {
            viewer: viewer.map(|v| v.id),
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
    current: CurrentUser,
    Json(req): Json<SendMessageReq>,
) -> Result<Json<SendMessageRes>, ApiError> {
    let cmd = SendMessageCommand {
        author: current.id,
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
    current: CurrentUser,
    Query(params): Query<InboxParams>,
) -> Result<Json<Vec<ConversationRes>>, ApiError> {
    let uc = InboxQuery::new(PgInboxReadModel::new(st.db.clone()));
    let items = uc
        .execute(InboxOf {
            user: current.id,
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
    current: CurrentUser,
    Path(id): Path<String>,
    Query(params): Query<ThreadParams>,
) -> Result<Json<Vec<MessageRes>>, ApiError> {
    let conversation = ConversationId::parse(&id).map_err(invalid)?;
    let uc = ThreadQuery::new(PgThreadReadModel::new(st.db.clone()));
    let items = uc
        .execute(ThreadOf {
            conversation,
            viewer: current.id,
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
    current: CurrentUser,
    Json(req): Json<FoundGroupReq>,
) -> Result<Json<FoundGroupRes>, ApiError> {
    let cmd = FoundGroupCommand {
        founder: current.id,
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

#[derive(Serialize)]
struct MembershipRes {
    group_id: String,
    user: String,
    role: String,
}

async fn join_group(
    State(st): State<AppState>,
    current: CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<MembershipRes>, ApiError> {
    let group = GroupId::parse(&id).map_err(invalid)?;
    let uc = JoinGroup::new(PgGroupMembershipTxFactory::new(st.db.clone()), SystemClock);
    let event = uc
        .execute(JoinGroupCommand {
            group,
            user: current.id,
        })
        .await?;
    Ok(Json(MembershipRes {
        group_id: event.group.to_string(),
        user: event.user.to_string(),
        role: event.role.as_str().to_owned(),
    }))
}

async fn leave_group(
    State(st): State<AppState>,
    current: CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let group = GroupId::parse(&id).map_err(invalid)?;
    let uc = LeaveGroup::new(PgGroupMembershipTxFactory::new(st.db.clone()), SystemClock);
    let event = uc
        .execute(LeaveGroupCommand {
            group,
            user: current.id,
        })
        .await?;
    Ok(Json(json!({
        "group_id": event.group.to_string(),
        "user": event.user.to_string(),
        "left": true,
    })))
}

#[derive(Deserialize)]
struct SetRoleReq {
    target: String,
    role: String,
}

async fn set_role(
    State(st): State<AppState>,
    current: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<SetRoleReq>,
) -> Result<Json<MembershipRes>, ApiError> {
    let cmd = SetMemberRoleCommand {
        group: GroupId::parse(&id).map_err(invalid)?,
        actor: current.id,
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
    body: String,
}

async fn post_to_group(
    State(st): State<AppState>,
    current: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<GroupPostReq>,
) -> Result<Json<PostRes>, ApiError> {
    let cmd = PostToGroupCommand {
        author: current.id,
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

// --- marketplace ---

#[derive(Deserialize)]
struct ListingReq {
    title: String,
    price: u64,
    description: Option<String>,
}

#[derive(Serialize)]
struct CreatedListingRes {
    listing_id: String,
    status: String,
    created_at: i64,
}

async fn create_listing(
    State(st): State<AppState>,
    current: CurrentUser,
    Json(req): Json<ListingReq>,
) -> Result<Json<CreatedListingRes>, ApiError> {
    let description = req
        .description
        .as_deref()
        .map(ListingDescription::parse)
        .transpose()
        .map_err(invalid)?;
    let draft = ListingDraft {
        title: ListingTitle::parse(&req.title).map_err(invalid)?,
        price: Price::parse(req.price).map_err(invalid)?,
        description,
    };
    let uc = CreateListing::new(
        PgUserRepository::new(st.db.clone()),
        PgListingRepository::new(st.db.clone()),
        SystemClock,
    );
    let listing = uc
        .execute(CreateListingCommand {
            seller: current.id,
            draft,
        })
        .await?;
    Ok(Json(CreatedListingRes {
        listing_id: listing.id().to_string(),
        status: listing.status().as_str().to_owned(),
        created_at: listing.created_at().into_offset().unix_timestamp(),
    }))
}

async fn mark_listing_sold(
    State(st): State<AppState>,
    current: CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let listing = ListingId::parse(&id).map_err(invalid)?;
    MarkListingSold::new(PgListingRepository::new(st.db.clone()))
        .execute(MarkListingSoldCommand {
            listing,
            actor: current.id,
        })
        .await?;
    Ok(Json(json!({ "listing_id": id, "status": "sold" })))
}

async fn withdraw_listing(
    State(st): State<AppState>,
    current: CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let listing = ListingId::parse(&id).map_err(invalid)?;
    WithdrawListing::new(PgListingRepository::new(st.db.clone()))
        .execute(WithdrawListingCommand {
            listing,
            actor: current.id,
        })
        .await?;
    Ok(Json(json!({ "listing_id": id, "status": "withdrawn" })))
}

#[derive(Serialize)]
struct ListingRes {
    listing_id: String,
    seller: String,
    seller_handle: String,
    title: String,
    price: u64,
    description: Option<String>,
    status: String,
    created_at: i64,
}

impl From<ListingView> for ListingRes {
    fn from(v: ListingView) -> Self {
        Self {
            listing_id: v.listing_id.to_string(),
            seller: v.seller.to_string(),
            seller_handle: v.seller_handle,
            title: v.title,
            price: v.price_rubles,
            description: v.description,
            status: v.status,
            created_at: v.created_at.into_offset().unix_timestamp(),
        }
    }
}

async fn market(
    State(st): State<AppState>,
    Query(params): Query<FeedParams>,
) -> Result<Json<Vec<ListingRes>>, ApiError> {
    let uc = MarketQuery::new(PgListingReadModel::new(st.db.clone()));
    let items = uc
        .execute(MarketBrowse {
            limit: params.limit.unwrap_or(50),
        })
        .await?;
    Ok(Json(items.into_iter().map(ListingRes::from).collect()))
}

async fn seller_listings(
    State(st): State<AppState>,
    Path(handle): Path<String>,
    Query(params): Query<FeedParams>,
) -> Result<Json<Vec<ListingRes>>, ApiError> {
    let uc = SellerListingsQuery::new(PgListingReadModel::new(st.db.clone()));
    let items = uc
        .execute(SellerListings {
            handle,
            limit: params.limit.unwrap_or(50),
        })
        .await?;
    Ok(Json(items.into_iter().map(ListingRes::from).collect()))
}

async fn verify_user(
    State(st): State<AppState>,
    current: CurrentUser,
    Path(handle): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let target = Handle::parse(&handle).map_err(invalid)?;
    let user = VerifyUser::new(PgUserRepository::new(st.db.clone()))
        .execute(VerifyUserCommand {
            actor: current.id,
            target,
        })
        .await?;
    Ok(Json(json!({
        "handle": user.handle().as_str(),
        "verified": user.verified().is_verified(),
    })))
}

// --- верификация: заявка → рассмотрение (ADR-0016) ---

/// Гейт «только админ» для админ-чтений (очередь верификации). Пишущие решения
/// (approve/reject) проверяют роль внутри use-case; чтение очереди — здесь.
async fn require_admin(db: &Db, user: UserId) -> Result<(), ApiError> {
    let actor = PgUserRepository::new(db.clone())
        .find_by_id(user)
        .await
        .map_err(|e| ApiError::App(e.into()))?
        .ok_or(ApiError::App(ApplicationError::NotFound("actor")))?;
    if actor.role() != UserRole::Admin {
        return Err(ApiError::App(ApplicationError::Forbidden("только админ")));
    }
    Ok(())
}

/// Распарсить опциональную записку/причину: пустую/пробельную трактуем как «нет».
fn parse_note(raw: Option<String>) -> Result<Option<RequestNote>, ApiError> {
    raw.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(RequestNote::parse)
        .transpose()
        .map_err(invalid)
}

fn parse_reason(raw: Option<String>) -> Result<Option<DecisionReason>, ApiError> {
    raw.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(DecisionReason::parse)
        .transpose()
        .map_err(invalid)
}

#[derive(Deserialize)]
struct RequestVerificationReq {
    #[serde(default)]
    note: Option<String>,
}

#[derive(Serialize)]
struct VerificationRequestRes {
    request_id: String,
    status: String,
}

async fn request_verification(
    State(st): State<AppState>,
    current: CurrentUser,
    Json(req): Json<RequestVerificationReq>,
) -> Result<Json<VerificationRequestRes>, ApiError> {
    let note = parse_note(req.note)?;
    let uc = RequestVerification::new(
        PgUserRepository::new(st.db.clone()),
        PgVerificationRequestRepository::new(st.db.clone()),
        SystemClock,
    );
    let request = uc
        .execute(RequestVerificationCommand {
            requester: current.id,
            note,
        })
        .await?;
    Ok(Json(VerificationRequestRes {
        request_id: request.id().to_string(),
        status: request.status().as_str().to_owned(),
    }))
}

#[derive(Deserialize)]
struct QueueParams {
    limit: Option<u32>,
}

#[derive(Serialize)]
struct QueueItemRes {
    request_id: String,
    requester_handle: String,
    note: Option<String>,
    created_at: i64,
}

async fn verification_queue(
    State(st): State<AppState>,
    current: CurrentUser,
    Query(params): Query<QueueParams>,
) -> Result<Json<Vec<QueueItemRes>>, ApiError> {
    require_admin(&st.db, current.id).await?;
    let uc = PendingVerificationsQuery::new(PgVerificationReadModel::new(st.db.clone()));
    let items = uc
        .execute(PendingVerifications {
            limit: params.limit.unwrap_or(100),
        })
        .await?;
    Ok(Json(
        items
            .into_iter()
            .map(|v| QueueItemRes {
                request_id: v.request_id.to_string(),
                requester_handle: v.requester_handle,
                note: v.note,
                created_at: v.created_at.into_offset().unix_timestamp(),
            })
            .collect(),
    ))
}

#[derive(Serialize)]
struct MyVerificationRes {
    request_id: String,
    status: String,
    decision_reason: Option<String>,
    created_at: i64,
    decided_at: Option<i64>,
}

async fn my_verification(
    State(st): State<AppState>,
    current: CurrentUser,
) -> Result<Json<Option<MyVerificationRes>>, ApiError> {
    let uc = MyVerificationQuery::new(PgVerificationReadModel::new(st.db.clone()));
    let view = uc
        .execute(MyVerificationOf {
            requester: current.id,
        })
        .await?;
    Ok(Json(view.map(|v| MyVerificationRes {
        request_id: v.request_id.to_string(),
        status: v.status,
        decision_reason: v.decision_reason,
        created_at: v.created_at.into_offset().unix_timestamp(),
        decided_at: v.decided_at.map(|t| t.into_offset().unix_timestamp()),
    })))
}

#[derive(Deserialize)]
struct DecisionReq {
    #[serde(default)]
    reason: Option<String>,
}

async fn approve_verification(
    State(st): State<AppState>,
    current: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<DecisionReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let request = VerificationRequestId::parse(&id).map_err(invalid)?;
    let reason = parse_reason(req.reason)?;
    let uc = ApproveVerification::new(
        PgVerificationDecisionTxFactory::new(st.db.clone()),
        SystemClock,
    );
    let event = uc
        .execute(ApproveVerificationCommand {
            actor: current.id,
            request,
            reason,
        })
        .await?;
    Ok(Json(json!({
        "request_id": event.request.to_string(),
        "status": "approved",
    })))
}

async fn reject_verification(
    State(st): State<AppState>,
    current: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<DecisionReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let request = VerificationRequestId::parse(&id).map_err(invalid)?;
    let reason = parse_reason(req.reason)?;
    let uc = RejectVerification::new(
        PgVerificationDecisionTxFactory::new(st.db.clone()),
        SystemClock,
    );
    let event = uc
        .execute(RejectVerificationCommand {
            actor: current.id,
            request,
            reason,
        })
        .await?;
    Ok(Json(json!({
        "request_id": event.request.to_string(),
        "status": "rejected",
    })))
}
