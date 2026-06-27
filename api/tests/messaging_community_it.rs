//! Сквозной HTTP-тест среза messaging + community против реальной Postgres
//! (ADR-0009). Гоняет роутер через `oneshot`. Без DATABASE_URL — скип.
//! Действия атрибутируются текущему юзеру из сессии (ADR-0013): каждый логинится.

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

/// Поднять схему, засеять админа с паролем и залогинить. Возвращает роутер и токен админа.
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

/// Зарегистрировать юзера по свежему инвайту от админа и залогинить. Возвращает
/// (user_id, токен).
async fn register_user(app: &Router, admin_token: &str, handle: &str) -> (String, String) {
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
    let user_id = body["user_id"].as_str().unwrap().to_owned();
    let token = login(app, handle, "memberpass1").await;
    (user_id, token)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn messaging_and_community_end_to_end() {
    let Some((app, admin_token)) = setup().await else {
        eprintln!("SKIP messaging_and_community_end_to_end: DATABASE_URL не задан");
        return;
    };

    let (alpha_id, alpha) = register_user(&app, &admin_token, "alpha").await;
    let (beta_id, beta) = register_user(&app, &admin_token, "beta").await;

    // --- messaging ---
    let (status, body) = request(
        &app,
        "POST",
        "/messages",
        Some(json!({ "recipient": beta_id, "body": "йоу" })),
        Some(&alpha),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "отправка сообщения: {body}");
    let conversation_id = body["conversation_id"].as_str().unwrap().to_owned();

    // тред видит участник (alpha)
    let (status, body) = request(
        &app,
        "GET",
        &format!("/conversations/{conversation_id}/thread"),
        None,
        Some(&alpha),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let msgs = body.as_array().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["author_handle"], "alpha");
    assert_eq!(msgs[0]["body"], "йоу");

    // тред НЕ видит посторонний (admin не участник) → пусто
    let (status, body) = request(
        &app,
        "GET",
        &format!("/conversations/{conversation_id}/thread"),
        None,
        Some(&admin_token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 0, "чужой не видит тред");

    // входящие у второго участника (beta): собеседник — alpha
    let (status, body) = request(&app, "GET", "/inbox", None, Some(&beta)).await;
    assert_eq!(status, StatusCode::OK);
    let inbox = body.as_array().unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0]["counterpart_handle"], "alpha");
    assert_eq!(inbox[0]["last_message"], "йоу");

    // диалог с самим собой → 422
    let (status, _) = request(
        &app,
        "POST",
        "/messages",
        Some(json!({ "recipient": alpha_id, "body": "сам" })),
        Some(&alpha),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // --- community ---
    let (status, body) = request(
        &app,
        "POST",
        "/groups",
        Some(json!({ "slug": "podval", "name": "Подвал", "kind": "public" })),
        Some(&alpha),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "основание группы: {body}");
    let group_id = body["group_id"].as_str().unwrap().to_owned();

    let (status, body) = request(&app, "GET", "/groups/podval", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["kind"], "public");
    assert_eq!(body["member_count"], 1);

    // beta вступает в паблик
    let (status, body) = request(
        &app,
        "POST",
        &format!("/groups/{group_id}/join"),
        None,
        Some(&beta),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "вступление: {body}");
    assert_eq!(body["role"], "member");

    let (_, body) = request(&app, "GET", "/groups/podval", None, None).await;
    assert_eq!(body["member_count"], 2);

    // повторное вступление → конфликт
    let (status, _) = request(
        &app,
        "POST",
        &format!("/groups/{group_id}/join"),
        None,
        Some(&beta),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "повторное вступление");

    // рядовой участник (beta) меняет роли → запрещено
    let (status, _) = request(
        &app,
        "POST",
        &format!("/groups/{group_id}/role"),
        Some(json!({ "target": alpha_id, "role": "member" })),
        Some(&beta),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "не владелец не модерирует");

    // --- публикация в сообщество (анти-ВК: контент паблика — в общей ленте) ---
    // владелец паблика (alpha) постит → пост виден в общей ленте с меткой сообщества
    let (status, body) = request(
        &app,
        "POST",
        &format!("/groups/{group_id}/posts"),
        Some(json!({ "body": "трек из подвала" })),
        Some(&alpha),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "публикация в паблик: {body}");

    let (status, body) = request(&app, "GET", "/feed", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let item = body
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["body"] == "трек из подвала")
        .expect("пост паблика в общей ленте");
    assert_eq!(
        item["group_slug"], "podval",
        "в ленте проставлена метка паблика"
    );

    // рядовой участник паблика (beta) постить не может → запрещено
    let (status, _) = request(
        &app,
        "POST",
        &format!("/groups/{group_id}/posts"),
        Some(json!({ "body": "я тоже" })),
        Some(&beta),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "в паблике пишут только модераторы"
    );

    // --- закрытая группа: пост в ленте виден только участнику (viewer-aware) ---
    let (status, body) = request(
        &app,
        "POST",
        "/groups",
        Some(json!({ "slug": "bunker", "name": "Бункер", "kind": "closed" })),
        Some(&alpha),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "основание закрытой группы: {body}");
    let bunker_id = body["group_id"].as_str().unwrap().to_owned();

    let (status, body) = request(
        &app,
        "POST",
        &format!("/groups/{bunker_id}/posts"),
        Some(json!({ "body": "секрет из бункера" })),
        Some(&alpha),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "пост в закрытую группу: {body}");

    let sees_secret = |feed: &Value| {
        feed.as_array()
            .unwrap()
            .iter()
            .any(|i| i["body"] == "секрет из бункера")
    };

    // аноним пост закрытой группы НЕ видит
    let (_, body) = request(&app, "GET", "/feed", None, None).await;
    assert!(!sees_secret(&body), "аноним не видит пост закрытой группы");

    // не-участник (beta) — тоже не видит
    let (_, body) = request(&app, "GET", "/feed", None, Some(&beta)).await;
    assert!(
        !sees_secret(&body),
        "не-участник не видит пост закрытой группы"
    );

    // участник (alpha) видит — с меткой группы
    let (_, body) = request(&app, "GET", "/feed", None, Some(&alpha)).await;
    let item = body
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["body"] == "секрет из бункера")
        .expect("участник видит пост своей закрытой группы");
    assert_eq!(
        item["group_slug"], "bunker",
        "метка закрытой группы в ленте"
    );

    // единственный владелец (alpha) не может выйти → конфликт
    let (status, _) = request(
        &app,
        "POST",
        &format!("/groups/{group_id}/leave"),
        None,
        Some(&alpha),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "последний владелец не выходит"
    );
}
