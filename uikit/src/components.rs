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

/// Элемент ленты: аватар + автор + текст поста. Если пост из сообщества
/// (`group_slug`/`group_name`) — чип-ссылка на группу прямо в общей ленте (анти-ВК).
#[component]
pub fn FeedItem(
    #[prop(into)] author_handle: String,
    #[prop(into)] body: String,
    #[prop(default = None)] group_slug: Option<String>,
    #[prop(default = None)] group_name: Option<String>,
) -> impl IntoView {
    let handle = author_handle.clone();
    let group = group_slug.zip(group_name).map(|(slug, name)| {
        view! {
            <a
                href=format!("/g/{slug}")
                class="inline-block mt-1 text-xs text-[var(--accent)] hover:underline"
            >
                "в сообществе "{name}
            </a>
        }
    });
    view! {
        <article class="flex gap-3 p-3 border-b border-[var(--border)]">
            <Avatar handle=handle />
            <div class="flex-1 min-w-0">
                <div class="font-semibold text-[var(--text)]">"@"{author_handle}</div>
                <p class="text-[var(--text)] whitespace-pre-wrap break-words">{body}</p>
                {group}
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

/// Поле формы: лейбл над инпутом, стиль из токенов. `kind` — тип инпута
/// (`text` по умолчанию, `password` и т.п.).
#[component]
pub fn Field(
    #[prop(into)] name: String,
    #[prop(into)] label: String,
    #[prop(optional, into)] kind: String,
    #[prop(optional, into)] placeholder: String,
) -> impl IntoView {
    let ty = if kind.is_empty() {
        "text".to_owned()
    } else {
        kind
    };
    view! {
        <label class="flex flex-col gap-1 text-sm text-[var(--text-muted)]">
            {label}
            <input
                name=name
                type=ty
                placeholder=placeholder
                class="px-3 py-2 bg-[var(--surface-raised)] border border-[var(--border)] \
                       rounded-[var(--radius)] text-[var(--text)] placeholder:text-[var(--text-muted)]"
            />
        </label>
    }
}

/// Многострочное поле — composer постов/сообщений. Аналог [`Field`], но `textarea`.
/// `rows` задаёт стартовую высоту (по умолчанию 3); поле растягивается вертикально.
#[component]
pub fn TextArea(
    #[prop(into)] name: String,
    #[prop(into)] label: String,
    #[prop(optional, into)] placeholder: String,
    #[prop(optional)] rows: u32,
) -> impl IntoView {
    let rows = if rows == 0 { 3 } else { rows };
    view! {
        <label class="flex flex-col gap-1 text-sm text-[var(--text-muted)]">
            {label}
            <textarea
                name=name
                rows=rows
                placeholder=placeholder
                class="px-3 py-2 bg-[var(--surface-raised)] border border-[var(--border)] \
                       rounded-[var(--radius)] text-[var(--text)] placeholder:text-[var(--text-muted)] \
                       resize-y"
            ></textarea>
        </label>
    }
}

/// Бейдж-чип (метка статуса/роли). `accent=true` — акцентный фон, иначе приглушённый.
#[component]
pub fn Badge(#[prop(optional)] accent: bool, children: Children) -> impl IntoView {
    let cls = if accent {
        "inline-block px-2 py-0.5 rounded-[var(--radius)] text-xs font-semibold \
         bg-[var(--accent)] text-[var(--accent-contrast)]"
    } else {
        "inline-block px-2 py-0.5 rounded-[var(--radius)] text-xs font-semibold \
         bg-[var(--surface-raised)] text-[var(--text-muted)] border border-[var(--border)]"
    };
    view! { <span class=cls>{children()}</span> }
}

/// Карточка трека: название, артист, опц. жанр, ссылка на прослушивание (внешний URL).
/// Презентационная; действия владельца (снять) — на стороне приложения.
#[component]
pub fn TrackCard(
    #[prop(into)] title: String,
    #[prop(into)] artist_handle: String,
    #[prop(default = None)] genre: Option<String>,
    #[prop(into)] audio_url: String,
) -> impl IntoView {
    view! {
        <div class="bg-[var(--surface)] border border-[var(--border)] rounded-[var(--radius-lg)] \
                    p-4 flex flex-col gap-1">
            <div class="flex items-start justify-between gap-3">
                <span class="font-semibold text-[var(--text)] break-words">{title}</span>
                <a
                    href=audio_url
                    target="_blank"
                    rel="noopener noreferrer"
                    class="font-bold text-[var(--accent)] hover:underline whitespace-nowrap"
                >
                    "слушать ▸"
                </a>
            </div>
            <div class="flex items-center gap-2 text-sm text-[var(--text-muted)]">
                "@"{artist_handle}
                {genre.map(|g| view! { <Badge>{g}</Badge> })}
            </div>
        </div>
    }
}

/// Карточка товара барахолки: заголовок, цена в рублях, продавец, статус (если не активен).
#[component]
pub fn ListingCard(
    #[prop(into)] title: String,
    #[prop(into)] seller_handle: String,
    price: u64,
    #[prop(into)] status: String,
) -> impl IntoView {
    let sold_or_withdrawn = status != "active";
    view! {
        <div class="bg-[var(--surface)] border border-[var(--border)] rounded-[var(--radius-lg)] \
                    p-4 flex flex-col gap-1">
            <div class="flex items-start justify-between gap-3">
                <span class="font-semibold text-[var(--text)] break-words">{title}</span>
                <span class="font-bold text-[var(--accent)] whitespace-nowrap">
                    {price}" ₽"
                </span>
            </div>
            <div class="flex items-center gap-2 text-sm text-[var(--text-muted)]">
                "@"{seller_handle}
                {sold_or_withdrawn
                    .then(|| view! { <Badge>{status.clone()}</Badge> })}
            </div>
        </div>
    }
}
