//! Прикладной слой babangida: use-cases, команды и запросы (CQRS, ADR-0004)
//! поверх доменных портов. Оркестрация без инфраструктуры и без HTTP.
//!
//! Запись ([`command`]) идёт через доменные инварианты (`domain` их и проверяет).
//! Чтение ([`query`]) — через read-модели, заточенные под экраны, отдельные от
//! агрегатов записи. См. `../../babangida-vault/COMMON.md`.

mod error;
mod ports;

pub mod command;
pub mod query;

pub use error::ApplicationError;
pub use ports::{Clock, InviteCodeFactory};
