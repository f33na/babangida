//! Сквозной HTTP-тест маркетплейса против реальной Postgres (ADR-0014/0010/0009).
//! Гоняет роутер через `oneshot`. Без DATABASE_URL — скип.
//!
//! Проверяет гейт верификации end-to-end: casual продавать не может (403) → админ
//! верифицирует → продажа проходит → товар виден в маркете и профиле → не-продавец
//! не закроет (403) → продано убирает из маркета → не-админ не верифицирует (403).

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
    sqlx::query("TRUNCATE users, groups, listings RESTART IDENTITY CASCADE")
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

/// Зарегистрировать юзера по инвайту от админа и залогинить. Возвращает токен.
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

fn has_title<'a>(body: &'a Value, title: &str) -> Option<&'a Value> {
    body.as_array()
        .unwrap()
        .iter()
        .find(|i| i["title"] == title)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn marketplace_end_to_end() {
    let Some((app, admin_token)) = setup().await else {
        eprintln!("SKIP marketplace_end_to_end: DATABASE_URL не задан");
        return;
    };

    let seller = register_user(&app, &admin_token, "seller").await;
    let listing_body = json!({ "title": "MPC 2000XL", "price": 45000, "description": "живой" });

    // casual продавать не может → 403
    let (status, _) = request(
        &app,
        "POST",
        "/listings",
        Some(listing_body.clone()),
        Some(&seller),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "casual не продаёт");

    // без сессии → 401
    let (status, _) = request(&app, "POST", "/listings", Some(listing_body.clone()), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "продажа требует сессию");

    // админ верифицирует продавца
    let (status, body) = request(
        &app,
        "POST",
        "/users/seller/verify",
        None,
        Some(&admin_token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "верификация: {body}");
    assert_eq!(body["verified"], true);

    // тот же токен продавца — статус перечитывается, верификация уже действует → 200
    let (status, body) = request(
        &app,
        "POST",
        "/listings",
        Some(listing_body.clone()),
        Some(&seller),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "верифицированный продаёт: {body}");
    let listing_id = body["listing_id"].as_str().unwrap().to_owned();
    assert_eq!(body["status"], "active");

    // товар в общем разделе маркета (публичное чтение)
    let (status, body) = request(&app, "GET", "/market", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let item = has_title(&body, "MPC 2000XL").expect("товар в маркете");
    assert_eq!(item["seller_handle"], "seller");
    assert_eq!(item["price"], 45000);
    assert_eq!(item["status"], "active");

    // и на профиле продавца (анти-ВК)
    let (status, body) = request(&app, "GET", "/profiles/seller/listings", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(has_title(&body, "MPC 2000XL").is_some(), "товар на профиле");

    // не-продавец не может отметить проданным → 403
    let buyer = register_user(&app, &admin_token, "buyer").await;
    let (status, _) = request(
        &app,
        "POST",
        &format!("/listings/{listing_id}/sold"),
        None,
        Some(&buyer),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "не продавец не закрывает");

    // продавец отмечает проданным → 200
    let (status, _) = request(
        &app,
        "POST",
        &format!("/listings/{listing_id}/sold"),
        None,
        Some(&seller),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // продано — уходит из активного раздела маркета
    let (_, body) = request(&app, "GET", "/market", None, None).await;
    assert!(
        has_title(&body, "MPC 2000XL").is_none(),
        "проданное не в маркете"
    );

    // но остаётся в истории продавца со статусом sold
    let (_, body) = request(&app, "GET", "/profiles/seller/listings", None, None).await;
    let item = has_title(&body, "MPC 2000XL").expect("проданное в истории продавца");
    assert_eq!(item["status"], "sold");

    // не-админ не может верифицировать → 403
    let (status, _) = request(&app, "POST", "/users/buyer/verify", None, Some(&seller)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "верифицирует только админ");
}
