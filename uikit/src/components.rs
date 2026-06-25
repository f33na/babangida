//! Базовые Leptos-компоненты babangida. Tailwind-классы ссылаются на токены
//! (`var(--…)` из `theme.css`), не на хардкод-цвета (ADR-0008, design-system.md).
//! Референс по API/структуре — shadcn/leptos-ui, но без копирования.

use leptos::prelude::*;

/// Вариант кнопки.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    /// Акцентная (тёмно-коричневая).
    #[default]
    Primary,
    /// Призрачная (граница, прозрачный фон).
    Ghost,
}

impl ButtonVariant {
    fn class(self) -> &'static str {
        match self {
            Self::Primary => {
                "px-4 py-2 rounded-[var(--radius)] bg-[var(--accent)] text-[var(--accent-contrast)] \
                 hover:bg-[var(--accent-hover)] font-semibold border border-transparent"
            }
            Self::Ghost => {
                "px-4 py-2 rounded-[var(--radius)] bg-transparent text-[var(--text)] \
                 hover:bg-[var(--surface-raised)] font-semibold border border-[var(--border)]"
            }
        }
    }
}

/// Кнопка. `submit` делает её `type=submit` (для форм).
#[component]
pub fn Button(
    #[prop(optional)] variant: ButtonVariant,
    #[prop(optional)] submit: bool,
    children: Children,
) -> impl IntoView {
    let ty = if submit { "submit" } else { "button" };
    view! {
        <button type=ty class=variant.class()>
            {children()}
        </button>
    }
}

/// Карточка-поверхность.
#[component]
pub fn Card(children: Children) -> impl IntoView {
    view! {
        <div class="bg-[var(--surface)] border border-[var(--border)] rounded-[var(--radius-lg)] p-4">
            {children()}
        </div>
    }
}

/// Аватар: квадрат с первой буквой handle (плейсхолдер до картинок).
#[component]
pub fn Avatar(#[prop(into)] handle: String) -> impl IntoView {
    let initial = handle
        .chars()
        .next()
        .map(|c| c.to_ascii_uppercase().to_string())
        .unwrap_or_default();
    view! {
        <div class="w-10 h-10 shrink-0 flex items-center justify-center bg-[var(--accent)] \
                    text-[var(--accent-contrast)] rounded-[var(--radius)] font-bold">
            {initial}
        </div>
    }
}

/// Элемент ленты: аватар + автор + текст поста.
#[component]
pub fn FeedItem(#[prop(into)] author_handle: String, #[prop(into)] body: String) -> impl IntoView {
    let handle = author_handle.clone();
    view! {
        <article class="flex gap-3 p-3 border-b border-[var(--border)]">
            <Avatar handle=handle />
            <div class="flex-1 min-w-0">
                <div class="font-semibold text-[var(--text)]">"@"{author_handle}</div>
                <p class="text-[var(--text)] whitespace-pre-wrap break-words">{body}</p>
            </div>
        </article>
    }
}

/// Верхняя навигация: бренд слева, слот действий справа (ссылки, переключатель темы).
#[component]
pub fn Nav(children: Children) -> impl IntoView {
    view! {
        <nav class="flex items-center justify-between px-4 h-14 bg-[var(--bg-alt)] \
                    border-b border-[var(--border)]">
            <span class="font-bold tracking-tight text-[var(--text)] text-lg">"babangida"</span>
            <div class="flex items-center gap-3">{children()}</div>
        </nav>
    }
}
