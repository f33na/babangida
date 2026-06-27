//! Dioxus-рендер компонентов babangida для mobile (ADR-0007/0008). Вёрстка отдельная
//! от Leptos (рендеры разные), но цвета/радиусы — через CSS-переменные `var(--…)`,
//! которые корень приложения инжектит из [`crate::theme`] (переключаемые dark/light).

use dioxus::prelude::*;

const ROW: &str = "display:flex;gap:12px;padding:12px;border-bottom:1px solid var(--border);";
const AVATAR: &str = "width:40px;height:40px;flex-shrink:0;display:flex;align-items:center;\
     justify-content:center;background:var(--accent);color:var(--accent-contrast);\
     border-radius:var(--radius);font-weight:700;";
const CARD: &str = "background:var(--surface);border:1px solid var(--border);\
     border-radius:var(--radius-lg);padding:16px;";
const BTN: &str = "padding:10px 16px;border-radius:var(--radius);background:var(--accent);\
     color:var(--accent-contrast);border:1px solid transparent;font-weight:600;\
     font-size:15px;cursor:pointer;";
const FIELD_WRAP: &str =
    "display:flex;flex-direction:column;gap:4px;color:var(--text-muted);font-size:14px;";
const FIELD_INPUT: &str = "padding:10px 12px;border-radius:var(--radius);\
     border:1px solid var(--border);background:var(--surface-raised);color:var(--text);\
     font-size:15px;";
const BADGE: &str = "display:inline-block;padding:2px 8px;border-radius:var(--radius);\
     font-size:12px;font-weight:600;background:var(--accent);color:var(--accent-contrast);";

/// Элемент ленты: аватар (первая буква handle) + автор + текст поста. Презентационный;
/// навигация (профиль/сообщество) — на стороне приложения (роутер), не здесь.
#[component]
pub fn FeedItem(author_handle: String, body: String) -> Element {
    let initial = author_handle
        .chars()
        .next()
        .map(|c| c.to_ascii_uppercase().to_string())
        .unwrap_or_default();
    rsx! {
        article { style: ROW,
            div { style: AVATAR, "{initial}" }
            div { style: "flex:1;min-width:0;",
                div { style: "font-weight:600;color:var(--text);", "@{author_handle}" }
                p { style: "margin:0;color:var(--text);white-space:pre-wrap;word-break:break-word;", "{body}" }
            }
        }
    }
}

/// Акцентная кнопка. `onclick` — обработчик нажатия.
#[component]
pub fn Button(onclick: EventHandler<MouseEvent>, children: Element) -> Element {
    rsx! {
        button { style: BTN, onclick: move |e| onclick.call(e), {children} }
    }
}

/// Поле ввода с лейблом и двусторонней привязкой к сигналу. `password=true` — тип
/// `password`.
#[component]
pub fn Field(
    label: String,
    mut value: Signal<String>,
    #[props(default)] password: bool,
    #[props(default)] placeholder: String,
) -> Element {
    let ty = if password { "password" } else { "text" };
    rsx! {
        label { style: FIELD_WRAP,
            "{label}"
            input {
                style: FIELD_INPUT,
                r#type: "{ty}",
                placeholder: "{placeholder}",
                value: value(),
                oninput: move |e| value.set(e.value()),
            }
        }
    }
}

/// Карточка-поверхность.
#[component]
pub fn Card(children: Element) -> Element {
    rsx! {
        div { style: CARD, {children} }
    }
}

/// Бейдж-чип (метка статуса/роли).
#[component]
pub fn Badge(children: Element) -> Element {
    rsx! {
        span { style: BADGE, {children} }
    }
}
