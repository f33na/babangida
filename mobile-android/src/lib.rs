//! Android-клиент babangida: Dioxus + нативный шелл (ADR-0007 — самая рискованная
//! ставка проекта). Шарит `babangida-domain`, `babangida-shared` и `babangida-uikit`
//! (токены + Dioxus-виджеты) с остальным стеком. PoC-срез: один экран-лента,
//! собранный из доменных значений через общий `uikit`-виджет.
//!
//! `cargo check` собирает UI-ядро (rsx! без рендерера). Реальный app под устройство
//! — `dx build`/Android Studio под фичей `shell` (нужен SDK/NDK). См. `AGENTS.md`.

use babangida_domain::content::PostBody;
use babangida_domain::identity::Handle;
use babangida_uikit::dx::FeedItem;
use babangida_uikit::tokens::dark;
use dioxus::prelude::*;

/// Демоданные ленты как доменные значения — доказываем, что `domain` шарится в
/// mobile без правок (ADR-0007: домен чист от Dioxus, реконституция — на границе).
fn sample_feed() -> Vec<(Handle, PostBody)> {
    [
        ("root", "первый пост в подполье. добро пожаловать."),
        ("mc_kto", "сходка в субботу, по инвайтам. без посторонних."),
    ]
    .into_iter()
    .filter_map(|(h, b)| Some((Handle::parse(h).ok()?, PostBody::parse(b).ok()?)))
    .collect()
}

/// Корневой компонент: лента из общего `uikit`-виджета поверх доменных данных.
/// Цвета/радиусы — из общих токенов (ADR-0008), без хардкода.
#[component]
pub fn App() -> Element {
    let items: Vec<(String, String)> = sample_feed()
        .into_iter()
        .map(|(h, b)| (h.as_str().to_owned(), b.as_str().to_owned()))
        .collect();

    let page = format!(
        "min-height:100vh;margin:0;background:{};color:{};font-family:system-ui,sans-serif;",
        dark::BG,
        dark::TEXT
    );
    let bar = format!(
        "display:flex;align-items:center;height:56px;padding:0 16px;background:{};\
         border-bottom:1px solid {};font-weight:700;font-size:18px;letter-spacing:-0.01em;",
        dark::BG_ALT,
        dark::BORDER
    );

    rsx! {
        main { style: "{page}",
            header { style: "{bar}", "babangida" }
            for (handle , body) in items {
                FeedItem { key: "{handle}", author_handle: handle.clone(), body: body.clone() }
            }
        }
    }
}

/// Нативный шелл: запуск под Android (webview-рендер через wry/tao). Под фичей
/// `shell` — требует device-тулчейн (Android SDK/NDK/`dx`), не `cargo check`.
/// План Б — ADR-0007.
#[cfg(feature = "shell")]
pub fn launch() {
    dioxus::launch(App);
}
