//! Postgres-адаптеры контекста identity: репозиторий юзеров и атомарная
//! транзакция выдачи инвайта (ADR-0011).

use async_trait::async_trait;
use babangida_application::{
    InviterIssuanceState, IssueInviteTx, IssueInviteTxFactory, RegistrationTx,
    RegistrationTxFactory,
};
use babangida_domain::RepositoryError;
use babangida_domain::auth::Credential;
use babangida_domain::identity::{
    Handle, Invite, InviteCode, InviteQuota, InviteStatus, IssuanceContext, User, UserId, UserRole,
};
use babangida_domain::social::Profile;
use babangida_shared::{Id, Timestamp};
use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::map_sqlx;
use crate::pool::Db;

fn role_str(role: UserRole) -> &'static str {
    match role {
        UserRole::Member => "member",
        UserRole::Admin => "admin",
    }
}

fn parse_role(raw: &str) -> Result<UserRole, RepositoryError> {
    match raw {
        "member" => Ok(UserRole::Member),
        "admin" => Ok(UserRole::Admin),
        other => Err(RepositoryError::Unavailable(format!(
            "неизвестная роль в БД: {other}"
        ))),
    }
}

fn row_to_user(
    id: Uuid,
    handle: String,
    role: String,
    verified: String,
    created_at: OffsetDateTime,
) -> Result<User, RepositoryError> {
    let handle = Handle::parse(&handle)
        .map_err(|_| RepositoryError::Unavailable("повреждённый handle в БД".to_owned()))?;
    let mut user = User::register(
        Id::from_uuid(id),
        handle,
        parse_role(&role)?,
        Timestamp::from_offset(created_at),
    );
    if verified == "verified" {
        user.verify();
    }
    Ok(user)
}

/// Репозиторий юзеров на Postgres.
pub struct PgUserRepository {
    db: Db,
}

impl PgUserRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl babangida_domain::identity::UserRepository for PgUserRepository {
    async fn find_by_id(&self, id: UserId) -> Result<Option<User>, RepositoryError> {
        let row: Option<(String, String, String, OffsetDateTime)> =
            sqlx::query_as("SELECT handle, role, verified, created_at FROM users WHERE id = $1")
                .bind(id.as_uuid())
                .fetch_optional(&self.db)
                .await
                .map_err(map_sqlx)?;
        row.map(|(h, r, v, c)| row_to_user(id.as_uuid(), h, r, v, c))
            .transpose()
    }

    async fn find_by_handle(&self, handle: &Handle) -> Result<Option<User>, RepositoryError> {
        let row: Option<(Uuid, String, String, String, OffsetDateTime)> = sqlx::query_as(
            "SELECT id, handle, role, verified, created_at FROM users WHERE handle = $1",
        )
        .bind(handle.as_str())
        .fetch_optional(&self.db)
        .await
        .map_err(map_sqlx)?;
        row.map(|(id, h, r, v, c)| row_to_user(id, h, r, v, c))
            .transpose()
    }

    async fn save(&self, user: &User) -> Result<(), RepositoryError> {
        sqlx::query(
            "INSERT INTO users (id, handle, role, verified, created_at) VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (id) DO UPDATE SET handle = EXCLUDED.handle, role = EXCLUDED.role, \
             verified = EXCLUDED.verified",
        )
        .bind(user.id().as_uuid())
        .bind(user.handle().as_str())
        .bind(role_str(user.role()))
        .bind(if user.verified().is_verified() { "verified" } else { "casual" })
        .bind(user.created_at().into_offset())
        .execute(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }
}

/// Фабрика атомарных транзакций выдачи инвайта (ADR-0011).
pub struct PgIssueInviteTxFactory {
    db: Db,
}

impl PgIssueInviteTxFactory {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl IssueInviteTxFactory for PgIssueInviteTxFactory {
    async fn begin(&self) -> Result<Box<dyn IssueInviteTx>, RepositoryError> {
        let tx = self.db.begin().await.map_err(map_sqlx)?;
        Ok(Box::new(PgIssueInviteTx { tx: Some(tx) }))
    }
}

struct PgIssueInviteTx {
    tx: Option<Transaction<'static, Postgres>>,
}

impl PgIssueInviteTx {
    fn tx(&mut self) -> Result<&mut Transaction<'static, Postgres>, RepositoryError> {
        self.tx
            .as_mut()
            .ok_or_else(|| RepositoryError::Unavailable("транзакция уже завершена".to_owned()))
    }
}

#[async_trait]
impl IssueInviteTx for PgIssueInviteTx {
    async fn lock_inviter(
        &mut self,
        inviter: UserId,
    ) -> Result<Option<InviterIssuanceState>, RepositoryError> {
        let tx = self.tx()?;
        // Блокируем строку инвайтера: сериализует параллельные выдачи (ADR-0011).
        let role: Option<String> =
            sqlx::query_scalar("SELECT role FROM users WHERE id = $1 FOR UPDATE")
                .bind(inviter.as_uuid())
                .fetch_optional(&mut **tx)
                .await
                .map_err(map_sqlx)?;
        let Some(role) = role else {
            return Ok(None);
        };
        let active: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM invites WHERE inviter_id = $1 AND status = 'active'",
        )
        .bind(inviter.as_uuid())
        .fetch_one(&mut **tx)
        .await
        .map_err(map_sqlx)?;
        let last: Option<OffsetDateTime> =
            sqlx::query_scalar("SELECT max(created_at) FROM invites WHERE inviter_id = $1")
                .bind(inviter.as_uuid())
                .fetch_one(&mut **tx)
                .await
                .map_err(map_sqlx)?;
        Ok(Some(InviterIssuanceState {
            quota: parse_role(&role)?.invite_quota(),
            active_count: u32::try_from(active).unwrap_or(u32::MAX),
            last_issued_at: last.map(Timestamp::from_offset),
        }))
    }

    async fn insert_invite(&mut self, invite: &Invite) -> Result<(), RepositoryError> {
        let tx = self.tx()?;
        sqlx::query(
            "INSERT INTO invites (id, code, inviter_id, status, accepted_by, accepted_at, created_at) \
             VALUES ($1, $2, $3, 'active', NULL, NULL, $4)",
        )
        .bind(invite.id().as_uuid())
        .bind(invite.code().as_str())
        .bind(invite.inviter().as_uuid())
        .bind(invite.created_at().into_offset())
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    async fn commit(&mut self) -> Result<(), RepositoryError> {
        if let Some(tx) = self.tx.take() {
            tx.commit().await.map_err(map_sqlx)?;
        }
        Ok(())
    }
}

fn active_invite_from_row(
    id: Uuid,
    code: String,
    inviter: Uuid,
    created_at: OffsetDateTime,
) -> Result<Invite, RepositoryError> {
    let code = InviteCode::parse(&code)
        .map_err(|_| RepositoryError::Unavailable("повреждённый код инвайта в БД".to_owned()))?;
    // Реконституция активного инвайта через доменный API (см. patterns/repository).
    let (invite, _event) = Invite::issue(
        Id::from_uuid(id),
        code,
        Id::from_uuid(inviter),
        IssuanceContext {
            quota: InviteQuota::Unlimited,
            active_count: 0,
            last_issued_at: None,
            now: Timestamp::from_offset(created_at),
        },
    )
    .map_err(|_| RepositoryError::Unavailable("реконституция инвайта".to_owned()))?;
    Ok(invite)
}

/// Фабрика транзакций регистрации по инвайту (атомарно: инвайт + юзер + профиль).
pub struct PgRegistrationTxFactory {
    db: Db,
}

impl PgRegistrationTxFactory {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl RegistrationTxFactory for PgRegistrationTxFactory {
    async fn begin(&self) -> Result<Box<dyn RegistrationTx>, RepositoryError> {
        let tx = self.db.begin().await.map_err(map_sqlx)?;
        Ok(Box::new(PgRegistrationTx { tx: Some(tx) }))
    }
}

struct PgRegistrationTx {
    tx: Option<Transaction<'static, Postgres>>,
}

impl PgRegistrationTx {
    fn tx(&mut self) -> Result<&mut Transaction<'static, Postgres>, RepositoryError> {
        self.tx
            .as_mut()
            .ok_or_else(|| RepositoryError::Unavailable("транзакция уже завершена".to_owned()))
    }
}

#[async_trait]
impl RegistrationTx for PgRegistrationTx {
    async fn take_active_invite(
        &mut self,
        code: &InviteCode,
    ) -> Result<Option<Invite>, RepositoryError> {
        let tx = self.tx()?;
        let row: Option<(Uuid, String, Uuid, OffsetDateTime)> = sqlx::query_as(
            "SELECT id, code, inviter_id, created_at FROM invites \
             WHERE code = $1 AND status = 'active' FOR UPDATE",
        )
        .bind(code.as_str())
        .fetch_optional(&mut **tx)
        .await
        .map_err(map_sqlx)?;
        row.map(|(id, c, inviter, ts)| active_invite_from_row(id, c, inviter, ts))
            .transpose()
    }

    async fn insert_user(&mut self, user: &User) -> Result<(), RepositoryError> {
        let tx = self.tx()?;
        sqlx::query(
            "INSERT INTO users (id, handle, role, verified, created_at) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(user.id().as_uuid())
        .bind(user.handle().as_str())
        .bind(role_str(user.role()))
        .bind(if user.verified().is_verified() { "verified" } else { "casual" })
        .bind(user.created_at().into_offset())
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    async fn insert_profile(&mut self, profile: &Profile) -> Result<(), RepositoryError> {
        let tx = self.tx()?;
        sqlx::query(
            "INSERT INTO profiles (user_id, display_name, subculture, bio) VALUES ($1, $2, $3, $4)",
        )
        .bind(profile.user_id().as_uuid())
        .bind(profile.display_name().as_str())
        .bind(profile.subculture().as_str())
        .bind(profile.bio().map(babangida_domain::social::Bio::as_str))
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    async fn insert_credential(&mut self, credential: &Credential) -> Result<(), RepositoryError> {
        let tx = self.tx()?;
        sqlx::query(
            "INSERT INTO credentials (user_id, password_hash, established_at) VALUES ($1, $2, $3)",
        )
        .bind(credential.user().as_uuid())
        .bind(credential.hash().as_str())
        .bind(credential.established_at().into_offset())
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    async fn mark_invite_accepted(&mut self, invite: &Invite) -> Result<(), RepositoryError> {
        let (by, at) = match invite.status() {
            InviteStatus::Accepted { by, at } => (by.as_uuid(), at.into_offset()),
            InviteStatus::Active => {
                return Err(RepositoryError::Unavailable("инвайт не принят".to_owned()));
            }
        };
        let tx = self.tx()?;
        let res = sqlx::query(
            "UPDATE invites SET status = 'accepted', accepted_by = $1, accepted_at = $2 \
             WHERE id = $3 AND status = 'active'",
        )
        .bind(by)
        .bind(at)
        .bind(invite.id().as_uuid())
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(RepositoryError::Conflict);
        }
        Ok(())
    }

    async fn commit(&mut self) -> Result<(), RepositoryError> {
        if let Some(tx) = self.tx.take() {
            tx.commit().await.map_err(map_sqlx)?;
        }
        Ok(())
    }
}
