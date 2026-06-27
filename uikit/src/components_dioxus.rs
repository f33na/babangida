//! Dioxus-рендер компонентов babangida для mobile (ADR-0007/0008). Вёрстка
//! отдельная от Leptos (рендеры разные), но цвета/радиусы — из общих [`crate::tokens`].
//! Dioxus mobile рендерит в webview, поэтому стиль — обычный inline-CSS из токенов
//! (без Tailwind-сборки, как на web). Светлая тема на mobile — задача после PoC.

use crate::tokens::{RADIUS, dark};
use dioxus::prelude::*;

/// Акцентная кнопка (тёмно-коричневая). `onclick` — обработчик нажатия.
/// Зеркалит web-`Button` по смыслу; общий слой — токены, не разметка (ADR-0008).
#[component]
pub fn Button(onclick: EventHandler<MouseEvent>, children: Element) -> Element {
    let style = format!(
        "padding:10px 16px;border-radius:{}px;background:{};color:{};border:1px solid transparent;\
         font-weight:600;font-size:15px;cursor:pointer;",
        RADIUS,
        dark::ACCENT,
        dark::ACCENT_CONTRAST
    );
    rsx! {
        button { style: "{style}", onclick: move |e| onclick.call(e), {children} }
    }
}

/// Поле ввода с лейблом и двусторонней привязкой к сигналу. `password=true` —
/// тип `password`. Стиль — из токенов (ADR-0008).
#[component]
pub fn Field(
    label: String,
    mut value: Signal<String>,
    #[props(default)] password: bool,
    #[props(default)] placeholder: String,
) -> Element {
    let wrap = format!(
        "display:flex;flex-direction:column;gap:4px;color:{};font-size:14px;",
        dark::TEXT_MUTED
    );
    let input_style = format!(
        "padding:10px 12px;border-radius:{}px;border:1px solid {};background:{};color:{};font-size:15px;",
        RADIUS,
        dark::BORDER,
        dark::SURFACE_RAISED,
        dark::TEXT
    );
    let ty = if password { "password" } else { "text" };
    rsx! {
        label { style: "{wrap}",
            "{label}"
            input {
                style: "{input_style}",
                r#type: "{ty}",
                placeholder: "{placeholder}",
                value: value(),
                oninput: move |e| value.set(e.value()),
            }
        }
    }
}

/// Элемент ленты: аватар (первая буква handle) + автор + текст поста.
/// Зеркалит web-аналог по смыслу; общий слой — токены, не разметка (ADR-0008).
#[component]
pub fn FeedItem(author_handle: String, body: String) -> Element {
    let initial = author_handle
        .chars()
        .next()
        .map(|c| c.to_ascii_uppercase().to_string())
        .unwrap_or_default();

    let row = format!(
        "display:flex;gap:12px;padding:12px;border-bottom:1px solid {};",
        dark::BORDER
    );
    let avatar = format!(
        "width:40px;height:40px;flex-shrink:0;display:flex;align-items:center;\
         justify-content:center;background:{};color:{};border-radius:{}px;font-weight:700;",
        dark::ACCENT,
        dark::ACCENT_CONTRAST,
        RADIUS
    );
    let author = format!("font-weight:600;color:{};", dark::TEXT);
    let text = format!(
        "margin:0;color:{};white-space:pre-wrap;word-break:break-word;",
        dark::TEXT
    );

    rsx! {
        article { style: "{row}",
            div { style: "{avatar}", "{initial}" }
            div { style: "flex:1;min-width:0;",
                div { style: "{author}", "@{author_handle}" }
                p { style: "{text}", "{body}" }
            }
        }
    }
}
