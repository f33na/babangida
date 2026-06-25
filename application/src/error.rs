use babangida_domain::RepositoryError;
use babangida_domain::auth::AuthError;
use babangida_domain::community::CommunityError;
use babangida_domain::identity::InviteError;
use babangida_domain::marketplace::MarketplaceError;
use babangida_domain::messaging::MessagingError;

/// Ошибка прикладного слоя. Оборачивает доменные нарушения и сбои портов, чтобы
/// `api`/UI получали единый тип на границе use-case'а.
#[derive(Debug, thiserror::Error)]
pub enum ApplicationError {
    /// Нарушение доменного правила инвайта (квота, кулдаун, статус).
    #[error(transparent)]
    Invite(#[from] InviteError),
    /// Нарушение правила переписки (само-диалог, не участник).
    #[error(transparent)]
    Messaging(#[from] MessagingError),
    /// Нарушение правила сообщества (членство, роли, права).
    #[error(transparent)]
    Community(#[from] CommunityError),
    /// Сбой аутентификации (неверные данные или нет валидной сессии).
    #[error(transparent)]
    Auth(#[from] AuthError),
    /// Нарушение правила барахолки (гейт верификации, права продавца, статус).
    #[error(transparent)]
    Marketplace(#[from] MarketplaceError),
    /// Сбой порта-репозитория.
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    /// Ожидаемая сущность не найдена (что именно — в строке).
    #[error("не найдено: {0}")]
    NotFound(&'static str),
    /// Действие запрещено политикой (например, не админ). Что именно — в строке.
    #[error("запрещено: {0}")]
    Forbidden(&'static str),
}
