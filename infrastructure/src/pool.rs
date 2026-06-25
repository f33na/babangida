use sqlx::postgres::{PgPool, PgPoolOptions};

/// Пул соединений Postgres — тип композиции для адаптеров.
pub type Db = PgPool;

/// Подключиться к Postgres.
///
/// # Errors
/// Ошибка sqlx при недоступности БД.
pub async fn connect(url: &str) -> Result<Db, sqlx::Error> {
    PgPoolOptions::new().max_connections(5).connect(url).await
}

/// Применить миграции из `migrations/` (встроены в бинарь на этапе компиляции).
///
/// # Errors
/// Ошибка применения миграции.
pub async fn run_migrations(db: &Db) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../migrations").run(db).await
}
