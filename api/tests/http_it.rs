//! Сквозной HTTP-тест первого среза против реальной Postgres (ADR-0009).
//! Гоняет роутер через `oneshot` (без живого сервера). Без DATABASE_URL — скип.

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
    sqlx::query("TRUNCATE users, invites, profiles, posts RESTART IDENTITY CASCADE")
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_slice_end_to_end() {
    let Some((db, admin_id)) = setup().await else {
        eprintln!("SKIP http_slice_end_to_end: DATABASE_URL не задан");
        return;
    };
    let app = router(AppState { db });

    // admin выдаёт инвайт
    let (status, body) = post_json(&app, "/invites", json!({ "inviter": admin_id })).await;
    assert_eq!(status, StatusCode::OK, "выдача: {body}");
    let code = body["code"].as_str().unwrap().to_owned();

    // регистрация по коду
    let (status, body) = post_json(
        &app,
        "/register",
        json!({ "code": code, "handle": "newcomer", "display_name": "New Comer", "subculture": "underground" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "регистрация: {body}");
    let user_id = body["user_id"].as_str().unwrap().to_owned();

    // повторное использование того же кода → инвайт уже не активен → 404
    let (status, _) = post_json(
        &app,
        "/register",
        json!({ "code": code, "handle": "latecomer", "display_name": "Late", "subculture": "hiphop" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "код одноразовый");

    // профиль
    let (status, body) = get_json(&app, "/profiles/newcomer").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["display_name"], "New Comer");
    assert_eq!(body["subculture"], "underground");
    assert_eq!(body["verified"], false);

    // пост + лента
    let (status, _) = post_json(
        &app,
        "/posts",
        json!({ "author": user_id, "body": "первый трек" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = get_json(&app, "/feed").await;
    assert_eq!(status, StatusCode::OK);
    let items = body.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["author_handle"], "newcomer");
    assert_eq!(items[0]["body"], "первый трек");

    // невалидный handle → 422 (домен валидирует на границе)
    let (status, body) = post_json(&app, "/invites", json!({ "inviter": admin_id })).await;
    assert_eq!(status, StatusCode::OK, "admin без кулдауна: {body}");
    let code2 = body["code"].as_str().unwrap().to_owned();
    let (status, _) = post_json(
        &app,
        "/register",
        json!({ "code": code2, "handle": "x", "display_name": "X", "subculture": "underground" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "короткий handle → 422"
    );
}
