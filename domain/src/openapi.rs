//! Контекст openapi: персональные API-ключи для открытого API (ADR-0018). Выдача —
//! привилегия верифицированных (ADR-0010): гейт держим инвариантом домена
//! ([`ApiKey::issue`] требует [`VerifiedStatus`]), `application` лишь читает статус.
//!
//! Чистый домен (ADR-0003): генерация секрета и его хэширование — недетерминированные
//! операции на границе; сюда приходит готовый [`ApiKeyHash`]. Домен хранит инварианты:
//! формат токена, гейт выдачи, право отзыва. По образцу [`crate::auth`].

use async_trait::async_trait;
use babangida_shared::{Id, Timestamp};

use crate::RepositoryError;
use crate::identity::{UserId, VerifiedStatus};

/// Непрозрачный секрет ключа, который держит интегратор (Bearer для `/api/v1`).
/// Высокоэнтропийный, генерируется на границе ([`ApiKeyFactory`](crate)); показывается
/// владельцу один раз. Debug скрыт — это секрет.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ApiKeyToken(String);

impl ApiKeyToken {
    /// Минимальная длина токена.
    pub const MIN_LEN: usize = 16;
    /// Максимальная длина — отсекаем мусор на входе.
    pub const MAX_LEN: usize = 128;

    /// Распарсить токен (от фабрики или из заголовка). Алфавит `A-Za-z0-9-_`
    /// (префикс `bbg_` допустим), длина в пределах [`MIN_LEN`]..=[`MAX_LEN`].
    ///
    /// # Errors
    /// [`ApiKeyTokenError`], если длина или символы нарушают правила.
    pub fn parse(input: &str) -> Result<Self, ApiKeyTokenError> {
        let len = input.chars().count();
        if !(Self::MIN_LEN..=Self::MAX_LEN).contains(&len) {
            return Err(ApiKeyTokenError::WrongLength { len });
        }
        if let Some(c) = input
            .chars()
            .find(|c| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '_'))
        {
            return Err(ApiKeyTokenError::InvalidChar(c));
        }
        Ok(Self(input.to_owned()))
    }

    /// Открыть значение для хэширования/выдачи владельцу. Зовётся только границей.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Debug for ApiKeyToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("ApiKeyToken(***)")
    }
}

/// Нарушение правил [`ApiKeyToken`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApiKeyTokenError {
    #[error("неверная длина API-ключа: {len} символов")]
    WrongLength { len: usize },
    #[error("недопустимый символ в API-ключе: {0:?}")]
    InvalidChar(char),
}

/// Хэш ключа (для хранения и поиска при аутентификации). Непрозрачен для домена:
/// единственный источник — [`ApiKeyHasher`](crate). Debug скрыт.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKeyHash(String);

impl ApiKeyHash {
    /// Обернуть готовый хэш (из адаптера-хэшера или из БД при реконституции).
    #[must_use]
    pub fn from_storage(value: String) -> Self {
        Self(value)
    }

    /// Строковое представление для сохранения/поиска.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Debug for ApiKeyHash {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("ApiKeyHash(***)")
    }
}

/// Метка ключа — имя, по которому владелец узнаёт ключ в списке. 1..=80 символов.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKeyLabel(String);

impl ApiKeyLabel {
    /// Максимальная длина метки.
    pub const MAX_LEN: usize = 80;

    /// Распарсить метку.
    ///
    /// # Errors
    /// [`ApiKeyLabelError`], если пусто или длиннее [`MAX_LEN`](Self::MAX_LEN).
    pub fn parse(input: &str) -> Result<Self, ApiKeyLabelError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(ApiKeyLabelError::Empty);
        }
        let len = trimmed.chars().count();
        if len > Self::MAX_LEN {
            return Err(ApiKeyLabelError::TooLong { len });
        }
        Ok(Self(trimmed.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Нарушение правил [`ApiKeyLabel`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApiKeyLabelError {
    #[error("метка ключа пустая")]
    Empty,
    #[error("метка ключа слишком длинная: {len} символов (максимум 80)")]
    TooLong { len: usize },
}

/// Статус ключа.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiKeyStatus {
    /// Действует — им можно ходить в `/api/v1`.
    Active,
    /// Отозван владельцем.
    Revoked,
}

impl ApiKeyStatus {
    /// Действует ли (можно аутентифицироваться/отозвать).
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Revoked => "revoked",
        }
    }
}

/// Фантомный маркер для [`ApiKeyId`].
pub enum ApiKeyMarker {}
/// Идентификатор API-ключа.
pub type ApiKeyId = Id<ApiKeyMarker>;

/// API-ключ — корень агрегата openapi. Выпускает только верифицированный владелец;
/// отозвать может только он. Хранит хэш секрета — сырого токена в домене нет.
#[derive(Debug, Clone)]
pub struct ApiKey {
    id: ApiKeyId,
    owner: UserId,
    label: ApiKeyLabel,
    hash: ApiKeyHash,
    status: ApiKeyStatus,
    created_at: Timestamp,
}

impl ApiKey {
    /// Выпустить ключ. Гейт верификации (ADR-0010): владелец обязан быть верифицирован —
    /// статус читает `application`, инвариант держит домен. Хэш приходит готовым с границы.
    ///
    /// # Errors
    /// [`OpenApiError::NotVerified`], если владелец не верифицирован.
    pub fn issue(
        id: ApiKeyId,
        owner: UserId,
        verified: VerifiedStatus,
        label: ApiKeyLabel,
        hash: ApiKeyHash,
        now: Timestamp,
    ) -> Result<(Self, ApiKeyIssued), OpenApiError> {
        if !verified.is_verified() {
            return Err(OpenApiError::NotVerified);
        }
        let key = Self {
            id,
            owner,
            label,
            hash,
            status: ApiKeyStatus::Active,
            created_at: now,
        };
        let event = ApiKeyIssued { key: id, owner };
        Ok((key, event))
    }

    /// Отозвать ключ (только владелец, только из действующего).
    ///
    /// # Errors
    /// [`OpenApiError`]: не владелец или ключ уже отозван.
    pub fn revoke(&mut self, actor: UserId) -> Result<ApiKeyRevoked, OpenApiError> {
        if actor != self.owner {
            return Err(OpenApiError::NotOwner);
        }
        if !self.status.is_active() {
            return Err(OpenApiError::AlreadyRevoked);
        }
        self.status = ApiKeyStatus::Revoked;
        Ok(ApiKeyRevoked { key: self.id })
    }

    /// Восстановить агрегат из хранилища (`infrastructure`). Новый контекст — честный
    /// reconstitute, доменного хака не требуется.
    #[must_use]
    pub fn reconstitute(
        id: ApiKeyId,
        owner: UserId,
        label: ApiKeyLabel,
        hash: ApiKeyHash,
        status: ApiKeyStatus,
        created_at: Timestamp,
    ) -> Self {
        Self {
            id,
            owner,
            label,
            hash,
            status,
            created_at,
        }
    }

    #[must_use]
    pub const fn id(&self) -> ApiKeyId {
        self.id
    }
    #[must_use]
    pub const fn owner(&self) -> UserId {
        self.owner
    }
    #[must_use]
    pub fn label(&self) -> &ApiKeyLabel {
        &self.label
    }
    #[must_use]
    pub fn hash(&self) -> &ApiKeyHash {
        &self.hash
    }
    #[must_use]
    pub const fn status(&self) -> ApiKeyStatus {
        self.status
    }
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }
}

/// Событие: выпущен API-ключ.
#[derive(Debug, Clone)]
pub struct ApiKeyIssued {
    pub key: ApiKeyId,
    pub owner: UserId,
}

/// Событие: API-ключ отозван.
#[derive(Debug, Clone)]
pub struct ApiKeyRevoked {
    pub key: ApiKeyId,
}

/// Нарушение правил контекста openapi.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OpenApiError {
    /// Выпускать ключи может только верифицированный (ADR-0010).
    #[error("открытое API доступно только верифицированным")]
    NotVerified,
    /// Отозвать ключ может только его владелец.
    #[error("отозвать ключ может только владелец")]
    NotOwner,
    /// Ключ уже отозван.
    #[error("ключ уже отозван")]
    AlreadyRevoked,
}

/// Хранилище API-ключей. Поиск при аутентификации — по хэшу предъявленного токена.
/// Перечисление ключей владельца — на стороне read-модели (CQRS, ADR-0004).
#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
    async fn find_by_id(&self, id: ApiKeyId) -> Result<Option<ApiKey>, RepositoryError>;
    /// Найти ключ по хэшу секрета (для аутентификации `/api/v1`).
    async fn find_by_hash(&self, hash: &ApiKeyHash) -> Result<Option<ApiKey>, RepositoryError>;
    async fn save(&self, key: &ApiKey) -> Result<(), RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label() -> ApiKeyLabel {
        ApiKeyLabel::parse("ci-bot").unwrap()
    }
    fn hash() -> ApiKeyHash {
        ApiKeyHash::from_storage("sha256:deadbeef".to_owned())
    }

    fn issued(owner: UserId) -> ApiKey {
        ApiKey::issue(
            Id::generate(),
            owner,
            VerifiedStatus::Verified,
            label(),
            hash(),
            Timestamp::now(),
        )
        .expect("верифицированный выпускает")
        .0
    }

    #[test]
    fn token_validates_and_redacts() {
        assert!(ApiKeyToken::parse("bbg_0123456789abcd").is_ok());
        assert!(matches!(
            ApiKeyToken::parse("short"),
            Err(ApiKeyTokenError::WrongLength { .. })
        ));
        assert!(matches!(
            ApiKeyToken::parse(&format!("bbg_{}!", "a".repeat(20))),
            Err(ApiKeyTokenError::InvalidChar('!'))
        ));
        let t = ApiKeyToken::parse("bbg_0123456789abcd").unwrap();
        assert_eq!(format!("{t:?}"), "ApiKeyToken(***)");
    }

    #[test]
    fn label_validates() {
        assert_eq!(ApiKeyLabel::parse("  "), Err(ApiKeyLabelError::Empty));
        assert!(matches!(
            ApiKeyLabel::parse(&"x".repeat(81)),
            Err(ApiKeyLabelError::TooLong { .. })
        ));
    }

    #[test]
    fn casual_cannot_issue() {
        let err = ApiKey::issue(
            Id::generate(),
            Id::generate(),
            VerifiedStatus::Casual,
            label(),
            hash(),
            Timestamp::now(),
        )
        .unwrap_err();
        assert_eq!(err, OpenApiError::NotVerified);
    }

    #[test]
    fn verified_issues_active() {
        let owner = Id::generate();
        let (key, event) = ApiKey::issue(
            Id::generate(),
            owner,
            VerifiedStatus::Verified,
            label(),
            hash(),
            Timestamp::now(),
        )
        .unwrap();
        assert!(key.status().is_active());
        assert_eq!(key.owner(), owner);
        assert_eq!(event.owner, owner);
    }

    #[test]
    fn only_owner_revokes() {
        let owner = Id::generate();
        let mut key = issued(owner);
        assert_eq!(
            key.revoke(Id::generate()).unwrap_err(),
            OpenApiError::NotOwner
        );
        assert!(key.revoke(owner).is_ok());
        assert_eq!(key.status(), ApiKeyStatus::Revoked);
    }

    #[test]
    fn cannot_revoke_twice() {
        let owner = Id::generate();
        let mut key = issued(owner);
        key.revoke(owner).unwrap();
        assert_eq!(key.revoke(owner).unwrap_err(), OpenApiError::AlreadyRevoked);
    }
}
