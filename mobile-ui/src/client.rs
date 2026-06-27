//! Data-client mobile: прямой HTTP к API babangida (ADR-0015), тот же контракт,
//! что и web. DTO минимальные — зеркалят JSON `api`, лишние поля serde игнорирует.

use serde::Deserialize;

/// База API. На устройстве укажи реальный хост (LAN/публичный); дефолт — локальный
/// (для хостовой проверки/симулятора). Кандидат на конфиг при выходе за PoC.
pub const API_BASE: &str = "http://127.0.0.1:8080";

/// Элемент ленты.
#[derive(Clone, Deserialize)]
pub struct FeedItemDto {
    pub author_handle: String,
    pub body: String,
}

#[derive(Deserialize)]
struct LoginRes {
    token: String,
}

/// Свежая лента (публичное чтение).
pub async fn fetch_feed() -> Result<Vec<FeedItemDto>, String> {
    reqwest::get(format!("{API_BASE}/feed"))
        .await
        .map_err(|e| e.to_string())?
        .json::<Vec<FeedItemDto>>()
        .await
        .map_err(|e| e.to_string())
}

/// Вход: возвращает токен сессии (ADR-0013). Шлём как `Authorization: Bearer` далее.
pub async fn login(handle: String, password: String) -> Result<String, String> {
    let resp = reqwest::Client::new()
        .post(format!("{API_BASE}/login"))
        .json(&serde_json::json!({ "handle": handle, "password": password }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err("неверный handle или пароль".to_owned());
    }
    let body = resp.json::<LoginRes>().await.map_err(|e| e.to_string())?;
    Ok(body.token)
}
