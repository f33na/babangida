//! Инфраструктура babangida: postgres-адаптеры (sqlx) под доменные/прикладные
//! порты. Зависимости направлены внутрь; `domain` про sqlx не знает (ADR-0003).
//! Композиция (какой адаптер под какой порт) — в `api`. См. `../../babangida-vault/COMMON.md`.

mod auth;
mod clock;
mod community;
mod content;
mod error;
mod identity;
mod invite_code;
mod marketplace;
mod messaging;
mod pool;

pub use auth::{
    Argon2PasswordHasher, PgCredentialRepository, PgSessionRepository, RandomSessionTokenFactory,
};
pub use clock::SystemClock;
pub use community::{
    PgGroupMembershipTxFactory, PgGroupPostRepository, PgGroupReadModel, PgGroupRepository,
};
pub use content::{PgFeedReadModel, PgPostRepository, PgProfileReadModel};
pub use identity::{PgIssueInviteTxFactory, PgRegistrationTxFactory, PgUserRepository};
pub use invite_code::RandomInviteCodeFactory;
pub use marketplace::{PgListingReadModel, PgListingRepository};
pub use messaging::{
    PgConversationRepository, PgInboxReadModel, PgMessageRepository, PgThreadReadModel,
};
pub use pool::{Db, connect, run_migrations};
