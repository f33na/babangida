//! Сквозной HTTP-тест первого среза против реальной Postgres (ADR-0009).
//! Гоняет роутер через `oneshot` (без живого сервера). Без DATABASE_URL — скип.
//! Команды идут от текущего юзера из сессии (ADR-0013): admin и newcomer логинятся.

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

/// Поднять схему, засеять админа с паролем и залогинить его. Возвращает роутер и токен админа.
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
async fn http_slice_end_to_end() {
    let Some((app, admin_token)) = setup().await else {
        eprintln!("SKIP http_slice_end_to_end: DATABASE_URL не задан");
        return;
    };

    // admin выдаёт инвайт
    let (status, body) = request(&app, "POST", "/invites", None, Some(&admin_token)).await;
    assert_eq!(status, StatusCode::OK, "выдача: {body}");
    let code = body["code"].as_str().unwrap().to_owned();

    // регистрация по коду (с паролем)
    let (status, body) = request(
        &app,
        "POST",
        "/register",
        Some(json!({ "code": code, "handle": "newcomer", "display_name": "New Comer", "subculture": "underground", "password": "newcomerpass" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "регистрация: {body}");

    // повторное использование того же кода → инвайт уже не активен → 404
    let (status, _) = request(
        &app,
        "POST",
        "/register",
        Some(json!({ "code": code, "handle": "latecomer", "display_name": "Late", "subculture": "hiphop", "password": "latecomerpass" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "код одноразовый");

    // профиль (публичное чтение)
    let (status, body) = request(&app, "GET", "/profiles/newcomer", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["display_name"], "New Comer");
    assert_eq!(body["subculture"], "underground");
    assert_eq!(body["verified"], false);

    // newcomer логинится и постит от своего имени (автор — из сессии)
    let newcomer_token = login(&app, "newcomer", "newcomerpass").await;
    let (status, _) = request(
        &app,
        "POST",
        "/posts",
        Some(json!({ "body": "первый трек" })),
        Some(&newcomer_token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // пост без сессии → 401
    let (status, _) = request(
        &app,
        "POST",
        "/posts",
        Some(json!({ "body": "аноним" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "пост требует сессию");

    // лента (публичное чтение)
    let (status, body) = request(&app, "GET", "/feed", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let items = body.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["author_handle"], "newcomer");
    assert_eq!(items[0]["body"], "первый трек");

    // невалидный handle → 422 (домен валидирует на границе)
    let (status, body) = request(&app, "POST", "/invites", None, Some(&admin_token)).await;
    assert_eq!(status, StatusCode::OK, "admin без кулдауна: {body}");
    let code2 = body["code"].as_str().unwrap().to_owned();
    let (status, _) = request(
        &app,
        "POST",
        "/register",
        Some(json!({ "code": code2, "handle": "x", "display_name": "X", "subculture": "underground", "password": "shorthandlepass" })),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "короткий handle → 422"
    );
}
