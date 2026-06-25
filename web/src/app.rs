//! Leptos SSR-приложение babangida: оболочка, роуты, экраны (лента, профиль,
//! вход/регистрация, маркет), переключатель темы, текущий юзер в нав. UI — из
//! `uikit`; данные — из HTTP API через server-функции (ADR-0006/0008).
//!
//! Сессия (ADR-0013): API кладёт токен в HttpOnly-куку `session`. server-функции
//! на стороне Leptos-сервера достают её из входящего запроса и шлют к API как
//! `Authorization: Bearer` (Bearer-форвард). На логине `Set-Cookie` от API
//! пробрасывается браузеру через `ResponseOptions`.

use babangida_uikit::{
    Avatar, Badge, Button, ButtonVariant, Card, FeedItem, Field, ListingCard, Nav,
};
use leptos::prelude::*;
use leptos_meta::{Html, MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::components::{A, Route, Router, Routes};
use leptos_router::hooks::use_params_map;
use leptos_router::path;
use serde::{Deserialize, Serialize};

/// Оболочка HTML-документа (SSR + скрипты гидратации).
pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="ru">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <AutoReload options=options.clone() />
                <HydrationScripts options />
                <MetaTags />
                <Stylesheet id="leptos" href="/pkg/babangida_web.css" />
                <Title text="babangida" />
            </head>
            <body>
                <App />
            </body>
        </html>
    }
}

/// Корневой компонент: тема, навигация (с текущим юзером), роуты.
#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();
    let (theme, set_theme) = signal("dark".to_string());
    let toggle = move |_| {
        set_theme.update(|t| {
            *t = if t == "dark" {
                "light".into()
            } else {
                "dark".into()
            }
        });
    };

    view! {
        // Реактивно ставим data-theme на <html> (SSR + гидратация).
        <Html attr:data-theme=move || theme.get() />
        <Router>
            <Nav>
                <A href="/">"лента"</A>
                <A href="/market">"маркет"</A>
                <A href="/join">"вступить"</A>
                <button
                    type="button"
                    class="px-3 py-1 rounded-[var(--radius)] border border-[var(--border)] text-[var(--text)]"
                    on:click=toggle
                >
                    "тема"
                </button>
                <UserMenu />
            </Nav>
            <main class="max-w-2xl mx-auto">
                <Routes fallback=|| view! { <p class="p-4">"не найдено"</p> }>
                    <Route path=path!("/") view=FeedPage />
                    <Route path=path!("/market") view=MarketPage />
                    <Route path=path!("/u/:handle") view=ProfilePage />
                    <Route path=path!("/login") view=LoginPage />
                    <Route path=path!("/join") view=JoinPage />
                </Routes>
            </main>
        </Router>
    }
}

// --- DTO ответов API (зеркалят JSON `api`; лишние поля serde игнорирует) ---

#[derive(Clone, Serialize, Deserialize)]
pub struct FeedItemDto {
    author_handle: String,
    body: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ProfileDto {
    handle: String,
    display_name: String,
    subculture: String,
    bio: Option<String>,
    verified: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MeDto {
    handle: String,
    verified: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ListingDto {
    title: String,
    seller_handle: String,
    price: u64,
    status: String,
}

#[cfg(feature = "ssr")]
fn api_base() -> String {
    std::env::var("API_BASE").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string())
}

/// Токен сессии из куки `session` входящего запроса к Leptos-серверу (Bearer-форвард).
#[cfg(feature = "ssr")]
async fn session_token() -> Option<String> {
    use axum::http::HeaderMap;
    use axum::http::header::COOKIE;
    let headers = leptos_axum::extract::<HeaderMap>().await.ok()?;
    let raw = headers.get(COOKIE)?.to_str().ok()?;
    raw.split(';')
        .find_map(|kv| kv.trim().strip_prefix("session=").map(str::to_owned))
}

/// Переложить `Set-Cookie` из ответа API в ответ Leptos (логин/логаут).
#[cfg(feature = "ssr")]
fn forward_set_cookie(resp: &reqwest::Response) {
    if let Some(cookie) = resp
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        && let Ok(value) = axum::http::HeaderValue::from_str(cookie)
    {
        expect_context::<leptos_axum::ResponseOptions>()
            .insert_header(axum::http::header::SET_COOKIE, value);
    }
}

// --- server-функции ---

#[server]
async fn fetch_feed() -> Result<Vec<FeedItemDto>, ServerFnError> {
    let items = reqwest::get(format!("{}/feed", api_base()))
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .json::<Vec<FeedItemDto>>()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    Ok(items)
}

#[server]
async fn fetch_profile(handle: String) -> Result<ProfileDto, ServerFnError> {
    let resp = reqwest::get(format!("{}/profiles/{handle}", api_base()))
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(ServerFnError::new("профиль не найден".to_string()));
    }
    resp.json::<ProfileDto>()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Текущий юзер по сессии (`None` — гость).
#[server]
async fn fetch_me() -> Result<Option<MeDto>, ServerFnError> {
    let Some(token) = session_token().await else {
        return Ok(None);
    };
    let resp = reqwest::Client::new()
        .get(format!("{}/me", api_base()))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let me = resp
        .json::<MeDto>()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    Ok(Some(me))
}

#[server]
async fn login(handle: String, password: String) -> Result<(), ServerFnError> {
    let resp = reqwest::Client::new()
        .post(format!("{}/login", api_base()))
        .json(&serde_json::json!({ "handle": handle, "password": password }))
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(ServerFnError::new("неверный handle или пароль".to_string()));
    }
    forward_set_cookie(&resp);
    Ok(())
}

#[server]
async fn logout() -> Result<(), ServerFnError> {
    let mut req = reqwest::Client::new().post(format!("{}/logout", api_base()));
    if let Some(token) = session_token().await {
        req = req.bearer_auth(token);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    forward_set_cookie(&resp);
    Ok(())
}

#[server]
async fn register(
    code: String,
    handle: String,
    display_name: String,
    subculture: String,
    password: String,
) -> Result<(), ServerFnError> {
    let resp = reqwest::Client::new()
        .post(format!("{}/register", api_base()))
        .json(&serde_json::json!({
            "code": code,
            "handle": handle,
            "display_name": display_name,
            "subculture": subculture,
            "password": password,
        }))
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(ServerFnError::new(
            "регистрация не удалась (проверь код инвайта, handle и пароль)".to_string(),
        ))
    }
}

#[server]
async fn fetch_market() -> Result<Vec<ListingDto>, ServerFnError> {
    let items = reqwest::get(format!("{}/market", api_base()))
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .json::<Vec<ListingDto>>()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    Ok(items)
}

#[server]
async fn create_listing(
    title: String,
    price: String,
    description: String,
) -> Result<(), ServerFnError> {
    let Some(token) = session_token().await else {
        return Err(ServerFnError::new("нужно войти".to_string()));
    };
    let price: u64 = price
        .trim()
        .parse()
        .map_err(|_| ServerFnError::new("цена — целое число рублей".to_string()))?;
    let description = {
        let d = description.trim();
        (!d.is_empty()).then(|| d.to_owned())
    };
    let resp = reqwest::Client::new()
        .post(format!("{}/listings", api_base()))
        .bearer_auth(token)
        .json(&serde_json::json!({ "title": title, "price": price, "description": description }))
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    match resp.status().as_u16() {
        200..=299 => Ok(()),
        401 => Err(ServerFnError::new(
            "сессия истекла — войди заново".to_string(),
        )),
        403 => Err(ServerFnError::new(
            "продавать может только верифицированный — нужна верификация".to_string(),
        )),
        422 => Err(ServerFnError::new("проверь заголовок и цену".to_string())),
        _ => Err(ServerFnError::new("не удалось выставить товар".to_string())),
    }
}

// --- нав: текущий юзер ---

#[component]
fn UserMenu() -> impl IntoView {
    let logout = ServerAction::<Logout>::new();
    // Перечитываем текущего юзера после логаута.
    let me = Resource::new(move || logout.version().get(), |_| fetch_me());
    view! {
        <Suspense fallback=move || {
            view! { <span class="text-sm text-[var(--text-muted)]">"…"</span> }
        }>
            {move || Suspend::new(async move {
                match me.await {
                    Ok(Some(u)) => {
                        view! {
                            <span class="flex items-center gap-2 text-sm text-[var(--text)]">
                                "@"{u.handle.clone()}
                                {u.verified.then(|| view! { <Badge accent=true>"verified"</Badge> })}
                                <ActionForm action=logout attr:class="inline">
                                    <button
                                        type="submit"
                                        class="text-[var(--text-muted)] hover:text-[var(--text)]"
                                    >
                                        "выйти"
                                    </button>
                                </ActionForm>
                            </span>
                        }
                            .into_any()
                    }
                    _ => view! { <A href="/login">"войти"</A> }.into_any(),
                }
            })}
        </Suspense>
    }
}

// --- экраны ---

#[component]
fn FeedPage() -> impl IntoView {
    let feed = Resource::new(|| (), |()| fetch_feed());
    view! {
        <Suspense fallback=move || {
            view! { <p class="p-4 text-[var(--text-muted)]">"загрузка ленты…"</p> }
        }>
            {move || Suspend::new(async move {
                match feed.await {
                    Ok(items) if items.is_empty() => {
                        view! { <p class="p-4 text-[var(--text-muted)]">"пока пусто"</p> }.into_any()
                    }
                    Ok(items) => {
                        view! {
                            <div>
                                {items
                                    .into_iter()
                                    .map(|i| {
                                        view! { <FeedItem author_handle=i.author_handle body=i.body /> }
                                    })
                                    .collect_view()}
                            </div>
                        }
                            .into_any()
                    }
                    Err(_) => {
                        view! {
                            <p class="p-4 text-[var(--danger)]">"не удалось загрузить ленту"</p>
                        }
                            .into_any()
                    }
                }
            })}
        </Suspense>
    }
}

#[component]
fn MarketPage() -> impl IntoView {
    let create = ServerAction::<CreateListing>::new();
    let created = create.value();
    // Перечитываем маркет после успешного выставления.
    let market = Resource::new(move || create.version().get(), |_| fetch_market());
    view! {
        <div class="p-4 flex flex-col gap-4">
            <Card>
                <h1 class="text-lg font-bold mb-3">"выставить товар"</h1>
                <ActionForm action=create attr:class="flex flex-col gap-3">
                    <Field name="title" label="что продаёшь" placeholder="MPC 2000XL" />
                    <Field name="price" label="цена, ₽" placeholder="45000" />
                    <Field
                        name="description"
                        label="описание (необязательно)"
                        placeholder="состояние, детали"
                    />
                    <Button submit=true variant=ButtonVariant::Primary>
                        "выставить"
                    </Button>
                </ActionForm>
                {move || match created.get() {
                    Some(Ok(())) => {
                        view! { <p class="mt-2 text-[var(--accent)]">"товар выставлен"</p> }
                            .into_any()
                    }
                    Some(Err(e)) => {
                        view! { <p class="mt-2 text-[var(--danger)]">{e.to_string()}</p> }.into_any()
                    }
                    None => ().into_any(),
                }}
            </Card>
            <Suspense fallback=move || {
                view! { <p class="text-[var(--text-muted)]">"загрузка маркета…"</p> }
            }>
                {move || Suspend::new(async move {
                    match market.await {
                        Ok(items) if items.is_empty() => {
                            view! {
                                <p class="text-[var(--text-muted)]">"на барахолке пока пусто"</p>
                            }
                                .into_any()
                        }
                        Ok(items) => {
                            view! {
                                <div class="flex flex-col gap-3">
                                    {items
                                        .into_iter()
                                        .map(|l| {
                                            view! {
                                                <ListingCard
                                                    title=l.title
                                                    seller_handle=l.seller_handle
                                                    price=l.price
                                                    status=l.status
                                                />
                                            }
                                        })
                                        .collect_view()}
                                </div>
                            }
                                .into_any()
                        }
                        Err(_) => {
                            view! {
                                <p class="text-[var(--danger)]">"не удалось загрузить маркет"</p>
                            }
                                .into_any()
                        }
                    }
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn ProfilePage() -> impl IntoView {
    let params = use_params_map();
    let handle = move || params.read().get("handle").unwrap_or_default();
    let profile = Resource::new(handle, fetch_profile);
    view! {
        <div class="p-4">
            <Suspense fallback=move || view! { <p class="text-[var(--text-muted)]">"загрузка…"</p> }>
                {move || Suspend::new(async move {
                    match profile.await {
                        Ok(p) => {
                            view! {
                                <Card>
                                    <div class="flex items-center gap-3">
                                        <Avatar handle=p.handle.clone() />
                                        <div>
                                            <div class="text-lg font-bold">{p.display_name}</div>
                                            <div class="text-[var(--text-muted)]">
                                                "@"{p.handle} " · " {p.subculture}
                                            </div>
                                        </div>
                                        {p
                                            .verified
                                            .then(|| view! { <Badge accent=true>"verified"</Badge> })}
                                    </div>
                                    {p.bio.map(|b| view! { <p class="mt-3">{b}</p> })}
                                </Card>
                            }
                                .into_any()
                        }
                        Err(_) => {
                            view! { <p class="text-[var(--danger)]">"профиль не найден"</p> }
                                .into_any()
                        }
                    }
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn LoginPage() -> impl IntoView {
    let action = ServerAction::<Login>::new();
    let result = action.value();
    view! {
        <div class="p-4">
            <Card>
                <h1 class="text-lg font-bold mb-3">"вход"</h1>
                <ActionForm action=action attr:class="flex flex-col gap-3">
                    <Field name="handle" label="@handle" placeholder="handle" />
                    <Field name="password" label="пароль" kind="password" placeholder="пароль" />
                    <Button submit=true variant=ButtonVariant::Primary>
                        "войти"
                    </Button>
                </ActionForm>
                {move || match result.get() {
                    Some(Ok(())) => {
                        view! {
                            <p class="mt-2 text-[var(--accent)]">
                                "вошёл. " <A href="/">"на главную"</A>
                            </p>
                        }
                            .into_any()
                    }
                    Some(Err(e)) => {
                        view! { <p class="mt-2 text-[var(--danger)]">{e.to_string()}</p> }.into_any()
                    }
                    None => ().into_any(),
                }}
            </Card>
        </div>
    }
}

#[component]
fn JoinPage() -> impl IntoView {
    // ActionForm шлёт данные в server-функцию `register` (та делает JSON POST в API).
    // Работает и без JS (прогрессивное улучшение).
    let action = ServerAction::<Register>::new();
    let result = action.value();
    view! {
        <div class="p-4">
            <Card>
                <h1 class="text-lg font-bold mb-3">"вступить по инвайту"</h1>
                <ActionForm action=action attr:class="flex flex-col gap-3">
                    <Field name="code" label="код инвайта" placeholder="код" />
                    <Field name="handle" label="@handle" placeholder="handle" />
                    <Field name="display_name" label="имя" placeholder="как показывать" />
                    <Field
                        name="subculture"
                        label="субкультура"
                        placeholder="underground / casual / skin / hiphop"
                    />
                    <Field name="password" label="пароль" kind="password" placeholder="пароль" />
                    <Button submit=true variant=ButtonVariant::Primary>
                        "зарегистрироваться"
                    </Button>
                </ActionForm>
                {move || match result.get() {
                    Some(Ok(())) => {
                        view! {
                            <p class="mt-2 text-[var(--accent)]">
                                "готово — теперь " <A href="/login">"войди"</A>
                            </p>
                        }
                            .into_any()
                    }
                    Some(Err(e)) => {
                        view! { <p class="mt-2 text-[var(--danger)]">{e.to_string()}</p> }.into_any()
                    }
                    None => ().into_any(),
                }}
            </Card>
        </div>
    }
}
