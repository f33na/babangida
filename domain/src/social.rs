//! Контекст social: профиль юзера (display name, субкультура, био).

use async_trait::async_trait;

use crate::RepositoryError;
use crate::identity::UserId;

/// Отображаемое имя. 1..=50 символов после обрезки, без управляющих символов.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayName(String);

impl DisplayName {
    /// Максимальная длина.
    pub const MAX_LEN: usize = 50;

    /// Распарсить отображаемое имя.
    ///
    /// # Errors
    /// [`DisplayNameError`], если имя пустое, слишком длинное или содержит
    /// управляющие символы.
    pub fn parse(input: &str) -> Result<Self, DisplayNameError> {
        let name = input.trim();
        if name.is_empty() {
            return Err(DisplayNameError::Empty);
        }
        let len = name.chars().count();
        if len > Self::MAX_LEN {
            return Err(DisplayNameError::TooLong { len });
        }
        if name.chars().any(char::is_control) {
            return Err(DisplayNameError::ControlChar);
        }
        Ok(Self(name.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Нарушение правил [`DisplayName`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DisplayNameError {
    #[error("отображаемое имя пустое")]
    Empty,
    #[error("отображаемое имя слишком длинное: {len} символов")]
    TooLong { len: usize },
    #[error("отображаемое имя содержит управляющие символы")]
    ControlChar,
}

/// Субкультура профиля. Набор курируемый — расширяется в коде (ADR-0003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subculture {
    Underground,
    Casual,
    Skin,
    HipHop,
}

impl Subculture {
    /// Каноническое строковое представление.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Underground => "underground",
            Self::Casual => "casual",
            Self::Skin => "skin",
            Self::HipHop => "hiphop",
        }
    }

    /// Распарсить из строки (регистронезависимо).
    ///
    /// # Errors
    /// [`SubcultureError`], если значение не из известного набора.
    pub fn parse(input: &str) -> Result<Self, SubcultureError> {
        match input.trim().to_ascii_lowercase().as_str() {
            "underground" => Ok(Self::Underground),
            "casual" => Ok(Self::Casual),
            "skin" => Ok(Self::Skin),
            "hiphop" => Ok(Self::HipHop),
            _ => Err(SubcultureError),
        }
    }
}

/// Значение субкультуры не из известного набора.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("неизвестная субкультура")]
pub struct SubcultureError;

/// Био профиля. До 280 символов, непустое (отсутствие био — `None` в профиле).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bio(String);

impl Bio {
    /// Максимальная длина.
    pub const MAX_LEN: usize = 280;

    /// Распарсить био.
    ///
    /// # Errors
    /// [`BioError`], если оно пустое или длиннее [`Bio::MAX_LEN`].
    pub fn parse(input: &str) -> Result<Self, BioError> {
        let bio = input.trim();
        if bio.is_empty() {
            return Err(BioError::Empty);
        }
        let len = bio.chars().count();
        if len > Self::MAX_LEN {
            return Err(BioError::TooLong { len });
        }
        Ok(Self(bio.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Нарушение правил [`Bio`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BioError {
    #[error("био пустое")]
    Empty,
    #[error("био слишком длинное: {len} символов")]
    TooLong { len: usize },
}

/// Профиль юзера. Один-к-одному с юзером — идентичность по [`UserId`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    user_id: UserId,
    display_name: DisplayName,
    subculture: Subculture,
    bio: Option<Bio>,
}

impl Profile {
    /// Создать профиль (без био).
    #[must_use]
    pub fn create(user_id: UserId, display_name: DisplayName, subculture: Subculture) -> Self {
        Self {
            user_id,
            display_name,
            subculture,
            bio: None,
        }
    }

    pub fn rename(&mut self, display_name: DisplayName) {
        self.display_name = display_name;
    }

    pub fn set_subculture(&mut self, subculture: Subculture) {
        self.subculture = subculture;
    }

    pub fn set_bio(&mut self, bio: Option<Bio>) {
        self.bio = bio;
    }

    #[must_use]
    pub const fn user_id(&self) -> UserId {
        self.user_id
    }

    #[must_use]
    pub fn display_name(&self) -> &DisplayName {
        &self.display_name
    }

    #[must_use]
    pub const fn subculture(&self) -> Subculture {
        self.subculture
    }

    #[must_use]
    pub const fn bio(&self) -> Option<&Bio> {
        self.bio.as_ref()
    }
}

/// Хранилище профилей (порт; реализация — в `infrastructure`).
#[async_trait]
pub trait ProfileRepository: Send + Sync {
    async fn find_by_user(&self, user: UserId) -> Result<Option<Profile>, RepositoryError>;
    async fn save(&self, profile: &Profile) -> Result<(), RepositoryError>;
}

#[cfg(test)]
mod tests {
    use babangida_shared::Id;

    use super::*;

    #[test]
    fn display_name_trims_and_limits() {
        assert_eq!(DisplayName::parse("  Гуф  ").unwrap().as_str(), "Гуф");
        assert_eq!(DisplayName::parse("   "), Err(DisplayNameError::Empty));
        assert!(matches!(
            DisplayName::parse(&"я".repeat(51)),
            Err(DisplayNameError::TooLong { len: 51 })
        ));
    }

    #[test]
    fn subculture_roundtrips() {
        assert_eq!(
            Subculture::parse("UnderGround").unwrap(),
            Subculture::Underground
        );
        assert_eq!(Subculture::HipHop.as_str(), "hiphop");
        assert_eq!(Subculture::parse("jazz"), Err(SubcultureError));
    }

    #[test]
    fn profile_updates_fields() {
        let mut p = Profile::create(
            Id::generate(),
            DisplayName::parse("MC").unwrap(),
            Subculture::Underground,
        );
        assert!(p.bio().is_none());
        p.set_bio(Some(Bio::parse("из подвала").unwrap()));
        assert_eq!(p.bio().map(Bio::as_str), Some("из подвала"));
        p.set_subculture(Subculture::HipHop);
        assert_eq!(p.subculture(), Subculture::HipHop);
    }
}
