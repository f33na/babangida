//! Dioxus-рендер компонентов babangida для mobile (ADR-0007/0008). Вёрстка
//! отдельная от Leptos (рендеры разные), но цвета/радиусы — из общих [`crate::tokens`].
//! Dioxus mobile рендерит в webview, поэтому стиль — обычный inline-CSS из токенов
//! (без Tailwind-сборки, как на web). Светлая тема на mobile — задача после PoC.

use crate::tokens::{RADIUS, dark};
use dioxus::prelude::*;

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
