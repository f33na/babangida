use babangida_domain::RepositoryError;

/// Маппинг ошибок sqlx в абстрактную доменную `RepositoryError`. Детали драйвера
/// наружу не текут (см. patterns/error-handling).
pub(crate) fn map_sqlx(err: sqlx::Error) -> RepositoryError {
    match &err {
        sqlx::Error::RowNotFound => RepositoryError::NotFound,
        sqlx::Error::Database(db) if db.is_unique_violation() => RepositoryError::Conflict,
        _ => RepositoryError::Unavailable(err.to_string()),
    }
}
