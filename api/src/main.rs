//! Бинарь сервера babangida: подключение к БД, миграции, запуск axum.

use babangida_api::{AppState, router};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL не задан (подними окружение через `nix develop`)")?;
    let db = babangida_infrastructure::connect(&url).await?;
    babangida_infrastructure::run_migrations(&db).await?;

    let addr = std::env::var("BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_owned());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("babangida api слушает на http://{addr}");
    axum::serve(listener, router(AppState { db })).await?;
    Ok(())
}
