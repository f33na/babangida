//! Контекст auth: учётные данные и сессии. Отдельный от identity: identity — это
//! «кто ты» (`User`/`Handle`), auth — это «как ты это доказываешь» (пароль и
//! сессия). Поэтому замороженный агрегат [`crate::identity::User`] не трогаем.
//!
//! Чистый домен (ADR-0003): хэширование пароля и генерация токена — операции
//! недетерминированные и тяжёлые, их место на границе (`application`/`infrastructure`),
//! сюда приходит уже готовый [`PasswordHash`]/[`SessionToken`]. Домен хранит
//! инварианты: политику пароля, срок жизни сессии, формат токена. Модель — ADR-0013.

use async_trait::async_trait;
use babangida_shared::{Duration, Id, Timestamp};

use crate::RepositoryError;
use crate::identity::UserId;

/// Сырой пароль с границы. Никогда не сохраняется и не логируется (Debug скрыт);
/// уходит только в [`PasswordHasher`](crate). Инвариант длины — в [`Password::parse`].
#[derive(Clone)]
pub struct Password(String);

impl Password {
    /// Минимальная длина пароля.
    pub const MIN_LEN: usize = 8;
    /// Максимальная длина пароля (защита от DoS на хэшировании).
    pub const MAX_LEN: usize = 200;

    /// Проверить политику пароля. Пробелы не обрезаем — они часть пароля.
    ///
    /// # Errors
    /// [`PasswordError`], если длина вне допустимого диапазона.
    pub fn parse(input: &str) -> Result<Self, PasswordError> {
        let len = input.chars().count();
        if len < Self::MIN_LEN {
            return Err(PasswordError::TooShort { len });
        }
        if len > Self::MAX_LEN {
            return Err(PasswordError::TooLong { len });
        }
        Ok(Self(input.to_owned()))
    }

    /// Открыть значение для хэширования. Зовётся только адаптером-хэшером.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

// Пароль не должен утечь в логи/трейсы.
impl core::fmt::Debug for Password {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Password(***)")
    }
}

/// Нарушение политики пароля.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PasswordError {
    #[error("пароль слишком короткий: {len} символов (минимум 8)")]
    TooShort { len: usize },
    #[error("пароль слишком длинный: {len} символов (максимум 200)")]
    TooLong { len: usize },
}

/// Хэш пароля (например PHC-строка argon2). Для домена — непрозрачное значение:
/// его единственный легитимный источник — [`PasswordHasher`](crate). Debug скрыт.
#[derive(Clone, PartialEq, Eq)]
pub struct PasswordHash(String);

impl PasswordHash {
    /// Обернуть готовый хэш (из адаптера-хэшера или из БД при реконституции).
    #[must_use]
    pub fn from_storage(value: String) -> Self {
        Self(value)
    }

    /// Строковое представление для сохранения/сверки.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Debug for PasswordHash {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("PasswordHash(***)")
    }
}

/// Непрозрачный секрет сессии, который держит клиент (кука/Bearer). Высокоэнтропийный,
/// генерируется на границе ([`SessionTokenFactory`](crate)). Debug скрыт — это секрет.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SessionToken(String);

impl SessionToken {
    /// Минимальная длина (>= 32 символа URL-safe base64 ≈ 192 бита).
    pub const MIN_LEN: usize = 32;
    /// Максимальная длина — отсекаем мусор на входе.
    pub const MAX_LEN: usize = 128;

    /// Распарсить токен (с куки/заголовка или от фабрики). URL-safe алфавит
    /// (`A-Za-z0-9-_`), длина в пределах [`MIN_LEN`](Self::MIN_LEN)..=[`MAX_LEN`](Self::MAX_LEN).
    ///
    /// # Errors
    /// [`SessionTokenError`], если длина или символы нарушают правила.
    pub fn parse(input: &str) -> Result<Self, SessionTokenError> {
        let len = input.chars().count();
        if !(Self::MIN_LEN..=Self::MAX_LEN).contains(&len) {
            return Err(SessionTokenError::WrongLength { len });
        }
        if let Some(c) = input
            .chars()
            .find(|c| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '_'))
        {
            return Err(SessionTokenError::InvalidChar(c));
        }
        Ok(Self(input.to_owned()))
    }

    /// Строковое представление токена.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SessionToken(***)")
    }
}

/// Нарушение правил [`SessionToken`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SessionTokenError {
    #[error("неверная длина токена сессии: {len} символов")]
    WrongLength { len: usize },
    #[error("недопустимый символ в токене сессии: {0:?}")]
    InvalidChar(char),
}

/// Учётные данные юзера. Идентичность — по [`UserId`] (одна запись на юзера).
/// Хранит только хэш — сырого пароля в домене нет.
#[derive(Debug, Clone)]
pub struct Credential {
    user: UserId,
    hash: PasswordHash,
    established_at: Timestamp,
}

impl Credential {
    /// Завести/перезаписать учётные данные. Хэш приходит готовым с границы.
    #[must_use]
    pub fn establish(user: UserId, hash: PasswordHash, now: Timestamp) -> Self {
        Self {
            user,
            hash,
            established_at: now,
        }
    }

    #[must_use]
    pub const fn user(&self) -> UserId {
        self.user
    }

    #[must_use]
    pub fn hash(&self) -> &PasswordHash {
        &self.hash
    }

    #[must_use]
    pub const fn established_at(&self) -> Timestamp {
        self.established_at
    }
}

/// Фантомный маркер для [`SessionId`].
pub enum SessionMarker {}
/// Идентификатор сессии (строка в хранилище).
pub type SessionId = Id<SessionMarker>;

/// Срок жизни сессии по умолчанию.
pub const SESSION_TTL: Duration = Duration::days(30);

/// Сессия — выпущенный логином доступ. Идентичность по [`SessionId`]; клиент
/// предъявляет [`SessionToken`]. Истечение — доменное правило ([`Session::is_active`]).
#[derive(Debug, Clone)]
pub struct Session {
    id: SessionId,
    token: SessionToken,
    user: UserId,
    issued_at: Timestamp,
    expires_at: Timestamp,
}

impl Session {
    /// Выпустить сессию на срок `ttl` от `now`. Идентификатор и токен приходят
    /// с границы; домен только фиксирует момент истечения.
    #[must_use]
    pub fn issue(
        id: SessionId,
        token: SessionToken,
        user: UserId,
        now: Timestamp,
        ttl: Duration,
    ) -> (Self, SessionIssued) {
        let expires_at = now + ttl;
        let session = Self {
            id,
            token,
            user,
            issued_at: now,
            expires_at,
        };
        let event = SessionIssued {
            session: id,
            user,
            expires_at,
        };
        (session, event)
    }

    /// Активна ли сессия в момент `now` (ещё не истекла).
    #[must_use]
    pub fn is_active(&self, now: Timestamp) -> bool {
        now < self.expires_at
    }

    #[must_use]
    pub const fn id(&self) -> SessionId {
        self.id
    }

    #[must_use]
    pub fn token(&self) -> &SessionToken {
        &self.token
    }

    #[must_use]
    pub const fn user(&self) -> UserId {
        self.user
    }

    #[must_use]
    pub const fn issued_at(&self) -> Timestamp {
        self.issued_at
    }

    #[must_use]
    pub const fn expires_at(&self) -> Timestamp {
        self.expires_at
    }
}

/// Событие: выпущена сессия.
#[derive(Debug, Clone)]
pub struct SessionIssued {
    pub session: SessionId,
    pub user: UserId,
    pub expires_at: Timestamp,
}

/// Ошибка контекста auth. Намеренно не различает «нет юзера»/«неверный пароль»
/// (защита от перебора): на оба случая — [`InvalidCredentials`](Self::InvalidCredentials).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    /// Логин не прошёл: нет такого юзера или неверный пароль.
    #[error("неверные учётные данные")]
    InvalidCredentials,
    /// Нет валидной сессии (токена нет, он невалиден или истёк).
    #[error("требуется аутентификация")]
    Unauthenticated,
}

/// Хранилище учётных данных.
#[async_trait]
pub trait CredentialRepository: Send + Sync {
    async fn find_by_user(&self, user: UserId) -> Result<Option<Credential>, RepositoryError>;
    /// Сохранить (создать или перезаписать) учётные данные юзера.
    async fn save(&self, credential: &Credential) -> Result<(), RepositoryError>;
}

/// Хранилище сессий. Поиск и удаление — по токену, который предъявляет клиент.
#[async_trait]
pub trait SessionRepository: Send + Sync {
    async fn find_by_token(&self, token: &SessionToken)
    -> Result<Option<Session>, RepositoryError>;
    async fn save(&self, session: &Session) -> Result<(), RepositoryError>;
    /// Удалить сессию по токену (logout). Идемпотентно: нет токена — не ошибка.
    async fn delete(&self, token: &SessionToken) -> Result<(), RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> UserId {
        Id::generate()
    }

    #[test]
    fn password_enforces_length() {
        assert!(matches!(
            Password::parse("short"),
            Err(PasswordError::TooShort { len: 5 })
        ));
        assert!(Password::parse("longenough").is_ok());
        assert!(matches!(
            Password::parse(&"x".repeat(201)),
            Err(PasswordError::TooLong { len: 201 })
        ));
    }

    #[test]
    fn password_debug_is_redacted() {
        let p = Password::parse("supersecret").unwrap();
        assert_eq!(format!("{p:?}"), "Password(***)");
        assert_eq!(p.expose(), "supersecret");
    }

    #[test]
    fn session_token_validates_charset_and_length() {
        assert!(SessionToken::parse(&"a".repeat(40)).is_ok());
        assert!(matches!(
            SessionToken::parse("tooshort"),
            Err(SessionTokenError::WrongLength { .. })
        ));
        assert!(matches!(
            SessionToken::parse(&format!("{}!", "a".repeat(39))),
            Err(SessionTokenError::InvalidChar('!'))
        ));
    }

    #[test]
    fn session_token_debug_is_redacted() {
        let t = SessionToken::parse(&"a".repeat(40)).unwrap();
        assert_eq!(format!("{t:?}"), "SessionToken(***)");
    }

    #[test]
    fn session_active_until_expiry() {
        let now = Timestamp::now();
        let token = SessionToken::parse(&"a".repeat(40)).unwrap();
        let (session, event) = Session::issue(Id::generate(), token, user(), now, SESSION_TTL);
        assert!(session.is_active(now));
        assert!(session.is_active(now + Duration::days(29)));
        assert!(!session.is_active(now + SESSION_TTL));
        assert!(!session.is_active(now + Duration::days(31)));
        assert_eq!(event.expires_at, now + SESSION_TTL);
    }

    #[test]
    fn credential_keeps_hash_not_password() {
        let now = Timestamp::now();
        let cred = Credential::establish(
            user(),
            PasswordHash::from_storage("phc$...".to_owned()),
            now,
        );
        assert_eq!(cred.hash().as_str(), "phc$...");
        assert_eq!(cred.established_at(), now);
    }
}
