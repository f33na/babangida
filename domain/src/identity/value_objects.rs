//! Value objects контекста identity. Инвариант — в конструкторе ([`Handle::parse`]
//! и т.п.): собрать невалидное значение невозможно.

use super::MAX_ACTIVE_INVITES;

/// Уникальный @-идентификатор юзера. Нормализуется в нижний регистр; 3..=20
/// символов, начинается с латинской буквы, дальше `[a-z0-9_]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Handle(String);

impl Handle {
    /// Минимальная длина handle.
    pub const MIN_LEN: usize = 3;
    /// Максимальная длина handle.
    pub const MAX_LEN: usize = 20;

    /// Распарсить и нормализовать handle.
    ///
    /// # Errors
    /// [`HandleError`], если длина или символы нарушают правила.
    pub fn parse(input: &str) -> Result<Self, HandleError> {
        let normalized = input.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Err(HandleError::Empty);
        }
        let len = normalized.chars().count();
        if len < Self::MIN_LEN {
            return Err(HandleError::TooShort { len });
        }
        if len > Self::MAX_LEN {
            return Err(HandleError::TooLong { len });
        }
        for (i, c) in normalized.chars().enumerate() {
            let ok = if i == 0 {
                c.is_ascii_lowercase()
            } else {
                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'
            };
            if !ok {
                return Err(if i == 0 {
                    HandleError::InvalidStart
                } else {
                    HandleError::InvalidChar(c)
                });
            }
        }
        Ok(Self(normalized))
    }

    /// Нормализованное строковое представление.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Нарушение правил [`Handle`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HandleError {
    #[error("handle пустой")]
    Empty,
    #[error("handle слишком короткий: {len} символов")]
    TooShort { len: usize },
    #[error("handle слишком длинный: {len} символов")]
    TooLong { len: usize },
    #[error("handle должен начинаться с латинской буквы")]
    InvalidStart,
    #[error("недопустимый символ в handle: {0:?}")]
    InvalidChar(char),
}

/// Код приглашения. 8..=32 символа, ASCII-буквенно-цифровой, регистрозависимый.
/// Сам код генерируется на границе (`application`) и валидируется здесь.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InviteCode(String);

impl InviteCode {
    /// Минимальная длина кода.
    pub const MIN_LEN: usize = 8;
    /// Максимальная длина кода.
    pub const MAX_LEN: usize = 32;

    /// Распарсить код приглашения.
    ///
    /// # Errors
    /// [`InviteCodeError`], если длина или символы нарушают правила.
    pub fn parse(input: &str) -> Result<Self, InviteCodeError> {
        let code = input.trim();
        let len = code.chars().count();
        if !(Self::MIN_LEN..=Self::MAX_LEN).contains(&len) {
            return Err(InviteCodeError::WrongLength { len });
        }
        if let Some(c) = code.chars().find(|c| !c.is_ascii_alphanumeric()) {
            return Err(InviteCodeError::InvalidChar(c));
        }
        Ok(Self(code.to_owned()))
    }

    /// Строковое представление кода.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Нарушение правил [`InviteCode`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InviteCodeError {
    #[error("неверная длина кода приглашения: {len} символов")]
    WrongLength { len: usize },
    #[error("недопустимый символ в коде приглашения: {0:?}")]
    InvalidChar(char),
}

/// Квота на активные инвайты. У обычного юзера ограничена, у админа — нет (ADR-0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InviteQuota {
    /// Ограничение на число одновременно активных инвайтов.
    Limited(u32),
    /// Без ограничения (админ).
    Unlimited,
}

impl InviteQuota {
    /// Можно ли выдать ещё один инвайт при текущем числе активных.
    #[must_use]
    pub const fn allows(self, active_count: u32) -> bool {
        match self {
            Self::Unlimited => true,
            Self::Limited(max) => active_count < max,
        }
    }

    /// Распространяется ли на эту квоту кулдаун выдачи (на админа — нет).
    #[must_use]
    pub const fn enforces_cooldown(self) -> bool {
        matches!(self, Self::Limited(_))
    }

    /// Числовой лимит, если он есть.
    #[must_use]
    pub const fn limit(self) -> Option<u32> {
        match self {
            Self::Limited(max) => Some(max),
            Self::Unlimited => None,
        }
    }
}

/// Статус верификации. Гейт привилегий (маркет, музыка, open API) — ADR-0010;
/// сами проверки гейтов выполняет `application`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VerifiedStatus {
    /// Обычный юзер: лента, профиль, переписка.
    #[default]
    Casual,
    /// Верифицированный: доступны привилегированные зоны.
    Verified,
}

impl VerifiedStatus {
    /// Верифицирован ли юзер.
    #[must_use]
    pub const fn is_verified(self) -> bool {
        matches!(self, Self::Verified)
    }
}

/// Роль юзера. Определяет квоту инвайтов.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UserRole {
    /// Рядовой участник.
    #[default]
    Member,
    /// Администратор — без лимита и кулдауна на инвайты (ADR-0005).
    Admin,
}

impl UserRole {
    /// Квота инвайтов для роли.
    #[must_use]
    pub const fn invite_quota(self) -> InviteQuota {
        match self {
            Self::Member => InviteQuota::Limited(MAX_ACTIVE_INVITES),
            Self::Admin => InviteQuota::Unlimited,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_normalizes_and_accepts_valid() {
        let h = Handle::parse("  FenFen_99 ").expect("валидный handle");
        assert_eq!(h.as_str(), "fenfen_99");
    }

    #[test]
    fn handle_rejects_bad_input() {
        assert_eq!(Handle::parse(""), Err(HandleError::Empty));
        assert_eq!(Handle::parse("ab"), Err(HandleError::TooShort { len: 2 }));
        assert!(matches!(
            Handle::parse("9abc"),
            Err(HandleError::InvalidStart)
        ));
        assert!(matches!(
            Handle::parse("ab-cd"),
            Err(HandleError::InvalidChar('-'))
        ));
        assert!(matches!(
            Handle::parse(&"a".repeat(21)),
            Err(HandleError::TooLong { len: 21 })
        ));
    }

    #[test]
    fn invite_code_validates_length_and_charset() {
        assert!(InviteCode::parse("ABCD1234").is_ok());
        assert_eq!(
            InviteCode::parse("short"),
            Err(InviteCodeError::WrongLength { len: 5 })
        );
        assert!(matches!(
            InviteCode::parse("ABCD-123"),
            Err(InviteCodeError::InvalidChar('-'))
        ));
    }

    #[test]
    fn quota_rules() {
        assert!(InviteQuota::Limited(2).allows(1));
        assert!(!InviteQuota::Limited(2).allows(2));
        assert!(InviteQuota::Unlimited.allows(999));
        assert!(InviteQuota::Limited(2).enforces_cooldown());
        assert!(!InviteQuota::Unlimited.enforces_cooldown());
    }

    #[test]
    fn role_maps_to_quota() {
        assert_eq!(
            UserRole::Member.invite_quota(),
            InviteQuota::Limited(MAX_ACTIVE_INVITES)
        );
        assert_eq!(UserRole::Admin.invite_quota(), InviteQuota::Unlimited);
    }
}
