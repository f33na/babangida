//! Дизайн-система babangida (ADR-0008): токены + базовые UI-компоненты.
//! Токены ([`tokens`]) — framework-agnostic, доступны всегда (в т.ч. для mobile).
//! Рендер-компоненты — под фичей фреймворка: Leptos в корне (фича `leptos`, web),
//! Dioxus в модуле [`dx`] (фича `dioxus`, mobile). CSS-тема web — `theme.css`.
//! См. `../../babangida-vault/stable/design-system.md`.

pub mod tokens;

#[cfg(feature = "leptos")]
mod components;
#[cfg(feature = "leptos")]
pub use components::{Avatar, Button, ButtonVariant, Card, FeedItem, Nav};

#[cfg(feature = "dioxus")]
mod components_dioxus;

/// Dioxus-рендер компонентов для mobile (ADR-0007). Отдельный модуль, чтобы имена
/// (`FeedItem`) не сталкивались с Leptos-версией в корне крейта.
#[cfg(feature = "dioxus")]
pub mod dx {
    pub use super::components_dioxus::FeedItem;
}
