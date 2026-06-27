//! Общий mobile-UI babangida (ADR-0015): Dioxus-экраны + сигнальная навигация +
//! data-client поверх HTTP-API. `mobile-ios`/`mobile-android` — тонкие шеллы,
//! делегируют в [`App`]; нативный рендерер (webview) включают они под фичей `shell`.
//! Тема — тёмная (как PoC, ADR-0008); светлая — после стабилизации.
//!
//! `cargo check` собирает UI-ядро (rsx! без рендерера). Реальный app под устройство —
//! `dx build` из шелл-крейта под фичей `shell` (нужен device-тулчейн, ADR-0007).

use babangida_uikit::dx::{Button, FeedItem, Field};
use babangida_uikit::tokens::dark;
use dioxus::prelude::*;

mod client;
use client::{FeedItemDto, fetch_feed, login as api_login};

/// Активный экран (сигнальная навигация, ADR-0015 — без dioxus-router на MVP).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Feed,
    Login,
}

/// Сессия: токен в памяти (ADR-0015). `None` — гость.
#[derive(Clone, Default)]
struct Session(Option<String>);

/// Корневой компонент: тёмная страница, навигация, активный экран. Экран и сессия —
/// в контексте (сигналы), доступны вложенным компонентам.
#[component]
pub fn App() -> Element {
    let screen = use_context_provider(|| Signal::new(Screen::Feed));
    let _session = use_context_provider(|| Signal::new(Session::default()));

    let page = format!(
        "min-height:100vh;margin:0;background:{};color:{};font-family:system-ui,sans-serif;",
        dark::BG,
        dark::TEXT
    );

    rsx! {
        main { style: "{page}",
            Nav {}
            {
                match screen() {
                    Screen::Feed => rsx! { FeedScreen {} },
                    Screen::Login => rsx! { LoginScreen {} },
                }
            }
        }
    }
}

/// Верхняя навигация: бренд + переключение экранов + состояние сессии (анти-ВК —
/// единая оболочка, не отдельные приложения).
#[component]
fn Nav() -> Element {
    let mut screen = use_context::<Signal<Screen>>();
    let session = use_context::<Signal<Session>>();

    let bar = format!(
        "display:flex;align-items:center;justify-content:space-between;height:56px;\
         padding:0 16px;background:{};border-bottom:1px solid {};",
        dark::BG_ALT,
        dark::BORDER
    );
    let brand = format!(
        "font-weight:700;font-size:18px;letter-spacing:-0.01em;color:{};",
        dark::TEXT
    );
    let link = format!(
        "background:none;border:none;color:{};font-size:15px;cursor:pointer;padding:0;",
        dark::TEXT_MUTED
    );
    let logged = format!("color:{};font-size:14px;font-weight:600;", dark::ACCENT);

    rsx! {
        header { style: "{bar}",
            span { style: "{brand}", "babangida" }
            div { style: "display:flex;align-items:center;gap:16px;",
                button {
                    style: "{link}",
                    onclick: move |_| screen.set(Screen::Feed),
                    "лента"
                }
                if session().0.is_some() {
                    span { style: "{logged}", "вошёл" }
                } else {
                    button {
                        style: "{link}",
                        onclick: move |_| screen.set(Screen::Login),
                        "войти"
                    }
                }
            }
        }
    }
}

/// Лента: реальные посты из API (`GET /feed`, публичное чтение).
#[component]
fn FeedScreen() -> Element {
    let feed = use_resource(|| async move { fetch_feed().await });

    let muted = format!("padding:16px;color:{};", dark::TEXT_MUTED);
    let danger = format!("padding:16px;color:{};", dark::DANGER);

    rsx! {
        {
            match &*feed.read_unchecked() {
                Some(Ok(items)) if items.is_empty() => {
                    rsx! { p { style: "{muted}", "пока пусто" } }
                }
                Some(Ok(items)) => {
                    let items: Vec<FeedItemDto> = items.clone();
                    rsx! {
                        for it in items {
                            FeedItem {
                                key: "{it.author_handle}-{it.body}",
                                author_handle: it.author_handle.clone(),
                                body: it.body.clone(),
                            }
                        }
                    }
                }
                Some(Err(e)) => rsx! { p { style: "{danger}", "не удалось загрузить ленту: {e}" } },
                None => rsx! { p { style: "{muted}", "загрузка ленты…" } },
            }
        }
    }
}

/// Вход: handle/пароль → токен сессии в памяти, затем на ленту.
#[component]
fn LoginScreen() -> Element {
    let handle = use_signal(String::new);
    let password = use_signal(String::new);
    let mut error = use_signal(|| Option::<String>::None);
    let mut session = use_context::<Signal<Session>>();
    let mut screen = use_context::<Signal<Screen>>();

    let on_submit = move |_| {
        let (h, p) = (handle(), password());
        spawn(async move {
            match api_login(h, p).await {
                Ok(token) => {
                    session.set(Session(Some(token)));
                    error.set(None);
                    screen.set(Screen::Feed);
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };

    let wrap = "display:flex;flex-direction:column;gap:12px;padding:16px;max-width:420px;";
    let danger = format!("color:{};font-size:14px;", dark::DANGER);

    rsx! {
        div { style: "{wrap}",
            Field { label: "@handle", value: handle, placeholder: "handle" }
            Field { label: "пароль", value: password, password: true, placeholder: "пароль" }
            Button { onclick: on_submit, "войти" }
            if let Some(e) = error() {
                p { style: "{danger}", "{e}" }
            }
        }
    }
}
