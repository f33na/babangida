//! Конкретные спецификации контекста identity (паттерн [`crate::Specification`]).

use crate::Specification;

use super::{Invite, User};

/// Инвайт активен (учитывается в квоте).
pub struct ActiveInvite;

impl Specification<Invite> for ActiveInvite {
    fn is_satisfied_by(&self, invite: &Invite) -> bool {
        invite.is_active()
    }
}

/// Юзер верифицирован — гейт привилегий (ADR-0010).
pub struct VerifiedUser;

impl Specification<User> for VerifiedUser {
    fn is_satisfied_by(&self, user: &User) -> bool {
        user.verified().is_verified()
    }
}

#[cfg(test)]
mod tests {
    use babangida_shared::{Id, Timestamp};

    use super::*;
    use crate::identity::{Handle, InviteCode, InviteQuota, IssuanceContext, UserRole};

    #[test]
    fn active_invite_spec_tracks_status() {
        let now = Timestamp::now();
        let (mut invite, _) = Invite::issue(
            Id::generate(),
            InviteCode::parse("ABCD1234").unwrap(),
            Id::generate(),
            IssuanceContext {
                quota: InviteQuota::Limited(2),
                active_count: 0,
                last_issued_at: None,
                now,
            },
        )
        .unwrap();
        assert!(ActiveInvite.is_satisfied_by(&invite));
        invite.accept(Id::generate(), now).unwrap();
        assert!(!ActiveInvite.is_satisfied_by(&invite));
    }

    #[test]
    fn verified_user_spec_tracks_status() {
        let mut user = User::register(
            Id::generate(),
            Handle::parse("mc_test").unwrap(),
            UserRole::Member,
            Timestamp::now(),
        );
        assert!(!VerifiedUser.is_satisfied_by(&user));
        user.verify();
        assert!(VerifiedUser.is_satisfied_by(&user));
    }
}
