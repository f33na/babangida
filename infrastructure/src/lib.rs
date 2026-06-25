//! Инфраструктура babangida: postgres-адаптеры (sqlx) под доменные/прикладные
//! порты. Зависимости направлены внутрь; `domain` про sqlx не знает (ADR-0003).
//! Композиция (какой адаптер под какой порт) — в `api`. См. `../../babangida-vault/COMMON.md`.

mod clock;
mod content;
mod error;
mod identity;
mod invite_code;
mod pool;

pub use clock::SystemClock;
pub use content::{PgFeedReadModel, PgPostRepository, PgProfileReadModel};
pub use identity::{PgIssueInviteTxFactory, PgRegistrationTxFactory, PgUserRepository};
pub use invite_code::RandomInviteCodeFactory;
pub use pool::{Db, connect, run_migrations};
