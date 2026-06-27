//! Экраны mobile (Dioxus 0.7 + dioxus-router): оболочка с навигацией и экраны лента,
//! вход, профиль, маркет, инбокс/тред, сообщество. Анти-ВК: всё под единой `Shell`.
//! Данные — через `client`; сессия/тема — из контекста (см. `lib.rs`).

use babangida_uikit::dx::{Badge, Button, Card, FeedItem, Field};
use babangida_uikit::theme::Mode;
use dioxus::prelude::*;

use crate::client::{
    create_post, fetch_feed, fetch_group, fetch_inbox, fetch_market, fetch_profile,
    fetch_seller_listings, fetch_thread, join_group, login, post_to_group, send_message,
};
use crate::store;
use crate::{Route, Session};

const PAGE: &str = "padding:16px;display:flex;flex-direction:column;gap:12px;";
const MUTED: &str = "padding:16px;color:var(--text-muted);";
const DANGER: &str = "color:var(--danger);font-size:14px;";
const LINK: &str = "color:var(--text-muted);text-decoration:none;";
const NAV_BTN: &str =
    "background:none;border:none;color:var(--text-muted);cursor:pointer;font-size:15px;";

/// Оболочка: фон/цвет темы, верхняя навигация, активный экран (`Outlet`).
#[component]
pub fn Shell() -> Element {
    rsx! {
        div { style: "min-height:100vh;background:var(--bg);color:var(--text);font-family:system-ui,sans-serif;",
            Nav {}
            div { style: "max-width:640px;margin:0 auto;", Outlet::<Route> {} }
        }
    }
}

#[component]
fn Nav() -> Element {
    let mut theme = use_context::<Signal<Mode>>();
    let mut session = use_context::<Signal<Session>>();
    let logged = session().token.is_some();
    rsx! {
        header { style: "display:flex;align-items:center;justify-content:space-between;height:56px;\
                         padding:0 16px;background:var(--bg-alt);border-bottom:1px solid var(--border);",
            span { style: "font-weight:700;font-size:18px;color:var(--text);", "babangida" }
            nav { style: "display:flex;align-items:center;gap:14px;font-size:15px;",
                Link { to: Route::Feed {}, style: LINK, "лента" }
                Link { to: Route::Market {}, style: LINK, "маркет" }
                Link { to: Route::Inbox {}, style: LINK, "сообщения" }
                button { style: NAV_BTN, onclick: move |_| theme.set(theme().toggled()), "тема" }
                if logged {
                    button {
                        style: NAV_BTN,
                        onclick: move |_| {
                            store::clear();
                            session.set(Session::default());
                        },
                        "выйти"
                    }
                } else {
                    Link { to: Route::Login {}, style: LINK, "войти" }
                }
            }
        }
    }
}

/// Лента: реальные посты (`GET /feed`, viewer-aware под токеном) + composer для
/// залогиненных. Пост сообщества несёт ссылку-чип на группу (анти-ВК).
#[component]
pub fn Feed() -> Element {
    let session = use_context::<Signal<Session>>();
    let mut feed = use_resource(move || async move { fetch_feed(session().token).await });
    let mut body = use_signal(String::new);
    let mut error = use_signal(|| Option::<String>::None);
    let logged = session().token.is_some();

    let submit = move |_| {
        let Some(tok) = session().token else {
            return;
        };
        let text = body();
        spawn(async move {
            match create_post(tok, text).await {
                Ok(()) => {
                    body.set(String::new());
                    error.set(None);
                    feed.restart();
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };

    rsx! {
        if logged {
            div { style: "padding:16px;border-bottom:1px solid var(--border);display:flex;flex-direction:column;gap:8px;",
                Field { label: "что нового", value: body, placeholder: "напиши в ленту" }
                Button { onclick: submit, "опубликовать" }
                if let Some(e) = error() {
                    p { style: DANGER, "{e}" }
                }
            }
        } else {
            p { style: "padding:16px;color:var(--text-muted);font-size:14px;",
                Link { to: Route::Login {}, style: "color:var(--accent);", "войди" }
                ", чтобы постить"
            }
        }
        {
            match &*feed.read_unchecked() {
                Some(Ok(items)) if items.is_empty() => rsx! {
                    p { style: MUTED, "пока пусто" }
                },
                Some(Ok(items)) => {
                    let items = items.clone();
                    rsx! {
                        for it in items {
                            div { style: "border-bottom:1px solid var(--border);",
                                FeedItem { author_handle: it.author_handle.clone(), body: it.body.clone() }
                                if let (Some(slug), Some(name)) = (it.group_slug.clone(), it.group_name.clone()) {
                                    div { style: "padding:0 12px 10px 64px;",
                                        Link {
                                            to: Route::Group { slug },
                                            style: "font-size:12px;color:var(--accent);text-decoration:none;",
                                            "в сообществе {name}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Some(Err(e)) => rsx! {
                    p { style: "padding:16px;color:var(--danger);", "не удалось загрузить ленту: {e}" }
                },
                None => rsx! {
                    p { style: MUTED, "загрузка ленты…" }
                },
            }
        }
    }
}

/// Вход: handle/пароль → токен в секьюр-стор + сессию, затем на ленту.
#[component]
pub fn Login() -> Element {
    let mut session = use_context::<Signal<Session>>();
    let nav = use_navigator();
    let handle = use_signal(String::new);
    let password = use_signal(String::new);
    let mut error = use_signal(|| Option::<String>::None);

    let submit = move |_| {
        let (h, p) = (handle(), password());
        spawn(async move {
            match login(h.clone(), p).await {
                Ok(token) => {
                    store::save(&token);
                    session.set(Session {
                        token: Some(token),
                        handle: Some(h),
                    });
                    error.set(None);
                    nav.push(Route::Feed {});
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };

    rsx! {
        div { style: PAGE,
            h1 { style: "font-size:18px;font-weight:700;", "вход" }
            Field { label: "@handle", value: handle, placeholder: "handle" }
            Field { label: "пароль", value: password, password: true, placeholder: "пароль" }
            Button { onclick: submit, "войти" }
            if let Some(e) = error() {
                p { style: DANGER, "{e}" }
            }
        }
    }
}

/// Профиль: карточка + товары продавца + «написать» (если залогинен и не свой профиль).
#[component]
pub fn Profile(handle: String) -> Element {
    let session = use_context::<Signal<Session>>();
    let profile = use_resource({
        let h = handle.clone();
        move || {
            let h = h.clone();
            async move { fetch_profile(h).await }
        }
    });
    let listings = use_resource({
        let h = handle.clone();
        move || {
            let h = h.clone();
            async move { fetch_seller_listings(h).await }
        }
    });

    let me = session().handle;
    let can_message = session().token.is_some() && me.as_deref() != Some(handle.as_str());
    let mut body = use_signal(String::new);
    let mut error = use_signal(|| Option::<String>::None);
    let send = {
        let target = handle.clone();
        move |_| {
            let Some(tok) = session().token else {
                return;
            };
            let (recipient, text) = (target.clone(), body());
            spawn(async move {
                match send_message(tok, recipient, text).await {
                    Ok(()) => {
                        body.set(String::new());
                        error.set(None);
                    }
                    Err(e) => error.set(Some(e)),
                }
            });
        }
    };

    rsx! {
        div { style: PAGE,
            {
                match &*profile.read_unchecked() {
                    Some(Ok(p)) => {
                        let p = p.clone();
                        rsx! {
                            Card {
                                div { style: "display:flex;align-items:center;justify-content:space-between;gap:12px;",
                                    div {
                                        div { style: "font-size:18px;font-weight:700;color:var(--text);", "{p.display_name}" }
                                        div { style: "color:var(--text-muted);font-size:14px;", "@{p.handle} · {p.subculture}" }
                                    }
                                    if p.verified {
                                        Badge { "verified" }
                                    }
                                }
                                if let Some(bio) = p.bio.clone() {
                                    p { style: "margin:8px 0 0;color:var(--text);", "{bio}" }
                                }
                            }
                        }
                    }
                    Some(Err(e)) => rsx! {
                        p { style: "color:var(--danger);", "{e}" }
                    },
                    None => rsx! {
                        p { style: "color:var(--text-muted);", "загрузка…" }
                    },
                }
            }
            if can_message {
                Card {
                    Field { label: "написать @{handle}", value: body, placeholder: "сообщение" }
                    Button { onclick: send, "отправить" }
                    if let Some(e) = error() {
                        p { style: DANGER, "{e}" }
                    }
                }
            }
            {
                match &*listings.read_unchecked() {
                    Some(Ok(items)) if !items.is_empty() => {
                        let items = items.clone();
                        rsx! {
                            h2 { style: "font-size:15px;font-weight:700;margin-top:8px;", "товары" }
                            for it in items {
                                ListingRow { title: it.title.clone(), seller_handle: it.seller_handle.clone(), price: it.price, status: it.status.clone() }
                            }
                        }
                    }
                    _ => rsx! {},
                }
            }
        }
    }
}

/// Маркет: активные товары; продавец — ссылка на профиль.
#[component]
pub fn Market() -> Element {
    let market = use_resource(|| async move { fetch_market().await });
    rsx! {
        div { style: PAGE,
            h1 { style: "font-size:18px;font-weight:700;", "маркет" }
            {
                match &*market.read_unchecked() {
                    Some(Ok(items)) if items.is_empty() => rsx! {
                        p { style: "color:var(--text-muted);", "на барахолке пусто" }
                    },
                    Some(Ok(items)) => {
                        let items = items.clone();
                        rsx! {
                            for it in items {
                                ListingRow { title: it.title.clone(), seller_handle: it.seller_handle.clone(), price: it.price, status: it.status.clone() }
                            }
                        }
                    }
                    Some(Err(e)) => rsx! {
                        p { style: "color:var(--danger);", "не удалось загрузить маркет: {e}" }
                    },
                    None => rsx! {
                        p { style: "color:var(--text-muted);", "загрузка…" }
                    },
                }
            }
        }
    }
}

/// Карточка товара: заголовок, цена, продавец-ссылка, статус (если не активен).
#[component]
fn ListingRow(title: String, seller_handle: String, price: u64, status: String) -> Element {
    rsx! {
        Card {
            div { style: "display:flex;justify-content:space-between;gap:12px;",
                span { style: "font-weight:600;color:var(--text);", "{title}" }
                span { style: "font-weight:700;color:var(--accent);white-space:nowrap;", "{price} ₽" }
            }
            div { style: "display:flex;align-items:center;gap:8px;font-size:14px;color:var(--text-muted);margin-top:4px;",
                Link { to: Route::Profile { handle: seller_handle.clone() }, style: LINK, "@{seller_handle}" }
                if status != "active" {
                    Badge { "{status}" }
                }
            }
        }
    }
}

/// Инбокс: переписки текущего юзера; тап — в тред (с собеседником в `?with`).
#[component]
pub fn Inbox() -> Element {
    let session = use_context::<Signal<Session>>();
    let inbox = use_resource(move || async move {
        match session().token {
            Some(t) => fetch_inbox(t).await,
            None => Ok(Vec::new()),
        }
    });
    let logged = session().token.is_some();
    rsx! {
        div { style: PAGE,
            h1 { style: "font-size:18px;font-weight:700;", "сообщения" }
            if !logged {
                p { style: "color:var(--text-muted);font-size:14px;",
                    Link { to: Route::Login {}, style: "color:var(--accent);", "войди" }
                    ", чтобы читать сообщения"
                }
            } else {
                {
                    match &*inbox.read_unchecked() {
                        Some(Ok(items)) if items.is_empty() => rsx! {
                            p { style: "color:var(--text-muted);", "переписок пока нет" }
                        },
                        Some(Ok(items)) => {
                            let items = items.clone();
                            rsx! {
                                for c in items {
                                    Link {
                                        to: Route::Thread { id: c.conversation_id.clone(), with: c.counterpart_handle.clone() },
                                        style: "text-decoration:none;",
                                        Card {
                                            div { style: "font-weight:600;color:var(--text);", "@{c.counterpart_handle}" }
                                            div { style: "color:var(--text-muted);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;", "{c.last_message}" }
                                        }
                                    }
                                }
                            }
                        }
                        Some(Err(e)) => rsx! {
                            p { style: "color:var(--danger);", "не удалось загрузить инбокс: {e}" }
                        },
                        None => rsx! {
                            p { style: "color:var(--text-muted);", "загрузка…" }
                        },
                    }
                }
            }
        }
    }
}

/// Тред: сообщения диалога + ответ (получатель — из `?with`).
#[component]
pub fn Thread(id: String, with: String) -> Element {
    let session = use_context::<Signal<Session>>();
    let mut thread = use_resource({
        let id = id.clone();
        move || {
            let (t, i) = (session().token, id.clone());
            async move {
                match t {
                    Some(tok) => fetch_thread(tok, i).await,
                    None => Ok(Vec::new()),
                }
            }
        }
    });
    let mut body = use_signal(String::new);
    let mut error = use_signal(|| Option::<String>::None);
    let send = {
        let with = with.clone();
        move |_| {
            let Some(tok) = session().token else {
                return;
            };
            let (recipient, text) = (with.clone(), body());
            spawn(async move {
                match send_message(tok, recipient, text).await {
                    Ok(()) => {
                        body.set(String::new());
                        error.set(None);
                        thread.restart();
                    }
                    Err(e) => error.set(Some(e)),
                }
            });
        }
    };

    rsx! {
        div { style: "padding:16px;display:flex;flex-direction:column;gap:8px;",
            Link { to: Route::Inbox {}, style: LINK, "← к сообщениям" }
            {
                match &*thread.read_unchecked() {
                    Some(Ok(msgs)) if msgs.is_empty() => rsx! {
                        p { style: "color:var(--text-muted);", "пока пусто" }
                    },
                    Some(Ok(msgs)) => {
                        let msgs = msgs.clone();
                        rsx! {
                            for m in msgs {
                                Card {
                                    div { style: "font-size:13px;color:var(--text-muted);", "@{m.author_handle}" }
                                    p { style: "margin:0;color:var(--text);white-space:pre-wrap;word-break:break-word;", "{m.body}" }
                                }
                            }
                        }
                    }
                    Some(Err(e)) => rsx! {
                        p { style: "color:var(--danger);", "{e}" }
                    },
                    None => rsx! {
                        p { style: "color:var(--text-muted);", "загрузка…" }
                    },
                }
            }
            if !with.is_empty() {
                Field { label: "ответить", value: body, placeholder: "сообщение" }
                Button { onclick: send, "отправить" }
                if let Some(e) = error() {
                    p { style: DANGER, "{e}" }
                }
            }
        }
    }
}

/// Сообщество: карточка + вступление и пост (для залогиненных). Вход — по чипу из ленты.
#[component]
pub fn Group(slug: String) -> Element {
    let session = use_context::<Signal<Session>>();
    let mut group = use_resource({
        let slug = slug.clone();
        move || {
            let s = slug.clone();
            async move { fetch_group(s).await }
        }
    });
    let mut body = use_signal(String::new);
    let mut error = use_signal(|| Option::<String>::None);
    let logged = session().token.is_some();

    rsx! {
        div { style: PAGE,
            {
                match &*group.read_unchecked() {
                    Some(Ok(g)) => {
                        let gid_join = g.group_id.clone();
                        let gid_post = g.group_id.clone();
                        let join = move |_| {
                            let Some(tok) = session().token else {
                                return;
                            };
                            let gid = gid_join.clone();
                            spawn(async move {
                                let _ = join_group(tok, gid).await;
                                group.restart();
                            });
                        };
                        let post = move |_| {
                            let Some(tok) = session().token else {
                                return;
                            };
                            let (gid, text) = (gid_post.clone(), body());
                            spawn(async move {
                                match post_to_group(tok, gid, text).await {
                                    Ok(()) => {
                                        body.set(String::new());
                                        error.set(None);
                                    }
                                    Err(e) => error.set(Some(e)),
                                }
                            });
                        };
                        rsx! {
                            Card {
                                div { style: "display:flex;align-items:center;justify-content:space-between;gap:12px;",
                                    div {
                                        div { style: "font-size:18px;font-weight:700;color:var(--text);", "{g.name}" }
                                        div { style: "color:var(--text-muted);font-size:14px;", "/g/{g.slug} · {g.member_count} участн." }
                                    }
                                    Badge { "{g.kind}" }
                                }
                            }
                            if logged {
                                Button { onclick: join, "вступить" }
                                Card {
                                    Field { label: "написать в сообщество", value: body, placeholder: "пост увидят в ленте" }
                                    Button { onclick: post, "опубликовать" }
                                    if let Some(e) = error() {
                                        p { style: DANGER, "{e}" }
                                    }
                                }
                            } else {
                                p { style: "color:var(--text-muted);font-size:14px;",
                                    Link { to: Route::Login {}, style: "color:var(--accent);", "войди" }
                                    ", чтобы вступить и постить"
                                }
                            }
                        }
                    }
                    Some(Err(e)) => rsx! {
                        p { style: "color:var(--danger);", "{e}" }
                    },
                    None => rsx! {
                        p { style: "color:var(--text-muted);", "загрузка…" }
                    },
                }
            }
        }
    }
}
