//! Leptos SSR-приложение babangida: оболочка, роуты, экраны (лента, профиль,
//! инвайт-регистрация), переключатель темы. UI — из `uikit`; данные — из HTTP API
//! через server-функции (ADR-0006/0008).

use babangida_uikit::{Avatar, Button, ButtonVariant, Card, FeedItem, Nav};
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

/// Корневой компонент: тема, навигация, роуты.
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
                <A href="/join">"вступить"</A>
                <button
                    type="button"
                    class="px-3 py-1 rounded-[var(--radius)] border border-[var(--border)] text-[var(--text)]"
                    on:click=toggle
                >
                    "тема"
                </button>
            </Nav>
            <main class="max-w-2xl mx-auto">
                <Routes fallback=|| view! { <p class="p-4">"не найдено"</p> }>
                    <Route path=path!("/") view=FeedPage />
                    <Route path=path!("/u/:handle") view=ProfilePage />
                    <Route path=path!("/join") view=JoinPage />
                </Routes>
            </main>
        </Router>
    }
}

// --- DTO ответов API (зеркалят JSON `api`) ---

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

#[cfg(feature = "ssr")]
fn api_base() -> String {
    std::env::var("API_BASE").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string())
}

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

#[server]
async fn register(
    code: String,
    handle: String,
    display_name: String,
    subculture: String,
) -> Result<(), ServerFnError> {
    let resp = reqwest::Client::new()
        .post(format!("{}/register", api_base()))
        .json(&serde_json::json!({
            "code": code,
            "handle": handle,
            "display_name": display_name,
            "subculture": subculture,
        }))
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(ServerFnError::new(
            "регистрация не удалась (проверь код инвайта и handle)".to_string(),
        ))
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
                                                {p.verified.then_some(" · verified")}
                                            </div>
                                        </div>
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
fn JoinPage() -> impl IntoView {
    // ActionForm шлёт данные в server-функцию `register` (та делает JSON POST в API).
    // Работает и без JS (прогрессивное улучшение).
    let action = ServerAction::<Register>::new();
    let result = action.value();
    view! {
        <div class="p-4">
            <Card>
                <h1 class="text-lg font-bold mb-3">"вступить по инвайту"</h1>
                <ActionForm action=action attr:class="flex flex-col gap-2">
                    <input name="code" placeholder="код инвайта" class=FIELD />
                    <input name="handle" placeholder="@handle" class=FIELD />
                    <input name="display_name" placeholder="имя" class=FIELD />
                    <input
                        name="subculture"
                        placeholder="underground / casual / skin / hiphop"
                        class=FIELD
                    />
                    <Button submit=true variant=ButtonVariant::Primary>
                        "зарегистрироваться"
                    </Button>
                </ActionForm>
                {move || match result.get() {
                    Some(Ok(())) => {
                        view! { <p class="mt-2 text-[var(--accent)]">"готово — теперь войди"</p> }
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

const FIELD: &str = "px-3 py-2 bg-[var(--surface-raised)] border border-[var(--border)] \
                     rounded-[var(--radius)] text-[var(--text)] placeholder:text-[var(--text-muted)]";
