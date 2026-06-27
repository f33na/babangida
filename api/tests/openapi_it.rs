//! Сквозной HTTP-тест открытого API против реальной Postgres (ADR-0018/0010/0009).
//! Гоняет роутер через `oneshot`. Без DATABASE_URL — скип.
//!
//! Проверяет: выпуск ключа только верифицированным (casual → 403); ключ
//! аутентифицирует `/api/v1` (нет/неизвестный/отозванный → 401); чтение и запись от
//! владельца (пост и трек через API); отозвать ключ может только владелец (403 чужому).

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

async fn setup() -> Option<(Router, String)> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let db = connect(&url).await.ok()?;
    run_migrations(&db).await.expect("миграции");
    sqlx::query("TRUNCATE users, api_keys, tracks RESTART IDENTITY CASCADE")
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

/// Зарегистрировать юзера по инвайту от админа и залогинить. Возвращает токен сессии.
async fn register_user(app: &Router, admin_token: &str, handle: &str) -> String {
    let (status, body) = request(app, "POST", "/invites", None, Some(admin_token)).await;
    assert_eq!(status, StatusCode::OK, "выдача инвайта: {body}");
    let code = body["code"].as_str().unwrap().to_owned();
    let (status, body) = request(
        app,
        "POST",
        "/register",
        Some(json!({ "code": code, "handle": handle, "display_name": handle, "subculture": "underground", "password": "memberpass1" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "регистрация {handle}: {body}");
    login(app, handle, "memberpass1").await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn openapi_end_to_end() {
    let Some((app, admin)) = setup().await else {
        eprintln!("SKIP openapi_end_to_end: DATABASE_URL не задан");
        return;
    };

    // верифицированный разработчик + casual
    let dev = register_user(&app, &admin, "devx").await;
    let (status, _) = request(&app, "POST", "/users/devx/verify", None, Some(&admin)).await;
    assert_eq!(status, StatusCode::OK, "верификация devx");
    let casual = register_user(&app, &admin, "casual").await;

    // casual не может выпустить ключ → 403
    let (status, _) = request(
        &app,
        "POST",
        "/api-keys",
        Some(json!({ "label": "bot" })),
        Some(&casual),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "casual не выпускает ключ");

    // verified выпускает ключ → 200, сырой токен отдаётся один раз
    let (status, body) = request(
        &app,
        "POST",
        "/api-keys",
        Some(json!({ "label": "ci" })),
        Some(&dev),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "выпуск ключа: {body}");
    let key = body["token"].as_str().unwrap().to_owned();
    let key_id = body["key_id"].as_str().unwrap().to_owned();
    assert_eq!(body["status"], "active");

    // ключ виден в списке владельца (без секрета)
    let (status, body) = request(&app, "GET", "/api-keys", None, Some(&dev)).await;
    assert_eq!(status, StatusCode::OK);
    let item = body
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["key_id"] == key_id)
        .expect("ключ в списке");
    assert_eq!(item["label"], "ci");
    assert_eq!(item["status"], "active");
    assert!(item.get("token").is_none(), "секрет не отдаётся в списке");

    // ключ аутентифицирует /api/v1/me
    let (status, body) = request(&app, "GET", "/api/v1/me", None, Some(&key)).await;
    assert_eq!(status, StatusCode::OK, "ключ аутентифицирует: {body}");
    assert_eq!(body["handle"], "devx");
    assert_eq!(body["verified"], true);

    // без ключа → 401; неизвестный ключ → 401
    let (status, _) = request(&app, "GET", "/api/v1/me", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "/api/v1 требует ключ");
    let bogus = format!("bbg_{}", "z".repeat(40));
    let (status, _) = request(&app, "GET", "/api/v1/me", None, Some(&bogus)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "неизвестный ключ");

    // запись через API: пост от владельца ключа
    let (status, body) = request(
        &app,
        "POST",
        "/api/v1/posts",
        Some(json!({ "body": "пост из API" })),
        Some(&key),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "пост через API: {body}");

    // и виден в ленте через API
    let (status, body) = request(&app, "GET", "/api/v1/feed", None, Some(&key)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.as_array()
            .unwrap()
            .iter()
            .any(|i| i["body"] == "пост из API"),
        "пост из API в ленте"
    );

    // запись через API: релиз трека (владелец верифицирован)
    let (status, body) = request(
        &app,
        "POST",
        "/api/v1/tracks",
        Some(json!({ "title": "API трек", "audio_url": "https://audio.example/api.mp3" })),
        Some(&key),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "релиз через API: {body}");
    let (_, body) = request(&app, "GET", "/api/v1/music", None, Some(&key)).await;
    assert!(
        body.as_array()
            .unwrap()
            .iter()
            .any(|i| i["title"] == "API трек"),
        "трек из API в разделе музыки"
    );

    // чужой не может отозвать ключ → 403
    let (status, _) = request(
        &app,
        "POST",
        &format!("/api-keys/{key_id}/revoke"),
        None,
        Some(&casual),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "отзыв только владельцем");

    // владелец отзывает → 200; отозванный ключ больше не аутентифицирует → 401
    let (status, _) = request(
        &app,
        "POST",
        &format!("/api-keys/{key_id}/revoke"),
        None,
        Some(&dev),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "владелец отзывает");
    let (status, _) = request(&app, "GET", "/api/v1/me", None, Some(&key)).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "отозванный ключ не работает"
    );

    // повторный отзыв → 409
    let (status, _) = request(
        &app,
        "POST",
        &format!("/api-keys/{key_id}/revoke"),
        None,
        Some(&dev),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "повторный отзыв");
}
