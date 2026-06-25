//! Агрегат [`User`].

use babangida_shared::{Id, Timestamp};

use super::{Handle, InviteQuota, UserRole, VerifiedStatus};

/// Фантомный маркер для типизированного [`UserId`].
pub enum UserMarker {}
/// Идентификатор юзера.
pub type UserId = Id<UserMarker>;

/// Юзер — корень агрегата identity. Идентичность по [`UserId`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    id: UserId,
    handle: Handle,
    role: UserRole,
    verified: VerifiedStatus,
    created_at: Timestamp,
}

impl User {
    /// Зарегистрировать нового юзера (по умолчанию `Casual`). Идентификатор и время
    /// приходят с границы; домен их не генерирует.
    #[must_use]
    pub fn register(id: UserId, handle: Handle, role: UserRole, now: Timestamp) -> Self {
        Self {
            id,
            handle,
            role,
            verified: VerifiedStatus::Casual,
            created_at: now,
        }
    }

    /// Пометить верифицированным. Кто и при каких условиях это вызывает —
    /// решает `application` (гейт ADR-0010).
    pub fn verify(&mut self) {
        self.verified = VerifiedStatus::Verified;
    }

    /// Квота инвайтов юзера (из роли).
    #[must_use]
    pub const fn invite_quota(&self) -> InviteQuota {
        self.role.invite_quota()
    }

    #[must_use]
    pub const fn id(&self) -> UserId {
        self.id
    }

    #[must_use]
    pub fn handle(&self) -> &Handle {
        &self.handle
    }

    #[must_use]
    pub const fn role(&self) -> UserRole {
        self.role
    }

    #[must_use]
    pub const fn verified(&self) -> VerifiedStatus {
        self.verified
    }

    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handle() -> Handle {
        Handle::parse("rapper_one").expect("валидный handle")
    }

    #[test]
    fn registered_user_is_casual() {
        let u = User::register(Id::generate(), handle(), UserRole::Member, Timestamp::now());
        assert_eq!(u.verified(), VerifiedStatus::Casual);
        assert_eq!(
            u.invite_quota(),
            InviteQuota::Limited(super::super::MAX_ACTIVE_INVITES)
        );
    }

    #[test]
    fn verify_promotes_status() {
        let mut u = User::register(Id::generate(), handle(), UserRole::Member, Timestamp::now());
        u.verify();
        assert!(u.verified().is_verified());
    }

    #[test]
    fn admin_has_unlimited_quota() {
        let u = User::register(Id::generate(), handle(), UserRole::Admin, Timestamp::now());
        assert_eq!(u.invite_quota(), InviteQuota::Unlimited);
    }
}
