//! Сквозной HTTP-тест верификации против реальной Postgres (ADR-0016/0010/0009).
//! Гоняет роутер через `oneshot`. Без DATABASE_URL — скип.
//!
//! Проверяет процесс end-to-end: юзер подаёт заявку → дубль отклоняется (409) →
//! очередь видит только админ (не-админ 403) → не-админ не решает (403) → админ
//! одобряет → юзер становится верифицированным и открывает маркет → повторное
//! решение/повторная заявка отклоняются (409) → отказ по другой заявке пускает
//! новую (re-request).

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
    sqlx::query("TRUNCATE users, groups, listings, verification_requests RESTART IDENTITY CASCADE")
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

fn has_handle<'a>(body: &'a Value, handle: &str) -> Option<&'a Value> {
    body.as_array()
        .unwrap()
        .iter()
        .find(|i| i["requester_handle"] == handle)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn verification_end_to_end() {
    let Some((app, admin)) = setup().await else {
        eprintln!("SKIP verification_end_to_end: DATABASE_URL не задан");
        return;
    };

    let artist = register_user(&app, &admin, "artist").await;

    // ещё нет заявок → /verification/me пусто
    let (status, body) = request(&app, "GET", "/verification/me", None, Some(&artist)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, Value::Null, "до заявки статус пуст");

    // подача заявки → pending
    let (status, body) = request(
        &app,
        "POST",
        "/verification/requests",
        Some(json!({ "note": "залил три релиза, торгую битами" })),
        Some(&artist),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "подача заявки: {body}");
    assert_eq!(body["status"], "pending");
    let request_id = body["request_id"].as_str().unwrap().to_owned();

    // вторая заявка при открытой первой → 409
    let (status, _) = request(
        &app,
        "POST",
        "/verification/requests",
        Some(json!({})),
        Some(&artist),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "дубль заявки запрещён");

    // свой статус — pending
    let (_, body) = request(&app, "GET", "/verification/me", None, Some(&artist)).await;
    assert_eq!(body["status"], "pending");

    // очередь видит только админ
    let (status, _) = request(&app, "GET", "/verification/requests", None, Some(&artist)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "очередь — только админ");
    let (status, body) = request(&app, "GET", "/verification/requests", None, Some(&admin)).await;
    assert_eq!(status, StatusCode::OK);
    let item = has_handle(&body, "artist").expect("заявка artist в очереди");
    assert_eq!(item["note"], "залил три релиза, торгую битами");

    // не-админ не решает
    let (status, _) = request(
        &app,
        "POST",
        &format!("/verification/requests/{request_id}/approve"),
        Some(json!({})),
        Some(&artist),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "решает только админ");

    // до одобрения artist не может торговать (гейт закрыт)
    let listing = json!({ "title": "SP-404", "price": 30000 });
    let (status, _) = request(
        &app,
        "POST",
        "/listings",
        Some(listing.clone()),
        Some(&artist),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "до верификации продавать нельзя"
    );

    // админ одобряет
    let (status, body) = request(
        &app,
        "POST",
        &format!("/verification/requests/{request_id}/approve"),
        Some(json!({})),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "одобрение: {body}");
    assert_eq!(body["status"], "approved");

    // artist теперь верифицирован (статус из его же сессии)
    let (_, body) = request(&app, "GET", "/me", None, Some(&artist)).await;
    assert_eq!(body["verified"], true, "одобрение верифицировало юзера");

    // и гейт открыт — продажа проходит
    let (status, body) = request(&app, "POST", "/listings", Some(listing), Some(&artist)).await;
    assert_eq!(status, StatusCode::OK, "после верификации продаёт: {body}");

    // /verification/me — approved
    let (_, body) = request(&app, "GET", "/verification/me", None, Some(&artist)).await;
    assert_eq!(body["status"], "approved");

    // повторное решение по той же заявке → 409
    let (status, _) = request(
        &app,
        "POST",
        &format!("/verification/requests/{request_id}/approve"),
        Some(json!({})),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "заявка уже рассмотрена");

    // повторная заявка от уже верифицированного → 409
    let (status, _) = request(
        &app,
        "POST",
        "/verification/requests",
        Some(json!({})),
        Some(&artist),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "верифицированному заявка не нужна"
    );

    // отказ открывает право на новую заявку (re-request)
    let producer = register_user(&app, &admin, "producer").await;
    let (_, body) = request(
        &app,
        "POST",
        "/verification/requests",
        Some(json!({ "note": "первый заход" })),
        Some(&producer),
    )
    .await;
    let req2 = body["request_id"].as_str().unwrap().to_owned();
    let (status, body) = request(
        &app,
        "POST",
        &format!("/verification/requests/{req2}/reject"),
        Some(json!({ "reason": "аккаунт слишком новый" })),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "отказ: {body}");
    assert_eq!(body["status"], "rejected");

    let (_, body) = request(&app, "GET", "/verification/me", None, Some(&producer)).await;
    assert_eq!(body["status"], "rejected");
    assert_eq!(body["decision_reason"], "аккаунт слишком новый");

    // producer всё ещё не верифицирован
    let (_, body) = request(&app, "GET", "/me", None, Some(&producer)).await;
    assert_eq!(body["verified"], false, "отказ не верифицирует");

    // после отказа можно подать новую
    let (status, body) = request(
        &app,
        "POST",
        "/verification/requests",
        Some(json!({ "note": "второй заход" })),
        Some(&producer),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "после отказа новая заявка: {body}");
    assert_eq!(body["status"], "pending");

    // очередь: новая заявка producer есть, рассмотренные (artist/первая producer) — нет
    let (_, body) = request(&app, "GET", "/verification/requests", None, Some(&admin)).await;
    assert!(
        has_handle(&body, "producer").is_some(),
        "новая заявка в очереди"
    );
    assert!(
        has_handle(&body, "artist").is_none(),
        "одобренная заявка ушла из очереди"
    );
}
