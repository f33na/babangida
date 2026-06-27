//! Leptos SSR-приложение babangida: оболочка, роуты, экраны (лента, профиль,
//! вход/регистрация, маркет), переключатель темы, текущий юзер в нав. UI — из
//! `uikit`; данные — из HTTP API через server-функции (ADR-0006/0008).
//!
//! Сессия (ADR-0013): API кладёт токен в HttpOnly-куку `session`. server-функции
//! на стороне Leptos-сервера достают её из входящего запроса и шлют к API как
//! `Authorization: Bearer` (Bearer-форвард). На логине `Set-Cookie` от API
//! пробрасывается браузеру через `ResponseOptions`.

use babangida_uikit::{
    Avatar, Badge, Button, ButtonVariant, Card, FeedItem, Field, ListingCard, Nav, TextArea,
    TrackCard,
};
use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::components::{A, Route, Router, Routes};
use leptos_router::hooks::{use_params_map, use_query_map};
use leptos_router::path;
use serde::{Deserialize, Serialize};

/// Оболочка HTML-документа (SSR + скрипты гидратации).
pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        // data-theme=dark — дефолт для no-JS; скрипт ниже до отрисовки подменяет его
        // на сохранённую в куке тему (без вспышки), и держит переключатель темы.
        <html lang="ru" data-theme="dark">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <script>
                    "(function(){var m=document.cookie.match(/(?:^|; )theme=([^;]*)/);document.documentElement.dataset.theme=m?m[1]:'dark';window.__toggleTheme=function(){var c=document.documentElement.dataset.theme==='dark'?'light':'dark';document.documentElement.dataset.theme=c;document.cookie='theme='+c+'; path=/; max-age=31536000; samesite=lax';};})();"
                </script>
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

/// Переключить тему (dark↔light): зовёт глобальный `__toggleTheme` из инлайн-скрипта
/// в `shell` (он же пишет куку). На сервере — no-op (выполняется только в браузере).
fn toggle_theme() {
    #[cfg(feature = "hydrate")]
    toggle_theme_js();
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = __toggleTheme)]
    fn toggle_theme_js();
}

/// Корневой компонент: тема, навигация (с текущим юзером), роуты.
#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();
    view! {
        <Router>
            <Nav>
                <A href="/">"лента"</A>
                <A href="/music">"музыка"</A>
                <A href="/market">"маркет"</A>
                <A href="/messages">"сообщения"</A>
                <A href="/verification">"верификация"</A>
                <A href="/join">"вступить"</A>
                <button
                    type="button"
                    class="px-3 py-1 rounded-[var(--radius)] border border-[var(--border)] text-[var(--text)]"
                    on:click=move |_| toggle_theme()
                >
                    "тема"
                </button>
                <UserMenu />
            </Nav>
            <main class="max-w-2xl mx-auto">
                <Routes fallback=|| view! { <p class="p-4">"не найдено"</p> }>
                    <Route path=path!("/") view=FeedPage />
                    <Route path=path!("/music") view=MusicPage />
                    <Route path=path!("/market") view=MarketPage />
                    <Route path=path!("/u/:handle") view=ProfilePage />
                    <Route path=path!("/g/:slug") view=GroupPage />
                    <Route path=path!("/messages") view=InboxPage />
                    <Route path=path!("/messages/:id") view=ThreadPage />
                    <Route path=path!("/verification") view=VerificationPage />
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
    /// Если пост из сообщества — слаг/имя группы (чип в общей ленте, анти-ВК).
    #[serde(default)]
    group_slug: Option<String>,
    #[serde(default)]
    group_name: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ProfileDto {
    user_id: String,
    handle: String,
    display_name: String,
    subculture: String,
    bio: Option<String>,
    verified: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ConversationDto {
    conversation_id: String,
    counterpart_handle: String,
    last_message: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MessageDto {
    author_handle: String,
    body: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MeDto {
    handle: String,
    verified: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ListingDto {
    listing_id: String,
    title: String,
    seller_handle: String,
    price: u64,
    status: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GroupDto {
    group_id: String,
    slug: String,
    name: String,
    kind: String,
    member_count: u32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MyVerificationDto {
    status: String,
    #[serde(default)]
    decision_reason: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct VerificationQueueItemDto {
    request_id: String,
    requester_handle: String,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TrackDto {
    track_id: String,
    title: String,
    artist_handle: String,
    audio_url: String,
    #[serde(default)]
    genre: Option<String>,
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

/// Опубликовать пост в общую ленту (от текущего юзера, Bearer-форвард).
#[server]
async fn create_post(body: String) -> Result<(), ServerFnError> {
    let Some(token) = session_token().await else {
        return Err(ServerFnError::new("нужно войти".to_string()));
    };
    let resp = reqwest::Client::new()
        .post(format!("{}/posts", api_base()))
        .bearer_auth(token)
        .json(&serde_json::json!({ "body": body }))
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    match resp.status().as_u16() {
        200..=299 => Ok(()),
        401 => Err(ServerFnError::new(
            "сессия истекла — войди заново".to_string(),
        )),
        422 => Err(ServerFnError::new("пост не может быть пустым".to_string())),
        _ => Err(ServerFnError::new("не удалось опубликовать".to_string())),
    }
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

/// Товары продавца (публичное чтение): для профиля.
#[server]
async fn fetch_seller_listings(handle: String) -> Result<Vec<ListingDto>, ServerFnError> {
    let resp = reqwest::get(format!("{}/profiles/{handle}/listings", api_base()))
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    if !resp.status().is_success() {
        return Ok(Vec::new());
    }
    resp.json::<Vec<ListingDto>>()
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

/// Отметить свой товар проданным (Bearer; только продавец).
#[server]
async fn mark_sold(listing_id: String) -> Result<(), ServerFnError> {
    listing_action(&listing_id, "sold").await
}

/// Снять свой товар с продажи (Bearer; только продавец).
#[server]
async fn withdraw(listing_id: String) -> Result<(), ServerFnError> {
    listing_action(&listing_id, "withdraw").await
}

/// Общий POST по товару текущего юзера: `sold` | `withdraw`.
#[cfg(feature = "ssr")]
async fn listing_action(listing_id: &str, action: &str) -> Result<(), ServerFnError> {
    let Some(token) = session_token().await else {
        return Err(ServerFnError::new("нужно войти".to_string()));
    };
    let resp = reqwest::Client::new()
        .post(format!("{}/listings/{listing_id}/{action}", api_base()))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    match resp.status().as_u16() {
        200..=299 => Ok(()),
        401 => Err(ServerFnError::new(
            "сессия истекла — войди заново".to_string(),
        )),
        403 => Err(ServerFnError::new("это не твой товар".to_string())),
        409 => Err(ServerFnError::new("товар уже неактивен".to_string())),
        _ => Err(ServerFnError::new("не удалось обновить товар".to_string())),
    }
}

// --- сообщества (анти-ВК: группа — срез общей ленты, не отдельное приложение) ---

/// Карточка сообщества по слагу (публичное чтение).
#[server]
async fn fetch_group(slug: String) -> Result<GroupDto, ServerFnError> {
    let resp = reqwest::get(format!("{}/groups/{slug}", api_base()))
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(ServerFnError::new("сообщество не найдено".to_string()));
    }
    resp.json::<GroupDto>()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Вступить в сообщество (Bearer-форвард).
#[server]
async fn join_group(group_id: String) -> Result<(), ServerFnError> {
    let Some(token) = session_token().await else {
        return Err(ServerFnError::new("нужно войти".to_string()));
    };
    let resp = reqwest::Client::new()
        .post(format!("{}/groups/{group_id}/join", api_base()))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    match resp.status().as_u16() {
        200..=299 => Ok(()),
        401 => Err(ServerFnError::new(
            "сессия истекла — войди заново".to_string(),
        )),
        409 => Err(ServerFnError::new("ты уже в сообществе".to_string())),
        _ => Err(ServerFnError::new("не удалось вступить".to_string())),
    }
}

/// Опубликовать пост в сообщество (Bearer; только участник). Пост виден в общей ленте.
#[server]
async fn post_to_group(group_id: String, body: String) -> Result<(), ServerFnError> {
    let Some(token) = session_token().await else {
        return Err(ServerFnError::new("нужно войти".to_string()));
    };
    let resp = reqwest::Client::new()
        .post(format!("{}/groups/{group_id}/posts", api_base()))
        .bearer_auth(token)
        .json(&serde_json::json!({ "body": body }))
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    match resp.status().as_u16() {
        200..=299 => Ok(()),
        401 => Err(ServerFnError::new(
            "сессия истекла — войди заново".to_string(),
        )),
        403 | 404 => Err(ServerFnError::new(
            "вступи в сообщество, чтобы постить".to_string(),
        )),
        422 => Err(ServerFnError::new("пост не может быть пустым".to_string())),
        _ => Err(ServerFnError::new("не удалось опубликовать".to_string())),
    }
}

// --- личные сообщения (DM: старт с профиля, единый инбокс — анти-ВК) ---

/// Инбокс текущего юзера (Bearer). Гость → пустой список.
#[server]
async fn fetch_inbox() -> Result<Vec<ConversationDto>, ServerFnError> {
    let Some(token) = session_token().await else {
        return Ok(Vec::new());
    };
    let resp = reqwest::Client::new()
        .get(format!("{}/inbox", api_base()))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    if !resp.status().is_success() {
        return Ok(Vec::new());
    }
    resp.json::<Vec<ConversationDto>>()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Переписка одного диалога (Bearer; только участник).
#[server]
async fn fetch_thread(conversation_id: String) -> Result<Vec<MessageDto>, ServerFnError> {
    let Some(token) = session_token().await else {
        return Err(ServerFnError::new("нужно войти".to_string()));
    };
    let resp = reqwest::Client::new()
        .get(format!(
            "{}/conversations/{conversation_id}/thread",
            api_base()
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(ServerFnError::new(
            "не удалось загрузить переписку".to_string(),
        ));
    }
    resp.json::<Vec<MessageDto>>()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Отправить сообщение по handle получателя (Bearer). Handle→UserId резолвится на
/// сервере (api ждёт UserId), так что UI оперирует только handle.
#[server]
async fn send_message(recipient_handle: String, body: String) -> Result<(), ServerFnError> {
    let Some(token) = session_token().await else {
        return Err(ServerFnError::new("нужно войти".to_string()));
    };
    let recipient = fetch_profile(recipient_handle).await?.user_id;
    let resp = reqwest::Client::new()
        .post(format!("{}/messages", api_base()))
        .bearer_auth(token)
        .json(&serde_json::json!({ "recipient": recipient, "body": body }))
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    match resp.status().as_u16() {
        200..=299 => Ok(()),
        401 => Err(ServerFnError::new(
            "сессия истекла — войди заново".to_string(),
        )),
        422 => Err(ServerFnError::new(
            "нельзя написать самому себе или пустой текст".to_string(),
        )),
        _ => Err(ServerFnError::new("не удалось отправить".to_string())),
    }
}

// --- верификация (ADR-0010/0016: гейт привилегий; заявка → рассмотрение админом) ---

/// Статус моей последней заявки (Bearer). Гость / нет заявки → `None`.
#[server]
async fn fetch_my_verification() -> Result<Option<MyVerificationDto>, ServerFnError> {
    let Some(token) = session_token().await else {
        return Ok(None);
    };
    let resp = reqwest::Client::new()
        .get(format!("{}/verification/me", api_base()))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    resp.json::<Option<MyVerificationDto>>()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Подать заявку на верификацию (Bearer). Пустая записка → без записки.
#[server]
async fn request_verification(note: String) -> Result<(), ServerFnError> {
    let Some(token) = session_token().await else {
        return Err(ServerFnError::new("нужно войти".to_string()));
    };
    let body = if note.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::json!({ "note": note })
    };
    let resp = reqwest::Client::new()
        .post(format!("{}/verification/requests", api_base()))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    match resp.status().as_u16() {
        200..=299 => Ok(()),
        401 => Err(ServerFnError::new(
            "сессия истекла — войди заново".to_string(),
        )),
        409 => Err(ServerFnError::new(
            "заявка уже подана или ты уже verified".to_string(),
        )),
        _ => Err(ServerFnError::new("не удалось подать заявку".to_string())),
    }
}

/// Очередь заявок на рассмотрении (Bearer; только админ). Не-админ/гость → пусто.
#[server]
async fn fetch_verification_queue() -> Result<Vec<VerificationQueueItemDto>, ServerFnError> {
    let Some(token) = session_token().await else {
        return Ok(Vec::new());
    };
    let resp = reqwest::Client::new()
        .get(format!("{}/verification/requests", api_base()))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    if !resp.status().is_success() {
        return Ok(Vec::new());
    }
    resp.json::<Vec<VerificationQueueItemDto>>()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Одобрить заявку (Bearer; только админ).
#[server]
async fn approve_verification(request_id: String) -> Result<(), ServerFnError> {
    verification_decision(&request_id, "approve", String::new()).await
}

/// Отклонить заявку с причиной (Bearer; только админ).
#[server]
async fn reject_verification(request_id: String, reason: String) -> Result<(), ServerFnError> {
    verification_decision(&request_id, "reject", reason).await
}

/// Общий POST решения по заявке: `approve` | `reject`. Пустая причина → без причины.
#[cfg(feature = "ssr")]
async fn verification_decision(
    request_id: &str,
    action: &str,
    reason: String,
) -> Result<(), ServerFnError> {
    let Some(token) = session_token().await else {
        return Err(ServerFnError::new("нужно войти".to_string()));
    };
    let body = if reason.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::json!({ "reason": reason })
    };
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/verification/requests/{request_id}/{action}",
            api_base()
        ))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    match resp.status().as_u16() {
        200..=299 => Ok(()),
        401 => Err(ServerFnError::new(
            "сессия истекла — войди заново".to_string(),
        )),
        403 => Err(ServerFnError::new("решать может только админ".to_string())),
        409 => Err(ServerFnError::new("заявка уже рассмотрена".to_string())),
        _ => Err(ServerFnError::new(
            "не удалось применить решение".to_string(),
        )),
    }
}

// --- музыка (ADR-0017: релиз за гейтом верификации; чтение публичное) ---

/// Общий раздел музыки — опубликованные треки (публичное чтение).
#[server]
async fn fetch_music() -> Result<Vec<TrackDto>, ServerFnError> {
    reqwest::get(format!("{}/music", api_base()))
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .json::<Vec<TrackDto>>()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Треки артиста (публичное чтение): для профиля.
#[server]
async fn fetch_artist_tracks(handle: String) -> Result<Vec<TrackDto>, ServerFnError> {
    let resp = reqwest::get(format!("{}/profiles/{handle}/tracks", api_base()))
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    if !resp.status().is_success() {
        return Ok(Vec::new());
    }
    resp.json::<Vec<TrackDto>>()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Выложить трек (Bearer; только верифицированный). Жанр пустой → без жанра (api сам
/// отсекает).
#[server]
async fn release_track(
    title: String,
    audio_url: String,
    genre: String,
) -> Result<(), ServerFnError> {
    let Some(token) = session_token().await else {
        return Err(ServerFnError::new("нужно войти".to_string()));
    };
    let resp = reqwest::Client::new()
        .post(format!("{}/tracks", api_base()))
        .bearer_auth(token)
        .json(&serde_json::json!({ "title": title, "audio_url": audio_url, "genre": genre }))
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    match resp.status().as_u16() {
        200..=299 => Ok(()),
        401 => Err(ServerFnError::new(
            "сессия истекла — войди заново".to_string(),
        )),
        403 => Err(ServerFnError::new(
            "релизить может только verified — пройди верификацию".to_string(),
        )),
        422 => Err(ServerFnError::new(
            "проверь название и ссылку (http/https)".to_string(),
        )),
        _ => Err(ServerFnError::new("не удалось выложить трек".to_string())),
    }
}

/// Снять свой трек (Bearer; только автор).
#[server]
async fn withdraw_track(track_id: String) -> Result<(), ServerFnError> {
    let Some(token) = session_token().await else {
        return Err(ServerFnError::new("нужно войти".to_string()));
    };
    let resp = reqwest::Client::new()
        .post(format!("{}/tracks/{track_id}/withdraw", api_base()))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    match resp.status().as_u16() {
        200..=299 => Ok(()),
        401 => Err(ServerFnError::new(
            "сессия истекла — войди заново".to_string(),
        )),
        403 => Err(ServerFnError::new("это не твой трек".to_string())),
        409 => Err(ServerFnError::new("трек уже снят".to_string())),
        _ => Err(ServerFnError::new("не удалось снять трек".to_string())),
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
    let post = ServerAction::<CreatePost>::new();
    let posted = post.value();
    let me = Resource::new(|| (), |()| fetch_me());
    // Лента перечитывается после успешной публикации.
    let feed = Resource::new(move || post.version().get(), |_| fetch_feed());
    view! {
        <div class="flex flex-col">
            <Suspense fallback=|| ()>
                {move || Suspend::new(async move {
                    match me.await {
                        Ok(Some(_)) => {
                            view! {
                                <div class="p-4 border-b border-[var(--border)]">
                                    <ActionForm action=post attr:class="flex flex-col gap-3">
                                        <TextArea
                                            name="body"
                                            label="что нового"
                                            placeholder="напиши в ленту"
                                        />
                                        <div class="flex items-center gap-3">
                                            <Button submit=true variant=ButtonVariant::Primary>
                                                "опубликовать"
                                            </Button>
                                            {move || match posted.get() {
                                                Some(Ok(())) => {
                                                    view! {
                                                        <span class="text-[var(--accent)] text-sm">"опубликовано"</span>
                                                    }
                                                        .into_any()
                                                }
                                                Some(Err(e)) => {
                                                    view! {
                                                        <span class="text-[var(--danger)] text-sm">{e.to_string()}</span>
                                                    }
                                                        .into_any()
                                                }
                                                None => ().into_any(),
                                            }}
                                        </div>
                                    </ActionForm>
                                </div>
                            }
                                .into_any()
                        }
                        _ => {
                            view! {
                                <p class="p-4 text-sm text-[var(--text-muted)] border-b border-[var(--border)]">
                                    <A href="/login">"войди"</A>
                                    ", чтобы постить"
                                </p>
                            }
                                .into_any()
                        }
                    }
                })}
            </Suspense>
            <Suspense fallback=move || {
                view! { <p class="p-4 text-[var(--text-muted)]">"загрузка ленты…"</p> }
            }>
                {move || Suspend::new(async move {
                    match feed.await {
                        Ok(items) if items.is_empty() => {
                            view! { <p class="p-4 text-[var(--text-muted)]">"пока пусто"</p> }
                                .into_any()
                        }
                        Ok(items) => {
                            view! {
                                <div>
                                    {items
                                        .into_iter()
                                        .map(|i| {
                                            view! {
                                                <FeedItem
                                                    author_handle=i.author_handle
                                                    body=i.body
                                                    group_slug=i.group_slug
                                                    group_name=i.group_name
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
                                <p class="p-4 text-[var(--danger)]">"не удалось загрузить ленту"</p>
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
fn MusicPage() -> impl IntoView {
    let release = ServerAction::<ReleaseTrack>::new();
    let released = release.value();
    let me = Resource::new(|| (), |()| fetch_me());
    // Раздел перечитывается после успешного релиза.
    let music = Resource::new(move || release.version().get(), |_| fetch_music());
    view! {
        <div class="p-4 flex flex-col gap-4">
            // Релиз — за гейтом верификации: форму видит только verified; иначе подсказка.
            <Suspense fallback=|| ()>
                {move || Suspend::new(async move {
                    match me.await.ok().flatten() {
                        Some(m) if m.verified => {
                            view! {
                                <Card>
                                    <h1 class="text-lg font-bold mb-3">"выложить трек"</h1>
                                    <ActionForm action=release attr:class="flex flex-col gap-3">
                                        <Field name="title" label="название" placeholder="Подвал" />
                                        <Field
                                            name="audio_url"
                                            label="ссылка на аудио"
                                            placeholder="https://…/track.mp3"
                                        />
                                        <Field
                                            name="genre"
                                            label="жанр (необязательно)"
                                            placeholder="boom bap"
                                        />
                                        <Button submit=true variant=ButtonVariant::Primary>
                                            "выложить"
                                        </Button>
                                    </ActionForm>
                                    {move || match released.get() {
                                        Some(Ok(())) => {
                                            view! { <p class="mt-2 text-[var(--accent)]">"трек выложен"</p> }
                                                .into_any()
                                        }
                                        Some(Err(e)) => {
                                            view! { <p class="mt-2 text-[var(--danger)]">{e.to_string()}</p> }
                                                .into_any()
                                        }
                                        None => ().into_any(),
                                    }}
                                </Card>
                            }
                                .into_any()
                        }
                        Some(_) => {
                            view! {
                                <Card>
                                    <p class="text-sm text-[var(--text-muted)]">
                                        "выкладывать музыку могут только verified — "
                                        <A href="/verification">"пройди верификацию"</A>
                                    </p>
                                </Card>
                            }
                                .into_any()
                        }
                        None => {
                            view! {
                                <Card>
                                    <p class="text-sm text-[var(--text-muted)]">
                                        <A href="/login">"войди"</A>
                                        ", чтобы выкладывать музыку"
                                    </p>
                                </Card>
                            }
                                .into_any()
                        }
                    }
                })}
            </Suspense>
            <Suspense fallback=move || {
                view! { <p class="text-[var(--text-muted)]">"загрузка музыки…"</p> }
            }>
                {move || Suspend::new(async move {
                    match music.await {
                        Ok(items) if items.is_empty() => {
                            view! { <p class="text-[var(--text-muted)]">"пока тихо"</p> }.into_any()
                        }
                        Ok(items) => {
                            view! {
                                <div class="flex flex-col gap-3">
                                    {items
                                        .into_iter()
                                        .map(|t| {
                                            view! {
                                                <TrackCard
                                                    title=t.title
                                                    artist_handle=t.artist_handle
                                                    genre=t.genre
                                                    audio_url=t.audio_url
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
                                <p class="text-[var(--danger)]">"не удалось загрузить музыку"</p>
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
fn VerificationPage() -> impl IntoView {
    let request = ServerAction::<RequestVerification>::new();
    let approve = ServerAction::<ApproveVerification>::new();
    let reject = ServerAction::<RejectVerification>::new();
    let req_val = request.value();
    let me = Resource::new(|| (), |()| fetch_me());
    // Свой статус перечитывается после подачи заявки.
    let myver = Resource::new(move || request.version().get(), |_| fetch_my_verification());
    // Очередь перечитывается после любого решения.
    let queue = Resource::new(
        move || (approve.version().get(), reject.version().get()),
        |_| fetch_verification_queue(),
    );
    view! {
        <div class="p-4 flex flex-col gap-4">
            <h1 class="text-lg font-bold text-[var(--text)]">"верификация"</h1>
            <p class="text-sm text-[var(--text-muted)]">
                "верификация открывает маркет и загрузку музыки. подай заявку — её рассмотрит админ."
            </p>
            // --- моя заявка / статус ---
            <Suspense fallback=move || {
                view! { <p class="text-[var(--text-muted)]">"загрузка…"</p> }
            }>
                {move || Suspend::new(async move {
                    let Some(me) = me.await.ok().flatten() else {
                        return view! {
                            <p class="text-[var(--text-muted)]">
                                <A href="/login">"войди"</A>
                                ", чтобы пройти верификацию"
                            </p>
                        }
                            .into_any();
                    };
                    if me.verified {
                        return view! {
                            <Card>
                                <div class="flex items-center gap-2">
                                    "ты " <Badge accent=true>"verified"</Badge>
                                    " — маркет и музыка открыты"
                                </div>
                            </Card>
                        }
                            .into_any();
                    }
                    let status = myver.await.ok().flatten();
                    let form = move || {
                        view! {
                            <ActionForm action=request attr:class="flex flex-col gap-3">
                                <TextArea
                                    name="note"
                                    label="пара слов о себе (необязательно)"
                                    placeholder="ссылки на треки, кто ты, чем занят"
                                />
                                <Button submit=true variant=ButtonVariant::Primary>
                                    "подать заявку"
                                </Button>
                            </ActionForm>
                            {move || match req_val.get() {
                                Some(Err(e)) => {
                                    view! {
                                        <p class="mt-2 text-[var(--danger)] text-sm">{e.to_string()}</p>
                                    }
                                        .into_any()
                                }
                                _ => ().into_any(),
                            }}
                        }
                    };
                    match status.as_ref().map(|s| s.status.as_str()) {
                        Some("pending") => {
                            view! {
                                <Card>
                                    <p class="text-[var(--text-muted)]">"заявка на рассмотрении"</p>
                                </Card>
                            }
                                .into_any()
                        }
                        Some("rejected") => {
                            let reason = status.and_then(|s| s.decision_reason);
                            view! {
                                <Card>
                                    <p class="text-[var(--text)]">"заявка отклонена"</p>
                                    {reason
                                        .map(|r| {
                                            view! {
                                                <p class="mt-1 mb-3 text-sm text-[var(--text-muted)]">{r}</p>
                                            }
                                        })}
                                    {form()}
                                </Card>
                            }
                                .into_any()
                        }
                        _ => view! { <Card>{form()}</Card> }.into_any(),
                    }
                })}
            </Suspense>
            // --- очередь админа (пусто для не-админов — секция скрыта) ---
            <Suspense fallback=|| ()>
                {move || Suspend::new(async move {
                    match queue.await {
                        Ok(items) if !items.is_empty() => {
                            view! {
                                <div class="flex flex-col gap-3">
                                    <h2 class="font-bold text-[var(--text)]">"заявки на рассмотрении"</h2>
                                    {items
                                        .into_iter()
                                        .map(|q| {
                                            let id_a = q.request_id.clone();
                                            let id_r = q.request_id.clone();
                                            view! {
                                                <Card>
                                                    <div class="font-semibold text-[var(--text)]">
                                                        "@"{q.requester_handle}
                                                    </div>
                                                    {q
                                                        .note
                                                        .map(|n| {
                                                            view! {
                                                                <p class="mt-1 text-sm text-[var(--text-muted)]">{n}</p>
                                                            }
                                                        })}
                                                    <div class="flex items-center gap-3 mt-3">
                                                        <ActionForm action=approve attr:class="inline">
                                                            <input type="hidden" name="request_id" value=id_a />
                                                            <button
                                                                type="submit"
                                                                class="text-sm font-semibold text-[var(--accent)] hover:underline"
                                                            >
                                                                "одобрить"
                                                            </button>
                                                        </ActionForm>
                                                        <ActionForm
                                                            action=reject
                                                            attr:class="flex items-center gap-2"
                                                        >
                                                            <input type="hidden" name="request_id" value=id_r />
                                                            <input
                                                                name="reason"
                                                                placeholder="причина (необязательно)"
                                                                class="px-2 py-1 text-sm bg-[var(--surface-raised)] border border-[var(--border)] rounded-[var(--radius)] text-[var(--text)]"
                                                            />
                                                            <button
                                                                type="submit"
                                                                class="text-sm text-[var(--text-muted)] hover:text-[var(--text)]"
                                                            >
                                                                "отклонить"
                                                            </button>
                                                        </ActionForm>
                                                    </div>
                                                </Card>
                                            }
                                        })
                                        .collect_view()}
                                </div>
                            }
                                .into_any()
                        }
                        _ => ().into_any(),
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
    let sold = ServerAction::<MarkSold>::new();
    let withdraw = ServerAction::<Withdraw>::new();
    let withdraw_track = ServerAction::<WithdrawTrack>::new();
    let send = ServerAction::<SendMessage>::new();
    let sent = send.value();
    let profile = Resource::new(handle, fetch_profile);
    let me = Resource::new(|| (), |()| fetch_me());
    // Товары перечитываются после sold/withdraw.
    let listings = Resource::new(
        move || (handle(), sold.version().get(), withdraw.version().get()),
        |(h, _, _)| fetch_seller_listings(h),
    );
    // Треки перечитываются после снятия.
    let tracks = Resource::new(
        move || (handle(), withdraw_track.version().get()),
        |(h, _)| fetch_artist_tracks(h),
    );
    // Ошибка последнего действия над товаром (реактивно, вне Suspense).
    let action_error = move || {
        let err = sold
            .value()
            .get()
            .and_then(Result::err)
            .or_else(|| withdraw.value().get().and_then(Result::err));
        err.map(|e| view! { <p class="text-[var(--danger)] text-sm">{e.to_string()}</p> })
    };
    view! {
        <div class="p-4 flex flex-col gap-4">
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
            // Свой профиль и ещё не verified — приглашение пройти верификацию (анти-ВК:
            // действие живёт в профиле, форма — на /verification).
            <Suspense fallback=|| ()>
                {move || Suspend::new(async move {
                    let my_handle = me.await.ok().flatten().map(|m| m.handle);
                    match (my_handle, profile.await.ok()) {
                        (Some(mh), Some(p)) if mh == p.handle && !p.verified => {
                            view! {
                                <Card>
                                    <p class="text-sm text-[var(--text-muted)]">
                                        "ты ещё не verified — "
                                        <A href="/verification">"подай заявку"</A>
                                        ", чтобы продавать и грузить музыку"
                                    </p>
                                </Card>
                            }
                                .into_any()
                        }
                        _ => ().into_any(),
                    }
                })}
            </Suspense>
            // Написать в личку — для залогиненных, на чужом профиле (анти-ВК: DM с профиля).
            <Suspense fallback=|| ()>
                {move || Suspend::new(async move {
                    let my_handle = me.await.ok().flatten().map(|m| m.handle);
                    let target = profile.await.ok().map(|p| p.handle);
                    match (my_handle, target) {
                        (Some(mh), Some(t)) if mh != t => {
                            let label = format!("написать @{t}");
                            view! {
                                <Card>
                                    <ActionForm action=send attr:class="flex flex-col gap-3">
                                        <input type="hidden" name="recipient_handle" value=t />
                                        <TextArea name="body" label=label placeholder="сообщение" />
                                        <Button submit=true variant=ButtonVariant::Primary>
                                            "отправить"
                                        </Button>
                                    </ActionForm>
                                    {move || match sent.get() {
                                        Some(Ok(())) => {
                                            view! {
                                                <p class="mt-2 text-[var(--accent)] text-sm">
                                                    "отправлено · " <A href="/messages">"в сообщения"</A>
                                                </p>
                                            }
                                                .into_any()
                                        }
                                        Some(Err(e)) => {
                                            view! {
                                                <p class="mt-2 text-[var(--danger)] text-sm">{e.to_string()}</p>
                                            }
                                                .into_any()
                                        }
                                        None => ().into_any(),
                                    }}
                                </Card>
                            }
                                .into_any()
                        }
                        _ => ().into_any(),
                    }
                })}
            </Suspense>
            {action_error}
            <Suspense fallback=|| ()>
                {move || Suspend::new(async move {
                    let my_handle = me.await.ok().flatten().map(|m| m.handle);
                    match listings.await {
                        Ok(items) if items.is_empty() => ().into_any(),
                        Ok(items) => {
                            view! {
                                <div class="flex flex-col gap-3">
                                    <h2 class="font-bold text-[var(--text)]">"товары"</h2>
                                    {items
                                        .into_iter()
                                        .map(|l| {
                                            let owner = my_handle.as_deref()
                                                == Some(l.seller_handle.as_str());
                                            let active = l.status == "active";
                                            let actions = (owner && active)
                                                .then(|| {
                                                    let id_sold = l.listing_id.clone();
                                                    let id_withdraw = l.listing_id.clone();
                                                    view! {
                                                        <div class="flex gap-3 pl-1">
                                                            <ActionForm action=sold attr:class="inline">
                                                                <input
                                                                    type="hidden"
                                                                    name="listing_id"
                                                                    value=id_sold
                                                                />
                                                                <button
                                                                    type="submit"
                                                                    class="text-sm text-[var(--text-muted)] hover:text-[var(--text)]"
                                                                >
                                                                    "продано"
                                                                </button>
                                                            </ActionForm>
                                                            <ActionForm action=withdraw attr:class="inline">
                                                                <input
                                                                    type="hidden"
                                                                    name="listing_id"
                                                                    value=id_withdraw
                                                                />
                                                                <button
                                                                    type="submit"
                                                                    class="text-sm text-[var(--text-muted)] hover:text-[var(--text)]"
                                                                >
                                                                    "снять"
                                                                </button>
                                                            </ActionForm>
                                                        </div>
                                                    }
                                                });
                                            view! {
                                                <div class="flex flex-col gap-1">
                                                    <ListingCard
                                                        title=l.title
                                                        seller_handle=l.seller_handle
                                                        price=l.price
                                                        status=l.status
                                                    />
                                                    {actions}
                                                </div>
                                            }
                                        })
                                        .collect_view()}
                                </div>
                            }
                                .into_any()
                        }
                        Err(_) => {
                            view! {
                                <p class="text-[var(--danger)]">"не удалось загрузить товары"</p>
                            }
                                .into_any()
                        }
                    }
                })}
            </Suspense>
            // Треки артиста (анти-ВК: музыка живёт в профиле). Снять может только автор.
            <Suspense fallback=|| ()>
                {move || Suspend::new(async move {
                    let my_handle = me.await.ok().flatten().map(|m| m.handle);
                    match tracks.await {
                        Ok(items) if items.is_empty() => ().into_any(),
                        Ok(items) => {
                            view! {
                                <div class="flex flex-col gap-3">
                                    <h2 class="font-bold text-[var(--text)]">"музыка"</h2>
                                    {items
                                        .into_iter()
                                        .map(|t| {
                                            let owner = my_handle.as_deref()
                                                == Some(t.artist_handle.as_str());
                                            let action = owner
                                                .then(|| {
                                                    let id = t.track_id.clone();
                                                    view! {
                                                        <ActionForm
                                                            action=withdraw_track
                                                            attr:class="inline pl-1"
                                                        >
                                                            <input type="hidden" name="track_id" value=id />
                                                            <button
                                                                type="submit"
                                                                class="text-sm text-[var(--text-muted)] hover:text-[var(--text)]"
                                                            >
                                                                "снять"
                                                            </button>
                                                        </ActionForm>
                                                    }
                                                });
                                            view! {
                                                <div class="flex flex-col gap-1">
                                                    <TrackCard
                                                        title=t.title
                                                        artist_handle=t.artist_handle
                                                        genre=t.genre
                                                        audio_url=t.audio_url
                                                    />
                                                    {action}
                                                </div>
                                            }
                                        })
                                        .collect_view()}
                                </div>
                            }
                                .into_any()
                        }
                        Err(_) => ().into_any(),
                    }
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn GroupPage() -> impl IntoView {
    let params = use_params_map();
    let slug = move || params.read().get("slug").unwrap_or_default();
    let join = ServerAction::<JoinGroup>::new();
    let post = ServerAction::<PostToGroup>::new();
    let me = Resource::new(|| (), |()| fetch_me());
    // Карточка перечитывается после вступления/поста (обновляет счётчик участников).
    let group = Resource::new(
        move || (slug(), join.version().get(), post.version().get()),
        |(s, _, _)| fetch_group(s),
    );
    let joined = join.value();
    let posted = post.value();
    let join_feedback = move || match joined.get() {
        Some(Ok(())) => {
            view! { <p class="text-[var(--accent)] text-sm">"ты в сообществе"</p> }.into_any()
        }
        Some(Err(e)) => {
            view! { <p class="text-[var(--danger)] text-sm">{e.to_string()}</p> }.into_any()
        }
        None => ().into_any(),
    };
    let post_feedback = move || match posted.get() {
        Some(Ok(())) => {
            view! { <p class="text-[var(--accent)] text-sm">"опубликовано в ленте"</p> }.into_any()
        }
        Some(Err(e)) => {
            view! { <p class="text-[var(--danger)] text-sm">{e.to_string()}</p> }.into_any()
        }
        None => ().into_any(),
    };
    view! {
        <div class="p-4 flex flex-col gap-4">
            <Suspense fallback=move || view! { <p class="text-[var(--text-muted)]">"загрузка…"</p> }>
                {move || Suspend::new(async move {
                    let logged_in = matches!(me.await, Ok(Some(_)));
                    match group.await {
                        Ok(g) => {
                            let gid_join = g.group_id.clone();
                            let gid_post = g.group_id.clone();
                            let member_label = format!("{} участн.", g.member_count);
                            let actions = if logged_in {
                                view! {
                                    <ActionForm action=join attr:class="inline">
                                        <input type="hidden" name="group_id" value=gid_join />
                                        <Button submit=true variant=ButtonVariant::Primary>
                                            "вступить"
                                        </Button>
                                    </ActionForm>
                                    <Card>
                                        <ActionForm action=post attr:class="flex flex-col gap-3">
                                            <input type="hidden" name="group_id" value=gid_post />
                                            <TextArea
                                                name="body"
                                                label="написать в сообщество"
                                                placeholder="пост увидят в общей ленте"
                                            />
                                            <Button submit=true variant=ButtonVariant::Primary>
                                                "опубликовать"
                                            </Button>
                                        </ActionForm>
                                    </Card>
                                }
                                    .into_any()
                            } else {
                                view! {
                                    <p class="text-sm text-[var(--text-muted)]">
                                        <A href="/login">"войди"</A>
                                        ", чтобы вступить и постить"
                                    </p>
                                }
                                    .into_any()
                            };
                            view! {
                                <Card>
                                    <div class="flex items-center justify-between gap-3">
                                        <div>
                                            <div class="text-lg font-bold">{g.name}</div>
                                            <div class="text-[var(--text-muted)]">
                                                "/g/"{g.slug} " · " {member_label}
                                            </div>
                                        </div>
                                        <Badge accent=true>{g.kind}</Badge>
                                    </div>
                                </Card>
                                {actions}
                            }
                                .into_any()
                        }
                        Err(_) => {
                            view! { <p class="text-[var(--danger)]">"сообщество не найдено"</p> }
                                .into_any()
                        }
                    }
                })}
            </Suspense>
            {join_feedback}
            {post_feedback}
        </div>
    }
}

#[component]
fn InboxPage() -> impl IntoView {
    let me = Resource::new(|| (), |()| fetch_me());
    let inbox = Resource::new(|| (), |()| fetch_inbox());
    view! {
        <div class="p-4 flex flex-col gap-3">
            <h1 class="text-lg font-bold">"сообщения"</h1>
            <Suspense fallback=move || {
                view! { <p class="text-[var(--text-muted)]">"загрузка…"</p> }
            }>
                {move || Suspend::new(async move {
                    if !matches!(me.await, Ok(Some(_))) {
                        return view! {
                            <p class="text-sm text-[var(--text-muted)]">
                                <A href="/login">"войди"</A>
                                ", чтобы читать сообщения"
                            </p>
                        }
                            .into_any();
                    }
                    match inbox.await {
                        Ok(items) if items.is_empty() => {
                            view! { <p class="text-[var(--text-muted)]">"переписок пока нет"</p> }
                                .into_any()
                        }
                        Ok(items) => {
                            view! {
                                <div class="flex flex-col gap-2">
                                    {items
                                        .into_iter()
                                        .map(|c| {
                                            let href = format!(
                                                "/messages/{}?with={}",
                                                c.conversation_id,
                                                c.counterpart_handle,
                                            );
                                            view! {
                                                <a href=href class="block">
                                                    <Card>
                                                        <div class="font-semibold text-[var(--text)]">
                                                            "@"{c.counterpart_handle}
                                                        </div>
                                                        <div class="text-[var(--text-muted)] truncate">
                                                            {c.last_message}
                                                        </div>
                                                    </Card>
                                                </a>
                                            }
                                        })
                                        .collect_view()}
                                </div>
                            }
                                .into_any()
                        }
                        Err(_) => {
                            view! {
                                <p class="text-[var(--danger)]">"не удалось загрузить инбокс"</p>
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
fn ThreadPage() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let id = move || params.read().get("id").unwrap_or_default();
    // Собеседник прокинут из инбокса (`?with=handle`) — по нему шлём ответ.
    let with = move || query.read().get("with").unwrap_or_default();
    let send = ServerAction::<SendMessage>::new();
    let sent = send.value();
    let thread = Resource::new(
        move || (id(), send.version().get()),
        |(i, _)| fetch_thread(i),
    );
    view! {
        <div class="p-4 flex flex-col gap-3">
            <A href="/messages">"← к сообщениям"</A>
            <Suspense fallback=move || {
                view! { <p class="text-[var(--text-muted)]">"загрузка переписки…"</p> }
            }>
                {move || Suspend::new(async move {
                    match thread.await {
                        Ok(msgs) if msgs.is_empty() => {
                            view! { <p class="text-[var(--text-muted)]">"пока пусто"</p> }.into_any()
                        }
                        Ok(msgs) => {
                            view! {
                                <div class="flex flex-col gap-2">
                                    {msgs
                                        .into_iter()
                                        .map(|m| {
                                            view! {
                                                <Card>
                                                    <div class="text-sm text-[var(--text-muted)]">
                                                        "@"{m.author_handle}
                                                    </div>
                                                    <p class="text-[var(--text)] whitespace-pre-wrap break-words">
                                                        {m.body}
                                                    </p>
                                                </Card>
                                            }
                                        })
                                        .collect_view()}
                                </div>
                            }
                                .into_any()
                        }
                        Err(e) => {
                            view! { <p class="text-[var(--danger)]">{e.to_string()}</p> }.into_any()
                        }
                    }
                })}
            </Suspense>
            {move || {
                let w = with();
                (!w.is_empty())
                    .then(|| {
                        view! {
                            <Card>
                                <ActionForm action=send attr:class="flex flex-col gap-3">
                                    <input type="hidden" name="recipient_handle" value=w />
                                    <TextArea name="body" label="ответить" placeholder="сообщение" />
                                    <Button submit=true variant=ButtonVariant::Primary>
                                        "отправить"
                                    </Button>
                                </ActionForm>
                                {move || match sent.get() {
                                    Some(Err(e)) => {
                                        view! {
                                            <p class="mt-2 text-[var(--danger)] text-sm">{e.to_string()}</p>
                                        }
                                            .into_any()
                                    }
                                    _ => ().into_any(),
                                }}
                            </Card>
                        }
                    })
            }}
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
