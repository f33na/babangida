//! iOS-клиент babangida: тонкий нативный шелл вокруг общего `mobile-ui` (ADR-0007/0015).
//! Весь UI, навигация и data-client — в `babangida-mobile-ui`; здесь только запуск под
//! iOS (webview-рендер через wry/tao).
//!
//! `cargo check` собирает UI-ядро (rsx! без рендерера). Реальный app под устройство —
//! `dx build`/Xcode под фичей `shell` (нужен device-тулчейн, ADR-0007). См. `AGENTS.md`.

pub use babangida_mobile_ui::App;

/// Нативный шелл: запуск под iOS. Под фичей `shell` — требует device-тулчейн
/// (Xcode/`dx`), не `cargo check`. План Б — ADR-0007.
#[cfg(feature = "shell")]
pub fn launch() {
    dioxus::launch(App);
}
