//! Прикладные порты для недетерминированных операций и транзакций. Держим их вне
//! домена, чтобы доменные правила оставались чистыми; реализации — в `infrastructure`.

use async_trait::async_trait;
use babangida_domain::RepositoryError;
use babangida_domain::auth::{Password, PasswordHash, SessionToken};
use babangida_domain::community::{Group, GroupId};
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

/// Хэширование и сверка паролей (argon2 и т.п.). Тяжёлая недетерминированная
/// операция (случайная соль) — на границе, не в домене (ADR-0013). Домен хранит
/// только результат — [`PasswordHash`].
pub trait PasswordHasher: Send + Sync {
    /// Захэшировать пароль (со случайной солью).
    fn hash(&self, password: &Password) -> PasswordHash;
    /// Сверить пароль с хэшем (constant-time внутри адаптера).
    fn verify(&self, password: &Password, hash: &PasswordHash) -> bool;
}

/// Генератор токенов сессий. Источник энтропии — здесь, на границе; домен токен
/// только валидирует ([`SessionToken::parse`]).
pub trait SessionTokenFactory: Send + Sync {
    /// Сгенерировать новый высокоэнтропийный токен.
    fn generate(&self) -> SessionToken;
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

/// Транзакция изменения членства/ролей в сообществе (ADR-0012, по образцу ADR-0011).
/// Блокирует строку группы (`SELECT ... FOR UPDATE`), отдаёт текущий агрегат,
/// принимает изменённый и фиксирует — чтобы инвариант «всегда есть владелец»
/// держался под конкуренцией (иначе два параллельных выхода/смены ролей могут
/// оставить группу без владельца).
#[async_trait]
pub trait GroupMembershipTx: Send {
    /// Заблокировать группу и вернуть её агрегат (с участниками). `None` — группы нет.
    async fn lock_group(&mut self, id: GroupId) -> Result<Option<Group>, RepositoryError>;
    /// Сохранить изменённый состав/роли в рамках транзакции.
    async fn save(&mut self, group: &Group) -> Result<(), RepositoryError>;
    /// Зафиксировать транзакцию.
    async fn commit(&mut self) -> Result<(), RepositoryError>;
}

/// Фабрика транзакций членства.
#[async_trait]
pub trait GroupMembershipTxFactory: Send + Sync {
    async fn begin(&self) -> Result<Box<dyn GroupMembershipTx>, RepositoryError>;
}
