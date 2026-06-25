//! Сквозной HTTP-тест среза messaging + community против реальной Postgres
//! (ADR-0009). Гоняет роутер через `oneshot`. Без DATABASE_URL — скип.

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
    Some((db, admin.id().to_string()))
}

async fn post_json(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    read(app, req).await
}

async fn get_json(app: &Router, uri: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    read(app, req).await
}

async fn read(app: &Router, req: Request<Body>) -> (StatusCode, Value) {
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

/// Зарегистрировать юзера по свежему инвайту от админа, вернуть его id.
async fn register_user(app: &Router, admin_id: &str, handle: &str) -> String {
    let (status, body) = post_json(app, "/invites", json!({ "inviter": admin_id })).await;
    assert_eq!(status, StatusCode::OK, "выдача инвайта: {body}");
    let code = body["code"].as_str().unwrap().to_owned();
    let (status, body) = post_json(
        app,
        "/register",
        json!({ "code": code, "handle": handle, "display_name": handle, "subculture": "underground", "password": "memberpass1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "регистрация {handle}: {body}");
    body["user_id"].as_str().unwrap().to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn messaging_and_community_end_to_end() {
    let Some((db, admin_id)) = setup().await else {
        eprintln!("SKIP messaging_and_community_end_to_end: DATABASE_URL не задан");
        return;
    };
    let app = router(AppState { db });

    let alpha = register_user(&app, &admin_id, "alpha").await;
    let beta = register_user(&app, &admin_id, "beta").await;

    // --- messaging ---
    let (status, body) = post_json(
        &app,
        "/messages",
        json!({ "author": alpha, "recipient": beta, "body": "йоу" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "отправка сообщения: {body}");
    let conversation_id = body["conversation_id"].as_str().unwrap().to_owned();

    // тред видит участник
    let (status, body) = get_json(
        &app,
        &format!("/conversations/{conversation_id}/thread?viewer={alpha}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let msgs = body.as_array().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["author_handle"], "alpha");
    assert_eq!(msgs[0]["body"], "йоу");

    // тред НЕ видит посторонний (admin не участник) → пусто
    let (status, body) = get_json(
        &app,
        &format!("/conversations/{conversation_id}/thread?viewer={admin_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 0, "чужой не видит тред");

    // входящие у второго участника: собеседник — alpha
    let (status, body) = get_json(&app, &format!("/inbox?user={beta}")).await;
    assert_eq!(status, StatusCode::OK);
    let inbox = body.as_array().unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0]["counterpart_handle"], "alpha");
    assert_eq!(inbox[0]["last_message"], "йоу");

    // диалог с самим собой → 422
    let (status, _) = post_json(
        &app,
        "/messages",
        json!({ "author": alpha, "recipient": alpha, "body": "сам" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // --- community ---
    let (status, body) = post_json(
        &app,
        "/groups",
        json!({ "founder": alpha, "slug": "podval", "name": "Подвал", "kind": "public" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "основание группы: {body}");
    let group_id = body["group_id"].as_str().unwrap().to_owned();

    let (status, body) = get_json(&app, "/groups/podval").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["kind"], "public");
    assert_eq!(body["member_count"], 1);

    // beta вступает в паблик
    let (status, body) = post_json(
        &app,
        &format!("/groups/{group_id}/join"),
        json!({ "user": beta }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "вступление: {body}");
    assert_eq!(body["role"], "member");

    let (_, body) = get_json(&app, "/groups/podval").await;
    assert_eq!(body["member_count"], 2);

    // повторное вступление → конфликт
    let (status, _) = post_json(
        &app,
        &format!("/groups/{group_id}/join"),
        json!({ "user": beta }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "повторное вступление");

    // рядовой участник меняет роли → запрещено
    let (status, _) = post_json(
        &app,
        &format!("/groups/{group_id}/role"),
        json!({ "actor": beta, "target": alpha, "role": "member" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "не владелец не модерирует");

    // --- публикация в сообщество (анти-ВК: контент паблика — в общей ленте) ---
    // владелец паблика постит → пост виден в общей ленте с меткой сообщества
    let (status, body) = post_json(
        &app,
        &format!("/groups/{group_id}/posts"),
        json!({ "author": alpha, "body": "трек из подвала" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "публикация в паблик: {body}");

    let (status, body) = get_json(&app, "/feed").await;
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

    // рядовой участник паблика постить не может → запрещено
    let (status, _) = post_json(
        &app,
        &format!("/groups/{group_id}/posts"),
        json!({ "author": beta, "body": "я тоже" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "в паблике пишут только модераторы"
    );

    // единственный владелец не может выйти → конфликт
    let (status, _) = post_json(
        &app,
        &format!("/groups/{group_id}/leave"),
        json!({ "user": alpha }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "последний владелец не выходит"
    );
}
