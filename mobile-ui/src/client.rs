//! Data-client mobile: прямой HTTP к API babangida (ADR-0015), тот же контракт, что
//! и web. DTO минимальные — зеркалят JSON `api`, лишние поля serde игнорирует. Токен
//! сессии (если есть) шлём как `Authorization: Bearer` — для viewer-aware выдачи и
//! записи (ADR-0013).

use serde::Deserialize;

/// База API. На устройстве укажи реальный хост (LAN/публичный); дефолт — локальный
/// (для хостовой проверки/симулятора). Кандидат на конфиг при выходе за PoC.
pub const API_BASE: &str = "http://127.0.0.1:8080";

#[derive(Clone, Deserialize)]
pub struct FeedItemDto {
    pub author_handle: String,
    pub body: String,
    #[serde(default)]
    pub group_slug: Option<String>,
    #[serde(default)]
    pub group_name: Option<String>,
}

#[derive(Clone, Deserialize)]
pub struct MeDto {
    pub handle: String,
}

#[derive(Clone, Deserialize)]
pub struct ProfileDto {
    pub user_id: String,
    pub handle: String,
    pub display_name: String,
    pub subculture: String,
    pub bio: Option<String>,
    pub verified: bool,
}

#[derive(Clone, Deserialize)]
pub struct ListingDto {
    pub title: String,
    pub seller_handle: String,
    pub price: u64,
    pub status: String,
}

#[derive(Clone, Deserialize)]
pub struct ConversationDto {
    pub conversation_id: String,
    pub counterpart_handle: String,
    pub last_message: String,
}

#[derive(Clone, Deserialize)]
pub struct MessageDto {
    pub author_handle: String,
    pub body: String,
}

#[derive(Clone, Deserialize)]
pub struct GroupDto {
    pub group_id: String,
    pub slug: String,
    pub name: String,
    pub kind: String,
    pub member_count: u32,
}

#[derive(Deserialize)]
struct LoginRes {
    token: String,
}

fn http() -> reqwest::Client {
    reqwest::Client::new()
}

fn bearer(req: reqwest::RequestBuilder, token: Option<&str>) -> reqwest::RequestBuilder {
    match token {
        Some(t) => req.bearer_auth(t),
        None => req,
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// --- чтения ---

/// Свежая лента. С токеном — viewer-aware (видны посты своих закрытых групп).
pub async fn fetch_feed(token: Option<String>) -> Result<Vec<FeedItemDto>, String> {
    bearer(http().get(format!("{API_BASE}/feed")), token.as_deref())
        .send()
        .await
        .map_err(err)?
        .json::<Vec<FeedItemDto>>()
        .await
        .map_err(err)
}

/// Текущий юзер по токену (`None` — гость/невалидный токен).
pub async fn fetch_me(token: String) -> Option<MeDto> {
    let resp = http()
        .get(format!("{API_BASE}/me"))
        .bearer_auth(token)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<MeDto>().await.ok()
}

/// Профиль по handle (публичное чтение).
pub async fn fetch_profile(handle: String) -> Result<ProfileDto, String> {
    let resp = http()
        .get(format!("{API_BASE}/profiles/{handle}"))
        .send()
        .await
        .map_err(err)?;
    if !resp.status().is_success() {
        return Err("профиль не найден".to_owned());
    }
    resp.json::<ProfileDto>().await.map_err(err)
}

/// Товары продавца (публичное чтение).
pub async fn fetch_seller_listings(handle: String) -> Result<Vec<ListingDto>, String> {
    let resp = http()
        .get(format!("{API_BASE}/profiles/{handle}/listings"))
        .send()
        .await
        .map_err(err)?;
    if !resp.status().is_success() {
        return Ok(Vec::new());
    }
    resp.json::<Vec<ListingDto>>().await.map_err(err)
}

/// Активные товары барахолки (публичное чтение).
pub async fn fetch_market() -> Result<Vec<ListingDto>, String> {
    http()
        .get(format!("{API_BASE}/market"))
        .send()
        .await
        .map_err(err)?
        .json::<Vec<ListingDto>>()
        .await
        .map_err(err)
}

/// Инбокс (Bearer).
pub async fn fetch_inbox(token: String) -> Result<Vec<ConversationDto>, String> {
    http()
        .get(format!("{API_BASE}/inbox"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(err)?
        .json::<Vec<ConversationDto>>()
        .await
        .map_err(err)
}

/// Переписка одного диалога (Bearer; только участник).
pub async fn fetch_thread(
    token: String,
    conversation_id: String,
) -> Result<Vec<MessageDto>, String> {
    http()
        .get(format!("{API_BASE}/conversations/{conversation_id}/thread"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(err)?
        .json::<Vec<MessageDto>>()
        .await
        .map_err(err)
}

/// Карточка сообщества по слагу (публичное чтение).
pub async fn fetch_group(slug: String) -> Result<GroupDto, String> {
    let resp = http()
        .get(format!("{API_BASE}/groups/{slug}"))
        .send()
        .await
        .map_err(err)?;
    if !resp.status().is_success() {
        return Err("сообщество не найдено".to_owned());
    }
    resp.json::<GroupDto>().await.map_err(err)
}

// --- записи (Bearer) ---

/// Вход: токен сессии (ADR-0013).
pub async fn login(handle: String, password: String) -> Result<String, String> {
    let resp = http()
        .post(format!("{API_BASE}/login"))
        .json(&serde_json::json!({ "handle": handle, "password": password }))
        .send()
        .await
        .map_err(err)?;
    if !resp.status().is_success() {
        return Err("неверный handle или пароль".to_owned());
    }
    Ok(resp.json::<LoginRes>().await.map_err(err)?.token)
}

/// Опубликовать пост в общую ленту.
pub async fn create_post(token: String, body: String) -> Result<(), String> {
    status_only(
        http()
            .post(format!("{API_BASE}/posts"))
            .bearer_auth(token)
            .json(&serde_json::json!({ "body": body })),
    )
    .await
}

/// Отправить сообщение по handle получателя (handle→UserId резолвится здесь же).
pub async fn send_message(
    token: String,
    recipient_handle: String,
    body: String,
) -> Result<(), String> {
    let recipient = fetch_profile(recipient_handle).await?.user_id;
    status_only(
        http()
            .post(format!("{API_BASE}/messages"))
            .bearer_auth(token)
            .json(&serde_json::json!({ "recipient": recipient, "body": body })),
    )
    .await
}

/// Вступить в сообщество.
pub async fn join_group(token: String, group_id: String) -> Result<(), String> {
    status_only(
        http()
            .post(format!("{API_BASE}/groups/{group_id}/join"))
            .bearer_auth(token),
    )
    .await
}

/// Опубликовать пост в сообщество (виден в общей ленте).
pub async fn post_to_group(token: String, group_id: String, body: String) -> Result<(), String> {
    status_only(
        http()
            .post(format!("{API_BASE}/groups/{group_id}/posts"))
            .bearer_auth(token)
            .json(&serde_json::json!({ "body": body })),
    )
    .await
}

/// Отправить запрос и свести ответ к `Ok(())`/понятной ошибке по статусу.
async fn status_only(req: reqwest::RequestBuilder) -> Result<(), String> {
    let resp = req.send().await.map_err(err)?;
    match resp.status().as_u16() {
        200..=299 => Ok(()),
        401 => Err("сессия истекла — войди заново".to_owned()),
        403 => Err("недостаточно прав".to_owned()),
        404 => Err("не найдено".to_owned()),
        409 => Err("конфликт состояния".to_owned()),
        422 => Err("проверь ввод".to_owned()),
        _ => Err("не удалось выполнить".to_owned()),
    }
}
