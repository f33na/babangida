//! Сквозной HTTP-тест музыки против реальной Postgres (ADR-0017/0010/0009).
//! Гоняет роутер через `oneshot`. Без DATABASE_URL — скип.
//!
//! Проверяет гейт верификации end-to-end: casual релизить не может (403) → без
//! сессии 401 → админ верифицирует → релиз проходит → трек виден в /music и профиле
//! → кривой URL 422 → не-автор не снимет (403) → автор снимает → снятый уходит из
//! /music и профиля.

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
    sqlx::query("TRUNCATE users, tracks RESTART IDENTITY CASCADE")
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
async fn music_end_to_end() {
    let Some((app, admin)) = setup().await else {
        eprintln!("SKIP music_end_to_end: DATABASE_URL не задан");
        return;
    };

    let artist = register_user(&app, &admin, "artist").await;
    let track = json!({
        "title": "Подвал",
        "audio_url": "https://audio.example/podval.mp3",
        "genre": "boom bap"
    });

    // casual релизить не может → 403
    let (status, _) = request(&app, "POST", "/tracks", Some(track.clone()), Some(&artist)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "casual не релизит");

    // без сессии → 401
    let (status, _) = request(&app, "POST", "/tracks", Some(track.clone()), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "релиз требует сессию");

    // админ верифицирует артиста
    let (status, _) = request(&app, "POST", "/users/artist/verify", None, Some(&admin)).await;
    assert_eq!(status, StatusCode::OK, "верификация");

    // тот же токен — статус перечитывается, релиз проходит → 200
    let (status, body) = request(&app, "POST", "/tracks", Some(track.clone()), Some(&artist)).await;
    assert_eq!(status, StatusCode::OK, "верифицированный релизит: {body}");
    let track_id = body["track_id"].as_str().unwrap().to_owned();
    assert_eq!(body["status"], "published");

    // трек в общем разделе музыки (публичное чтение)
    let (status, body) = request(&app, "GET", "/music", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let item = has_title(&body, "Подвал").expect("трек в /music");
    assert_eq!(item["artist_handle"], "artist");
    assert_eq!(item["genre"], "boom bap");
    assert_eq!(item["audio_url"], "https://audio.example/podval.mp3");

    // и на профиле артиста (анти-ВК)
    let (status, body) = request(&app, "GET", "/profiles/artist/tracks", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(has_title(&body, "Подвал").is_some(), "трек на профиле");

    // кривой URL → 422
    let (status, _) = request(
        &app,
        "POST",
        "/tracks",
        Some(json!({ "title": "x", "audio_url": "not-a-url" })),
        Some(&artist),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "кривой URL");

    // не-автор не может снять → 403
    let other = register_user(&app, &admin, "other").await;
    let (status, _) = request(
        &app,
        "POST",
        &format!("/tracks/{track_id}/withdraw"),
        None,
        Some(&other),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "снять может только автор");

    // автор снимает → 200
    let (status, _) = request(
        &app,
        "POST",
        &format!("/tracks/{track_id}/withdraw"),
        None,
        Some(&artist),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // снятый трек уходит из /music и из профиля
    let (_, body) = request(&app, "GET", "/music", None, None).await;
    assert!(has_title(&body, "Подвал").is_none(), "снятое не в /music");
    let (_, body) = request(&app, "GET", "/profiles/artist/tracks", None, None).await;
    assert!(has_title(&body, "Подвал").is_none(), "снятое не в профиле");

    // повторное снятие → 409
    let (status, _) = request(
        &app,
        "POST",
        &format!("/tracks/{track_id}/withdraw"),
        None,
        Some(&artist),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "снять снятое нельзя");
}
