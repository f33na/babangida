//! Android-клиент babangida: тонкий нативный шелл вокруг общего `mobile-ui`
//! (ADR-0007/0015). Весь UI, навигация и data-client — в `babangida-mobile-ui`;
//! здесь только запуск под Android (webview-рендер через wry/tao).
//!
//! `cargo check` собирает UI-ядро (rsx! без рендерера). Реальный app под устройство —
//! `dx build`/Android Studio под фичей `shell` (нужен SDK/NDK). См. `AGENTS.md`.

pub use babangida_mobile_ui::App;

/// Нативный шелл: запуск под Android. Под фичей `shell` — требует device-тулчейн
/// (Android SDK/NDK/`dx`), не `cargo check`. План Б — ADR-0007.
#[cfg(feature = "shell")]
pub fn launch() {
    dioxus::launch(App);
}
