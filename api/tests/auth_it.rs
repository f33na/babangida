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
use babangida_domain::identity::{Handle, User, UserRepository, UserRole};
use babangida_infrastructure::{Db, PgUserRepository, connect, run_migrations};
use babangida_shared::{Id, Timestamp};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn setup() -> Option<(Db, String)> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let db = connect(&url).await.ok()?;
    run_migrations(&db).await.expect("миграции");
    sqlx::query(
        "TRUNCATE users, invites, profiles, credentials, sessions RESTART IDENTITY CASCADE",
    )
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
    Some((db, admin.id().to_string()))
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auth_end_to_end() {
    let Some((db, admin_id)) = setup().await else {
        eprintln!("SKIP auth_end_to_end: DATABASE_URL не задан");
        return;
    };
    let app = router(AppState { db });

    // admin выдаёт инвайт (выдача в 2a ещё по параметру)
    let (status, body) = request(
        &app,
        "POST",
        "/invites",
        Some(json!({ "inviter": admin_id })),
        None,
    )
    .await;
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
    let (status, body) = request(
        &app,
        "POST",
        "/login",
        Some(json!({ "handle": "newcomer", "password": "newcomerpass" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "логин: {body}");
    let token = body["token"].as_str().unwrap().to_owned();
    assert!(body["expires_at"].as_i64().unwrap() > 0);

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
