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
    CreatePost, CreatePostCommand, IssueInvite, IssueInviteCommand, Register, RegisterCommand,
};
use babangida_application::query::{
    FeedQuery, ProfileByHandle, ProfileQuery, ProfileView, RecentFeed,
};
use babangida_domain::RepositoryError;
use babangida_domain::content::PostBody;
use babangida_domain::identity::{Handle, InviteCode, UserId};
use babangida_domain::social::{DisplayName, Subculture};
use babangida_infrastructure::{
    Db, PgFeedReadModel, PgIssueInviteTxFactory, PgPostRepository, PgProfileReadModel,
    PgRegistrationTxFactory, RandomInviteCodeFactory, SystemClock,
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
        })
        .collect();
    Ok(Json(out))
}
