//! Тема mobile (Dioxus/webview): CSS-переменные из токенов, переключаемые dark/light.
//! Web использует статический `theme.css`; здесь генерим `:root`-переменные в рантайме,
//! Dioxus-компоненты ссылаются на `var(--…)` (как на web). Framework-agnostic. ADR-0008.

use crate::tokens::{RADIUS, RADIUS_LG};

/// Режим темы.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Dark,
    Light,
}

impl Mode {
    /// Противоположный режим (для переключателя).
    #[must_use]
    pub fn toggled(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }
}

/// CSS-переменные `:root` для режима — инжектится в `<style>` корня приложения.
#[must_use]
pub fn css_vars(mode: Mode) -> String {
    macro_rules! palette {
        ($m:ident) => {{
            use crate::tokens::$m as p;
            (
                p::BG,
                p::BG_ALT,
                p::SURFACE,
                p::SURFACE_RAISED,
                p::BORDER,
                p::TEXT,
                p::TEXT_MUTED,
                p::ACCENT,
                p::ACCENT_HOVER,
                p::ACCENT_CONTRAST,
                p::DANGER,
            )
        }};
    }
    let (
        bg,
        bg_alt,
        surface,
        surface_raised,
        border,
        text,
        text_muted,
        accent,
        accent_hover,
        accent_contrast,
        danger,
    ) = match mode {
        Mode::Dark => palette!(dark),
        Mode::Light => palette!(light),
    };
    format!(
        ":root{{--bg:{bg};--bg-alt:{bg_alt};--surface:{surface};--surface-raised:{surface_raised};\
         --border:{border};--text:{text};--text-muted:{text_muted};--accent:{accent};\
         --accent-hover:{accent_hover};--accent-contrast:{accent_contrast};--danger:{danger};\
         --radius:{RADIUS}px;--radius-lg:{RADIUS_LG}px;}}"
    )
}
