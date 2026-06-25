//! Бинарь сервера babangida: подключение к БД, миграции, запуск axum.

use babangida_api::{AppState, bootstrap_admin, router};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL не задан (подними окружение через `nix develop`)")?;
    let db = babangida_infrastructure::connect(&url).await?;
    babangida_infrastructure::run_migrations(&db).await?;

    // Bootstrap-пароль сид-админа (ADR-0013): один раз через env, чтобы `root` мог войти.
    if let Ok(password) = std::env::var("ADMIN_BOOTSTRAP_PASSWORD") {
        let handle = std::env::var("ADMIN_BOOTSTRAP_HANDLE").unwrap_or_else(|_| "root".to_owned());
        match bootstrap_admin(&db, &handle, &password).await {
            Ok(true) => println!("bootstrap: учётные данные админа '{handle}' установлены"),
            Ok(false) => eprintln!("bootstrap: админ '{handle}' не найден — пропуск"),
            Err(e) => return Err(format!("bootstrap админа: {e}").into()),
        }
    }

    let addr = std::env::var("BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_owned());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("babangida api слушает на http://{addr}");
    axum::serve(listener, router(AppState { db })).await?;
    Ok(())
}
