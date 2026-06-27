//! Общий mobile-UI babangida (ADR-0015): Dioxus 0.7 + dioxus-router, data-client
//! поверх HTTP-API, секьюр-стор токена сессии, переключаемая тема (dark/light).
//! `mobile-ios`/`mobile-android` — тонкие шеллы, делегируют в [`App`]; нативный
//! рендерер (webview) включают они под фичей `shell`.
//!
//! `cargo check` собирает UI-ядро (rsx! без рендерера). Реальный app под устройство —
//! `dx build` из шелл-крейта под фичей `shell` (нужен device-тулчейн, ADR-0007).

use babangida_uikit::theme::{self, Mode};
use dioxus::prelude::*;

mod client;
mod screens;
mod store;

use client::fetch_me;
use screens::{Feed, Group, Inbox, Login, Market, Profile, Shell, Thread};

/// Сессия в контексте (ADR-0015): токен (для Bearer) и handle вошедшего (чтобы не
/// предлагать написать самому себе). Оба `None` — гость.
#[derive(Clone, Default)]
pub(crate) struct Session {
    pub token: Option<String>,
    pub handle: Option<String>,
}

/// Маршруты приложения (dioxus-router). Анти-ВК: все экраны под единой оболочкой
/// [`Shell`] (общая нав), а не отдельными приложениями.
#[derive(Routable, Clone, PartialEq)]
pub(crate) enum Route {
    #[layout(Shell)]
    #[route("/")]
    Feed {},
    #[route("/login")]
    Login {},
    #[route("/u/:handle")]
    Profile { handle: String },
    #[route("/market")]
    Market {},
    #[route("/messages")]
    Inbox {},
    #[route("/messages/:id?:with")]
    Thread { id: String, with: String },
    #[route("/g/:slug")]
    Group { slug: String },
}

/// Корневой компонент. Тема (по умолчанию тёмная) и сессия (токен из секьюр-стора) —
/// в контексте; CSS-переменные темы инжектятся в `<style>` и переключаются реактивно.
#[component]
pub fn App() -> Element {
    let theme = use_context_provider(|| Signal::new(Mode::Dark));
    let mut session = use_context_provider(|| {
        Signal::new(Session {
            token: store::load(),
            handle: None,
        })
    });
    // Возобновление сессии: есть сохранённый токен, но handle ещё неизвестен —
    // подтягиваем `/me` (чтобы знать «свой профиль» и подтвердить валидность токена).
    use_effect(move || {
        if let (Some(tok), None) = {
            let s = session.peek();
            (s.token.clone(), s.handle.clone())
        } {
            spawn(async move {
                if let Some(me) = fetch_me(tok).await {
                    session.write().handle = Some(me.handle);
                }
            });
        }
    });
    let css = theme::css_vars(theme());
    rsx! {
        style { "{css}" }
        Router::<Route> {}
    }
}
