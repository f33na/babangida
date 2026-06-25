//! Прикладные порты для недетерминированных операций и транзакций. Держим их вне
//! домена, чтобы доменные правила оставались чистыми; реализации — в `infrastructure`.

use async_trait::async_trait;
use babangida_domain::RepositoryError;
use babangida_domain::identity::{Invite, InviteCode, InviteQuota, UserId};
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
