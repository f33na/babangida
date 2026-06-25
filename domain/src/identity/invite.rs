//! Агрегат [`Invite`] и инвариант его выдачи (ADR-0005).

use babangida_shared::{Duration, Id, Timestamp};

use super::{InviteCode, InviteQuota, UserId};

/// Максимум одновременно активных инвайтов у обычного юзера (ADR-0005).
pub const MAX_ACTIVE_INVITES: u32 = 2;
/// Кулдаун между выдачами инвайтов у обычного юзера (ADR-0005).
pub const INVITE_COOLDOWN: Duration = Duration::hours(12);

/// Фантомный маркер для типизированного [`InviteId`].
pub enum InviteMarker {}
/// Идентификатор инвайта.
pub type InviteId = Id<InviteMarker>;

/// Состояние инвайта. Активным считается невыданный (непринятый) — именно он
/// учитывается в квоте.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InviteStatus {
    /// Выпущен, ещё не принят.
    Active,
    /// Принят юзером `by` в момент `at`.
    Accepted { by: UserId, at: Timestamp },
}

/// Инвайт — корень агрегата. Создаётся только через [`Invite::issue`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    id: InviteId,
    code: InviteCode,
    inviter: UserId,
    status: InviteStatus,
    created_at: Timestamp,
}

/// Данные для проверки инварианта выдачи. Собирает `application` из портов
/// репозитория (число активных, время последней выдачи) перед вызовом [`Invite::issue`].
#[derive(Debug, Clone, Copy)]
pub struct IssuanceContext {
    /// Квота выдающего (из его роли).
    pub quota: InviteQuota,
    /// Сколько у него сейчас активных инвайтов.
    pub active_count: u32,
    /// Когда он выдавал инвайт в последний раз (для кулдауна).
    pub last_issued_at: Option<Timestamp>,
    /// Текущий момент.
    pub now: Timestamp,
}

impl Invite {
    /// Единственный способ выпустить инвайт. Здесь — и только здесь — живёт
    /// инвариант ADR-0005: не больше [`MAX_ACTIVE_INVITES`] активных, кулдаун
    /// [`INVITE_COOLDOWN`], админ (квота `Unlimited`) обходит и лимит, и кулдаун.
    ///
    /// # Errors
    /// - [`InviteError::QuotaExceeded`] — исчерпан лимит активных.
    /// - [`InviteError::CooldownActive`] — с прошлой выдачи прошло меньше кулдауна.
    pub fn issue(
        id: InviteId,
        code: InviteCode,
        inviter: UserId,
        ctx: IssuanceContext,
    ) -> Result<(Self, InviteIssued), InviteError> {
        if !ctx.quota.allows(ctx.active_count) {
            return Err(InviteError::QuotaExceeded {
                active: ctx.active_count,
                limit: ctx.quota.limit().unwrap_or(ctx.active_count),
            });
        }
        if ctx.quota.enforces_cooldown()
            && let Some(last) = ctx.last_issued_at
        {
            let elapsed = ctx.now.duration_since(last);
            if elapsed < INVITE_COOLDOWN {
                return Err(InviteError::CooldownActive {
                    retry_after: INVITE_COOLDOWN - elapsed,
                });
            }
        }
        let invite = Self {
            id,
            code: code.clone(),
            inviter,
            status: InviteStatus::Active,
            created_at: ctx.now,
        };
        let event = InviteIssued {
            invite_id: id,
            inviter,
            code,
            issued_at: ctx.now,
        };
        Ok((invite, event))
    }

    /// Принять активный инвайт. Нельзя принять неактивный или собственный инвайт.
    ///
    /// # Errors
    /// [`InviteError::NotActive`] или [`InviteError::SelfAccept`].
    pub fn accept(&mut self, by: UserId, now: Timestamp) -> Result<InviteAccepted, InviteError> {
        if !self.is_active() {
            return Err(InviteError::NotActive);
        }
        if by == self.inviter {
            return Err(InviteError::SelfAccept);
        }
        self.status = InviteStatus::Accepted { by, at: now };
        Ok(InviteAccepted {
            invite_id: self.id,
            code: self.code.clone(),
            inviter: self.inviter,
            accepted_by: by,
            accepted_at: now,
        })
    }

    /// Активен ли инвайт (учитывается в квоте).
    #[must_use]
    pub fn is_active(&self) -> bool {
        matches!(self.status, InviteStatus::Active)
    }

    #[must_use]
    pub const fn id(&self) -> InviteId {
        self.id
    }

    #[must_use]
    pub const fn code(&self) -> &InviteCode {
        &self.code
    }

    #[must_use]
    pub const fn inviter(&self) -> UserId {
        self.inviter
    }

    #[must_use]
    pub const fn status(&self) -> &InviteStatus {
        &self.status
    }

    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }
}

/// Нарушение правил выдачи/приёма инвайта.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InviteError {
    #[error("исчерпана квота активных инвайтов: {active}/{limit}")]
    QuotaExceeded { active: u32, limit: u32 },
    #[error("кулдаун выдачи: ещё {seconds} c", seconds = retry_after.whole_seconds())]
    CooldownActive { retry_after: Duration },
    #[error("инвайт не активен")]
    NotActive,
    #[error("нельзя принять собственный инвайт")]
    SelfAccept,
}

/// Инвайт выпущен.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteIssued {
    pub invite_id: InviteId,
    pub inviter: UserId,
    pub code: InviteCode,
    pub issued_at: Timestamp,
}

/// Инвайт принят.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteAccepted {
    pub invite_id: InviteId,
    pub code: InviteCode,
    pub inviter: UserId,
    pub accepted_by: UserId,
    pub accepted_at: Timestamp,
}

/// Доменное событие контекста identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityEvent {
    InviteIssued(InviteIssued),
    InviteAccepted(InviteAccepted),
}

impl From<InviteIssued> for IdentityEvent {
    fn from(event: InviteIssued) -> Self {
        Self::InviteIssued(event)
    }
}

impl From<InviteAccepted> for IdentityEvent {
    fn from(event: InviteAccepted) -> Self {
        Self::InviteAccepted(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid() -> UserId {
        Id::generate()
    }

    fn iid() -> InviteId {
        Id::generate()
    }

    fn code() -> InviteCode {
        InviteCode::parse("ABCD1234").expect("валидный код")
    }

    fn ctx(
        quota: InviteQuota,
        active_count: u32,
        last_issued_at: Option<Timestamp>,
        now: Timestamp,
    ) -> IssuanceContext {
        IssuanceContext {
            quota,
            active_count,
            last_issued_at,
            now,
        }
    }

    #[test]
    fn issues_when_under_quota_and_no_prior() {
        let now = Timestamp::now();
        let (invite, event) = Invite::issue(
            iid(),
            code(),
            uid(),
            ctx(InviteQuota::Limited(2), 0, None, now),
        )
        .expect("должен выдаться");
        assert!(invite.is_active());
        assert_eq!(event.issued_at, now);
    }

    #[test]
    fn quota_blocks_when_active_at_limit() {
        let now = Timestamp::now();
        let err = Invite::issue(
            iid(),
            code(),
            uid(),
            ctx(
                InviteQuota::Limited(2),
                2,
                Some(now + Duration::hours(-24)),
                now,
            ),
        )
        .unwrap_err();
        assert_eq!(
            err,
            InviteError::QuotaExceeded {
                active: 2,
                limit: 2
            }
        );
    }

    #[test]
    fn cooldown_blocks_within_window() {
        let now = Timestamp::now();
        let last = now + Duration::hours(-6); // 6ч назад, кулдаун 12ч
        let err = Invite::issue(
            iid(),
            code(),
            uid(),
            ctx(InviteQuota::Limited(2), 0, Some(last), now),
        )
        .unwrap_err();
        match err {
            InviteError::CooldownActive { retry_after } => {
                assert_eq!(retry_after, Duration::hours(6));
            }
            other => panic!("ожидался CooldownActive, получено {other:?}"),
        }
    }

    #[test]
    fn cooldown_passes_after_window() {
        let now = Timestamp::now();
        let last = now + Duration::hours(-13);
        assert!(
            Invite::issue(
                iid(),
                code(),
                uid(),
                ctx(InviteQuota::Limited(2), 1, Some(last), now)
            )
            .is_ok()
        );
    }

    #[test]
    fn cooldown_boundary_exactly_12h_is_allowed() {
        let now = Timestamp::now();
        let last = now + Duration::hours(-12); // ровно кулдаун → elapsed >= cooldown
        assert!(
            Invite::issue(
                iid(),
                code(),
                uid(),
                ctx(InviteQuota::Limited(2), 0, Some(last), now)
            )
            .is_ok()
        );
    }

    #[test]
    fn admin_bypasses_quota_and_cooldown() {
        let now = Timestamp::now();
        let last = now + Duration::minutes(-1); // выдавал только что
        assert!(
            Invite::issue(
                iid(),
                code(),
                uid(),
                ctx(InviteQuota::Unlimited, 999, Some(last), now)
            )
            .is_ok()
        );
    }

    #[test]
    fn accept_active_marks_accepted_and_emits_event() {
        let now = Timestamp::now();
        let inviter = uid();
        let (mut invite, _) = Invite::issue(
            iid(),
            code(),
            inviter,
            ctx(InviteQuota::Limited(2), 0, None, now),
        )
        .unwrap();
        let invitee = uid();
        let event = invite
            .accept(invitee, now + Duration::hours(1))
            .expect("принятие активного");
        assert_eq!(event.accepted_by, invitee);
        assert!(!invite.is_active());
    }

    #[test]
    fn cannot_accept_twice() {
        let now = Timestamp::now();
        let (mut invite, _) = Invite::issue(
            iid(),
            code(),
            uid(),
            ctx(InviteQuota::Limited(2), 0, None, now),
        )
        .unwrap();
        invite.accept(uid(), now).unwrap();
        assert_eq!(
            invite.accept(uid(), now).unwrap_err(),
            InviteError::NotActive
        );
    }

    #[test]
    fn cannot_accept_own_invite() {
        let now = Timestamp::now();
        let inviter = uid();
        let (mut invite, _) = Invite::issue(
            iid(),
            code(),
            inviter,
            ctx(InviteQuota::Limited(2), 0, None, now),
        )
        .unwrap();
        assert_eq!(
            invite.accept(inviter, now).unwrap_err(),
            InviteError::SelfAccept
        );
    }
}
