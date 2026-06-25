//! Прикладные порты для недетерминированных операций и транзакций. Держим их вне
//! домена, чтобы доменные правила оставались чистыми; реализации — в `infrastructure`.

use async_trait::async_trait;
use babangida_domain::RepositoryError;
use babangida_domain::identity::{Invite, InviteCode, InviteQuota, User, UserId};
use babangida_domain::social::Profile;
use babangida_shared::Timestamp;

/// Источник текущего времени (адаптер часов).
pub trait Clock: Send + Sync {
    /// Текущий момент.
    fn now(&self) -> Timestamp;
}

/// Генератор кодов приглашений. Случайность — здесь, на границе, не в домене:
/// домен код только валидирует.
pub trait InviteCodeFactory: Send + Sync {
    /// Сгенерировать новый валидный код.
    fn generate(&self) -> InviteCode;
}

/// Состояние инвайтера, прочитанное ПОД блокировкой строки (для инварианта выдачи,
/// ADR-0005/0011). Квота берётся из роли.
#[derive(Debug, Clone, Copy)]
pub struct InviterIssuanceState {
    pub quota: InviteQuota,
    pub active_count: u32,
    pub last_issued_at: Option<Timestamp>,
}

/// Транзакция выдачи инвайта (ADR-0011). Всё в одной БД-транзакции: блокировка
/// инвайтера, чтение состояния, вставка. Доменное решение принимается между этими
/// шагами; при `drop` без `commit` — откат.
#[async_trait]
pub trait IssueInviteTx: Send {
    /// Заблокировать строку инвайтера (`SELECT ... FOR UPDATE`) и прочитать состояние
    /// для инварианта. `None` — инвайтера нет.
    async fn lock_inviter(
        &mut self,
        inviter: UserId,
    ) -> Result<Option<InviterIssuanceState>, RepositoryError>;

    /// Вставить выпущенный инвайт в рамках транзакции.
    async fn insert_invite(&mut self, invite: &Invite) -> Result<(), RepositoryError>;

    /// Зафиксировать транзакцию.
    async fn commit(&mut self) -> Result<(), RepositoryError>;
}

/// Фабрика транзакций выдачи.
#[async_trait]
pub trait IssueInviteTxFactory: Send + Sync {
    async fn begin(&self) -> Result<Box<dyn IssueInviteTx>, RepositoryError>;
}

/// Транзакция регистрации по инвайту. Всё в одной БД-транзакции: блокировка
/// активного инвайта по коду, создание юзера и профиля, пометка инвайта принятым.
/// Атомарно — иначе возможны полу-состояния (юзер без принятого инвайта и наоборот).
#[async_trait]
pub trait RegistrationTx: Send {
    /// Заблокировать активный инвайт по коду (`SELECT ... FOR UPDATE`) и вернуть его.
    /// `None` — кода нет или он уже не активен.
    async fn take_active_invite(
        &mut self,
        code: &InviteCode,
    ) -> Result<Option<Invite>, RepositoryError>;

    /// Вставить нового юзера (нарушение уникальности handle → `Conflict`).
    async fn insert_user(&mut self, user: &User) -> Result<(), RepositoryError>;

    /// Вставить профиль нового юзера.
    async fn insert_profile(&mut self, profile: &Profile) -> Result<(), RepositoryError>;

    /// Пометить инвайт принятым (status + accepted_by/at).
    async fn mark_invite_accepted(&mut self, invite: &Invite) -> Result<(), RepositoryError>;

    /// Зафиксировать транзакцию.
    async fn commit(&mut self) -> Result<(), RepositoryError>;
}

/// Фабрика транзакций регистрации.
#[async_trait]
pub trait RegistrationTxFactory: Send + Sync {
    async fn begin(&self) -> Result<Box<dyn RegistrationTx>, RepositoryError>;
}
