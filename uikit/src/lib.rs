//! Дизайн-система babangida (ADR-0008): токены + базовые UI-компоненты.
//! Токены ([`tokens`]) — framework-agnostic, доступны всегда (в т.ч. для mobile).
//! Leptos-компоненты — под фичей `leptos`. CSS-тема — `theme.css`.
//! См. `../../babangida-vault/stable/design-system.md`.

pub mod tokens;

#[cfg(feature = "leptos")]
mod components;
#[cfg(feature = "leptos")]
pub use components::{Avatar, Button, ButtonVariant, Card, FeedItem, Nav};
