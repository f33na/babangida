//! Сквозной HTTP-тест аутентификации против реальной Postgres (ADR-0013/0009).
//! Гоняет роутер через `oneshot`. Без DATABASE_URL — скип.
//!
//! Проверяет всю цепочку: регистрация с паролем (атомарно создаёт креды) → логин
//! (выпуск сессии) → защищённый `GET /me` по токену → logout гасит сессию. Плюс
//! отказы: нет токена, неверный пароль, неизвестный handle — все 401.

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use babangida_api::{AppState, router};
use babangida_application::command::{EstablishCredential, EstablishCredentialCommand};
use babangida_domain::auth::Password;
use babangida_domain::identity::{Handle, User, UserRepository, UserRole};
use babangida_infrastructure::{
    Argon2PasswordHasher, PgCredentialRepository, PgUserRepository, SystemClock, connect,
    run_migrations,
};
use babangida_shared::{Id, Timestamp};
use serde_json::{Value, json};
use tower::ServiceExt;

/// Поднять схему, засеять админа с паролем и залогинить его. Возвращает роутер и
/// токен админа (выдача инвайта теперь под аутентификацией).
async fn setup() -> Option<(Router, String)> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let db = connect(&url).await.ok()?;
    run_migrations(&db).await.expect("миграции");
    sqlx::query("TRUNCATE users, groups RESTART IDENTITY CASCADE")
        .execute(&db)
        .await
        .expect("truncate");
    let admin = User::register(
        Id::generate(),
        Handle::parse("rootadmin").unwrap(),
        UserRole::Admin,
        Timestamp::now(),
    );
    PgUserRepository::new(db.clone())
        .save(&admin)
        .await
        .expect("seed admin");
    EstablishCredential::new(
        PgCredentialRepository::new(db.clone()),
        Argon2PasswordHasher,
        SystemClock,
    )
    .execute(EstablishCredentialCommand {
        user: admin.id(),
        password: Password::parse("rootpassword").unwrap(),
    })
    .await
    .expect("seed admin cred");
    let app = router(AppState { db });
    let token = login(&app, "rootadmin", "rootpassword").await;
    Some((app, token))
}

/// Запрос с опциональным телом и Bearer-токеном.
async fn request(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    let req = match body {
        Some(b) => builder
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&b).unwrap()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

async fn login(app: &Router, handle: &str, password: &str) -> String {
    let (status, body) = request(
        app,
        "POST",
        "/login",
        Some(json!({ "handle": handle, "password": password })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "логин {handle}: {body}");
    body["token"].as_str().unwrap().to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auth_end_to_end() {
    let Some((app, admin_token)) = setup().await else {
        eprintln!("SKIP auth_end_to_end: DATABASE_URL не задан");
        return;
    };

    // запись без токена запрещена: выдача инвайта → 401
    let (status, _) = request(&app, "POST", "/invites", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "без сессии писать нельзя");

    // admin (по токену) выдаёт инвайт
    let (status, body) = request(&app, "POST", "/invites", None, Some(&admin_token)).await;
    assert_eq!(status, StatusCode::OK, "выдача инвайта: {body}");
    let code = body["code"].as_str().unwrap().to_owned();

    // регистрация с паролем → создаёт юзера, профиль и креды атомарно
    let (status, body) = request(
        &app,
        "POST",
        "/register",
        Some(json!({
            "code": code,
            "handle": "newcomer",
            "display_name": "New Comer",
            "subculture": "underground",
            "password": "newcomerpass"
        })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "регистрация: {body}");

    // логин по верному паролю → токен сессии
    let token = login(&app, "newcomer", "newcomerpass").await;

    // GET /me по токену → распознан текущий юзер
    let (status, body) = request(&app, "GET", "/me", None, Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "/me: {body}");
    assert_eq!(body["handle"], "newcomer");
    assert_eq!(body["verified"], false);

    // GET /me без токена → 401
    let (status, _) = request(&app, "GET", "/me", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "нет токена → 401");

    // неверный пароль → 401 (как и неизвестный handle — анти-энумерация)
    let (status, _) = request(
        &app,
        "POST",
        "/login",
        Some(json!({ "handle": "newcomer", "password": "wrongpass123" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "неверный пароль → 401");

    let (status, _) = request(
        &app,
        "POST",
        "/login",
        Some(json!({ "handle": "ghost", "password": "whatever12345" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "неизвестный handle → 401");

    // logout гасит сессию
    let (status, _) = request(&app, "POST", "/logout", None, Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "logout");

    // тот же токен после logout → 401
    let (status, _) = request(&app, "GET", "/me", None, Some(&token)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "погашенная сессия → 401");
}
