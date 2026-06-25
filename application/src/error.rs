use babangida_domain::RepositoryError;
use babangida_domain::identity::InviteError;

/// Ошибка прикладного слоя. Оборачивает доменные нарушения и сбои портов, чтобы
/// `api`/UI получали единый тип на границе use-case'а.
#[derive(Debug, thiserror::Error)]
pub enum ApplicationError {
    /// Нарушение доменного правила инвайта (квота, кулдаун, статус).
    #[error(transparent)]
    Invite(#[from] InviteError),
    /// Сбой порта-репозитория.
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    /// Ожидаемая сущность не найдена (что именно — в строке).
    #[error("не найдено: {0}")]
    NotFound(&'static str),
}
