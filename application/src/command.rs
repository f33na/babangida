//! Командная сторона CQRS: use-cases записи поверх доменных портов. Каждый
//! проходит через доменные инварианты — лимиты и кулдаун проверяет `domain`
//! ([`Invite::issue`]/[`Invite::accept`]), а не хендлеры.

use babangida_domain::identity::{
    Invite, InviteAccepted, InviteCode, InviteId, InviteIssued, InviteRepository, IssuanceContext,
    UserId, UserRepository,
};

use crate::{ApplicationError, Clock, InviteCodeFactory};

/// Выдать инвайт от имени юзера.
pub struct IssueInviteCommand {
    pub inviter: UserId,
}

/// Use-case выдачи инвайта. Собирает контекст инварианта (квота из роли, число
/// активных и время последней выдачи из портов) и доверяет решение домену.
pub struct IssueInvite<U, I, C, F> {
    users: U,
    invites: I,
    clock: C,
    codes: F,
}

impl<U, I, C, F> IssueInvite<U, I, C, F>
where
    U: UserRepository,
    I: InviteRepository,
    C: Clock,
    F: InviteCodeFactory,
{
    pub fn new(users: U, invites: I, clock: C, codes: F) -> Self {
        Self {
            users,
            invites,
            clock,
            codes,
        }
    }

    /// ВНИМАНИЕ (закрыть в Prompt 2): чтение `active_count`/`last_issued_at` и
    /// последующий `save` здесь НЕ атомарны. Два параллельных вызова для одного
    /// инвайтера могут оба прочитать `active_count = 1` и обойти квоту (TOCTOU).
    /// Доменный инвариант корректен, но гарантия не сильнее консистентности чтения.
    /// При реализации в `infrastructure` обернуть count+проверку+insert в одну
    /// транзакцию с блокировкой строки инвайтера (`SELECT ... FOR UPDATE`) либо
    /// добавить partial-unique constraint на активные инвайты, маппя его в
    /// `RepositoryError::Conflict`.
    ///
    /// # Errors
    /// [`ApplicationError`]: инвайт не выдан (квота/кулдаун), инвайтер не найден
    /// или сбой хранилища.
    pub async fn execute(&self, cmd: IssueInviteCommand) -> Result<InviteIssued, ApplicationError> {
        let inviter = self
            .users
            .find_by_id(cmd.inviter)
            .await?
            .ok_or(ApplicationError::NotFound("inviter"))?;

        let ctx = IssuanceContext {
            quota: inviter.invite_quota(),
            active_count: self.invites.count_active_by_inviter(inviter.id()).await?,
            last_issued_at: self.invites.last_issued_at_by_inviter(inviter.id()).await?,
            now: self.clock.now(),
        };

        let (invite, event) = Invite::issue(
            InviteId::generate(),
            self.codes.generate(),
            inviter.id(),
            ctx,
        )?;
        self.invites.save(&invite).await?;
        Ok(event)
    }
}

/// Принять инвайт по коду.
pub struct AcceptInviteCommand {
    pub code: InviteCode,
    pub acceptor: UserId,
}

/// Use-case приёма инвайта.
pub struct AcceptInvite<I, C> {
    invites: I,
    clock: C,
}

impl<I, C> AcceptInvite<I, C>
where
    I: InviteRepository,
    C: Clock,
{
    pub fn new(invites: I, clock: C) -> Self {
        Self { invites, clock }
    }

    /// # Errors
    /// [`ApplicationError`]: инвайт не найден, не активен, либо собственный.
    pub async fn execute(
        &self,
        cmd: AcceptInviteCommand,
    ) -> Result<InviteAccepted, ApplicationError> {
        let mut invite = self
            .invites
            .find_by_code(&cmd.code)
            .await?
            .ok_or(ApplicationError::NotFound("invite"))?;
        let event = invite.accept(cmd.acceptor, self.clock.now())?;
        self.invites.save(&invite).await?;
        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use babangida_domain::RepositoryError;
    use babangida_domain::identity::{Handle, InviteError, User, UserRole};
    use babangida_shared::{Duration, Id, Timestamp};

    use super::*;

    struct FixedClock(Timestamp);
    impl Clock for FixedClock {
        fn now(&self) -> Timestamp {
            self.0
        }
    }

    struct FixedCode(InviteCode);
    impl InviteCodeFactory for FixedCode {
        fn generate(&self) -> InviteCode {
            self.0.clone()
        }
    }

    struct FakeUsers(Option<User>);
    #[async_trait]
    impl UserRepository for FakeUsers {
        async fn find_by_id(&self, _id: UserId) -> Result<Option<User>, RepositoryError> {
            Ok(self.0.clone())
        }
        async fn find_by_handle(&self, _h: &Handle) -> Result<Option<User>, RepositoryError> {
            Ok(self.0.clone())
        }
        async fn save(&self, _u: &User) -> Result<(), RepositoryError> {
            Ok(())
        }
    }

    struct FakeInvites {
        active: u32,
        last: Option<Timestamp>,
        by_code: Option<Invite>,
        saved: Mutex<Vec<Invite>>,
    }
    impl FakeInvites {
        fn empty() -> Self {
            Self {
                active: 0,
                last: None,
                by_code: None,
                saved: Mutex::new(Vec::new()),
            }
        }
    }
    #[async_trait]
    impl InviteRepository for FakeInvites {
        async fn find_by_id(&self, _id: InviteId) -> Result<Option<Invite>, RepositoryError> {
            Ok(None)
        }
        async fn find_by_code(
            &self,
            _code: &InviteCode,
        ) -> Result<Option<Invite>, RepositoryError> {
            Ok(self.by_code.clone())
        }
        async fn save(&self, invite: &Invite) -> Result<(), RepositoryError> {
            self.saved.lock().unwrap().push(invite.clone());
            Ok(())
        }
        async fn count_active_by_inviter(&self, _inviter: UserId) -> Result<u32, RepositoryError> {
            Ok(self.active)
        }
        async fn last_issued_at_by_inviter(
            &self,
            _inviter: UserId,
        ) -> Result<Option<Timestamp>, RepositoryError> {
            Ok(self.last)
        }
    }

    fn member(now: Timestamp) -> User {
        User::register(
            Id::generate(),
            Handle::parse("inviter1").unwrap(),
            UserRole::Member,
            now,
        )
    }

    fn code() -> InviteCode {
        InviteCode::parse("ABCD1234").unwrap()
    }

    #[tokio::test]
    async fn issue_invite_succeeds_and_saves() {
        let now = Timestamp::now();
        let invites = FakeInvites::empty();
        let uc = IssueInvite::new(
            FakeUsers(Some(member(now))),
            invites,
            FixedClock(now),
            FixedCode(code()),
        );
        let event = uc
            .execute(IssueInviteCommand {
                inviter: Id::generate(),
            })
            .await
            .unwrap();
        assert_eq!(event.code, code());
        assert_eq!(uc.invites.saved.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn issue_invite_surfaces_quota_error() {
        let now = Timestamp::now();
        let invites = FakeInvites {
            active: 2,
            ..FakeInvites::empty()
        };
        let uc = IssueInvite::new(
            FakeUsers(Some(member(now))),
            invites,
            FixedClock(now),
            FixedCode(code()),
        );
        let err = uc
            .execute(IssueInviteCommand {
                inviter: Id::generate(),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Invite(InviteError::QuotaExceeded { .. })
        ));
    }

    #[tokio::test]
    async fn issue_invite_surfaces_cooldown_error() {
        let now = Timestamp::now();
        let invites = FakeInvites {
            last: Some(now + Duration::hours(-1)),
            ..FakeInvites::empty()
        };
        let uc = IssueInvite::new(
            FakeUsers(Some(member(now))),
            invites,
            FixedClock(now),
            FixedCode(code()),
        );
        let err = uc
            .execute(IssueInviteCommand {
                inviter: Id::generate(),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ApplicationError::Invite(InviteError::CooldownActive { .. })
        ));
    }

    #[tokio::test]
    async fn issue_invite_missing_inviter_is_not_found() {
        let now = Timestamp::now();
        let uc = IssueInvite::new(
            FakeUsers(None),
            FakeInvites::empty(),
            FixedClock(now),
            FixedCode(code()),
        );
        let err = uc
            .execute(IssueInviteCommand {
                inviter: Id::generate(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ApplicationError::NotFound("inviter")));
    }

    #[tokio::test]
    async fn accept_invite_succeeds() {
        let now = Timestamp::now();
        let inviter = Id::generate();
        let (invite, _) = Invite::issue(
            Id::generate(),
            code(),
            inviter,
            IssuanceContext {
                quota: babangida_domain::identity::InviteQuota::Limited(2),
                active_count: 0,
                last_issued_at: None,
                now,
            },
        )
        .unwrap();
        let invites = FakeInvites {
            by_code: Some(invite),
            ..FakeInvites::empty()
        };
        let uc = AcceptInvite::new(invites, FixedClock(now));
        let event = uc
            .execute(AcceptInviteCommand {
                code: code(),
                acceptor: Id::generate(),
            })
            .await
            .unwrap();
        assert_eq!(event.inviter, inviter);
        assert_eq!(uc.invites.saved.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn accept_invite_unknown_code_is_not_found() {
        let now = Timestamp::now();
        let uc = AcceptInvite::new(FakeInvites::empty(), FixedClock(now));
        let err = uc
            .execute(AcceptInviteCommand {
                code: code(),
                acceptor: Id::generate(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ApplicationError::NotFound("invite")));
    }
}
