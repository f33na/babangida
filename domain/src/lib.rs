//! Доменное ядро babangida — source of truth для типов и инвариантов (ADR-0003).
//! Чистый Rust: без axum/sqlx/leptos/dioxus и без I/O. Зависит только от
//! `babangida-shared`. Контексты — модулями: [`identity`], [`social`], [`content`]
//! (отдельные крейты пока не нужны). См. `../../babangida-vault/COMMON.md`.

mod error;
mod specification;

pub mod content;
pub mod identity;
pub mod social;

pub use error::RepositoryError;
pub use specification::{And, Not, Or, Specification};
