//! Тест bootstrap-пароля сид-админа (ADR-0013) против реальной Postgres (ADR-0009).
//! Без DATABASE_URL — скип. Проверяет: до bootstrap `root` войти не может, после —
//! может; повтор идемпотентен; для несуществующего админа — пропуск.

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use babangida_api::{AppState, bootstrap_admin, router};
use babangida_domain::identity::{Handle, User, UserRepository, UserRole};
use babangida_infrastructure::{Db, PgUserRepository, connect, run_migrations};
use babangida_shared::{Id, Timestamp};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn setup() -> Option<(Db, Router)> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let db = connect(&url).await.ok()?;
    run_migrations(&db).await.expect("миграции");
    sqlx::query("TRUNCATE users, groups RESTART IDENTITY CASCADE")
        .execute(&db)
        .await
        .expect("truncate");
    // Сид-админ без кредов (как делает миграция 0002 для `root`).
    let root = User::register(
        Id::generate(),
        Handle::parse("root").unwrap(),
        UserRole::Admin,
        Timestamp::now(),
    );
    PgUserRepository::new(db.clone())
        .save(&root)
        .await
        .expect("seed root");
    let app = router(AppState { db: db.clone() });
    Some((db, app))
}

async fn login_status(app: &Router, handle: &str, password: &str) -> StatusCode {
    let req = Request::builder()
        .method("POST")
        .uri("/login")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({ "handle": handle, "password": password })).unwrap(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let _: Value = {
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        }
    };
    status
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bootstrap_lets_root_log_in() {
    let Some((db, app)) = setup().await else {
        eprintln!("SKIP bootstrap_lets_root_log_in: DATABASE_URL не задан");
        return;
    };

    // до bootstrap у root нет кредов → войти нельзя
    assert_eq!(
        login_status(&app, "root", "rootpass12").await,
        StatusCode::UNAUTHORIZED,
        "без кредов вход невозможен"
    );

    // bootstrap ставит креды
    assert!(
        bootstrap_admin(&db, "root", "rootpass12")
            .await
            .expect("bootstrap"),
        "креды установлены"
    );

    // теперь root входит
    assert_eq!(
        login_status(&app, "root", "rootpass12").await,
        StatusCode::OK,
        "после bootstrap вход проходит"
    );

    // идемпотентность: повтор не падает и снова true
    assert!(
        bootstrap_admin(&db, "root", "rootpass12")
            .await
            .expect("повтор bootstrap"),
    );

    // несуществующий админ → пропуск (Ok(false))
    assert!(
        !bootstrap_admin(&db, "nosuchadmin", "whatever12345")
            .await
            .expect("bootstrap несуществующего"),
        "нет такого админа — пропуск"
    );
}
