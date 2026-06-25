//! Порты репозиториев контекста identity. Это интерфейсы, которые домен требует
//! от внешнего мира; реализации — в `infrastructure` (ADR-0003). Методы для квоты
//! и кулдауна (`count_active_*`, `last_issued_at_*`) кормят инвариант [`super::Invite::issue`].

use async_trait::async_trait;
use babangida_shared::Timestamp;

use super::{Handle, Invite, InviteCode, InviteId, User, UserId};
use crate::RepositoryError;

/// Хранилище юзеров.
#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn find_by_id(&self, id: UserId) -> Result<Option<User>, RepositoryError>;
    async fn find_by_handle(&self, handle: &Handle) -> Result<Option<User>, RepositoryError>;
    async fn save(&self, user: &User) -> Result<(), RepositoryError>;
}

/// Хранилище инвайтов.
#[async_trait]
pub trait InviteRepository: Send + Sync {
    async fn find_by_id(&self, id: InviteId) -> Result<Option<Invite>, RepositoryError>;
    async fn find_by_code(&self, code: &InviteCode) -> Result<Option<Invite>, RepositoryError>;
    async fn save(&self, invite: &Invite) -> Result<(), RepositoryError>;
    /// Число активных (непринятых) инвайтов юзера — для проверки квоты.
    async fn count_active_by_inviter(&self, inviter: UserId) -> Result<u32, RepositoryError>;
    /// Время последней выдачи инвайта юзером — для проверки кулдауна.
    async fn last_issued_at_by_inviter(
        &self,
        inviter: UserId,
    ) -> Result<Option<Timestamp>, RepositoryError>;
}
