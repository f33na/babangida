//! Дизайн-токены babangida как Rust-константы — framework-agnostic слой, который
//! шарится между web (Leptos) и mobile (Dioxus) по ADR-0008. Канон и светлая тема —
//! `../../babangida-vault/stable/design-system.md` и `theme.css`.

/// Палитра тёмной темы (дефолт).
pub mod dark {
    pub const BG: &str = "#0E0E0E";
    pub const BG_ALT: &str = "#121212";
    pub const SURFACE: &str = "#1A1A1A";
    pub const SURFACE_RAISED: &str = "#222222";
    pub const BORDER: &str = "#2A2A2A";
    pub const TEXT: &str = "#F5F1E8";
    pub const TEXT_MUTED: &str = "#A8A29A";
    pub const ACCENT: &str = "#6B4423";
    pub const ACCENT_HOVER: &str = "#8A5A2E";
    pub const ACCENT_CONTRAST: &str = "#F5F1E8";
    pub const DANGER: &str = "#8B2E2E";
}

/// Палитра светлой темы (инверсия на ivory).
pub mod light {
    pub const BG: &str = "#F5F1E8";
    pub const BG_ALT: &str = "#ECE6D8";
    pub const SURFACE: &str = "#FBF8F1";
    pub const SURFACE_RAISED: &str = "#FFFFFF";
    pub const BORDER: &str = "#DDD5C5";
    pub const TEXT: &str = "#1A1614";
    pub const TEXT_MUTED: &str = "#6B645C";
    pub const ACCENT: &str = "#6B4423";
    pub const ACCENT_HOVER: &str = "#5A381C";
    pub const ACCENT_CONTRAST: &str = "#F5F1E8";
    pub const DANGER: &str = "#9B2C2C";
}

/// Радиусы (px). Boxy-эстетика старого ВК — скругления минимальны.
pub const RADIUS: u8 = 2;
pub const RADIUS_LG: u8 = 4;
