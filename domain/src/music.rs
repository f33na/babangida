//! Контекст music: релизы треков. Загрузка — привилегия верифицированных (ADR-0010):
//! гейт держим инвариантом домена ([`Track::release`] требует [`VerifiedStatus`]),
//! `application` лишь читает статус и передаёт сюда — как с маркетом
//! ([`crate::marketplace::Listing::list`], ADR-0011/0003).
//!
//! Аудио на MVP — внешняя ссылка ([`AudioRef`], URL): храним метаданные + локатор,
//! без хостинга байтов (ADR-0017). Анти-ВК: треки живут в профиле артиста и общем
//! разделе музыки внутри той же сети, не отдельным плеером.

use babangida_shared::{Id, Timestamp};

use crate::identity::{UserId, VerifiedStatus};

/// Название трека. 1..=200 символов.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackTitle(String);

impl TrackTitle {
    /// Максимальная длина названия.
    pub const MAX_LEN: usize = 200;

    /// Распарсить название.
    ///
    /// # Errors
    /// [`TrackTitleError`], если пусто или длиннее [`MAX_LEN`](Self::MAX_LEN).
    pub fn parse(input: &str) -> Result<Self, TrackTitleError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(TrackTitleError::Empty);
        }
        let len = trimmed.chars().count();
        if len > Self::MAX_LEN {
            return Err(TrackTitleError::TooLong { len });
        }
        Ok(Self(trimmed.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Нарушение правил [`TrackTitle`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TrackTitleError {
    #[error("название трека пустое")]
    Empty,
    #[error("название трека слишком длинное: {len} символов (максимум 200)")]
    TooLong { len: usize },
}

/// Локатор аудио. На MVP (ADR-0017) — внешняя ссылка (http/https URL): домен не
/// хостит байты, лишь хранит проверенный URL. Замена на объектное хранилище позже
/// не затронет домен (тип остаётся непрозрачным локатором).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioRef(String);

impl AudioRef {
    /// Максимальная длина URL.
    pub const MAX_LEN: usize = 2048;

    /// Распарсить ссылку на аудио. Лёгкая валидация (схема http/https, есть хост, без
    /// пробелов) — без URL-крейта, чтобы домен оставался без зависимостей.
    ///
    /// # Errors
    /// [`AudioRefError`], если пусто, слишком длинно, не http(s) или без хоста.
    pub fn parse(input: &str) -> Result<Self, AudioRefError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(AudioRefError::Empty);
        }
        if trimmed.chars().count() > Self::MAX_LEN {
            return Err(AudioRefError::TooLong);
        }
        if trimmed.chars().any(char::is_whitespace) {
            return Err(AudioRefError::Malformed);
        }
        let host = trimmed
            .strip_prefix("https://")
            .or_else(|| trimmed.strip_prefix("http://"))
            .ok_or(AudioRefError::NotHttp)?;
        // Хост непуст и есть хотя бы один разделитель (точка) — отсекаем «http://».
        if host.is_empty() || !host.contains('.') {
            return Err(AudioRefError::Malformed);
        }
        Ok(Self(trimmed.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Нарушение правил [`AudioRef`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AudioRefError {
    #[error("ссылка на аудио пустая")]
    Empty,
    #[error("ссылка на аудио слишком длинная")]
    TooLong,
    #[error("ссылка на аудио должна начинаться с http:// или https://")]
    NotHttp,
    #[error("ссылка на аудио некорректна")]
    Malformed,
}

/// Жанр трека (опц. метка: boom bap, drill, trap…). 1..=60 символов.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Genre(String);

impl Genre {
    /// Максимальная длина жанра.
    pub const MAX_LEN: usize = 60;

    /// Распарсить жанр.
    ///
    /// # Errors
    /// [`GenreError`], если пусто или длиннее [`MAX_LEN`](Self::MAX_LEN).
    pub fn parse(input: &str) -> Result<Self, GenreError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(GenreError::Empty);
        }
        let len = trimmed.chars().count();
        if len > Self::MAX_LEN {
            return Err(GenreError::TooLong { len });
        }
        Ok(Self(trimmed.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Нарушение правил [`Genre`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GenreError {
    #[error("жанр пустой")]
    Empty,
    #[error("жанр слишком длинный: {len} символов (максимум 60)")]
    TooLong { len: usize },
}

/// Черновик трека — то, что задаёт артист (без id/времени/статуса).
#[derive(Debug, Clone)]
pub struct TrackDraft {
    pub title: TrackTitle,
    pub audio: AudioRef,
    pub genre: Option<Genre>,
}

/// Статус трека.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackStatus {
    /// Опубликован — виден в ленте/разделе/профиле.
    Published,
    /// Снят артистом.
    Withdrawn,
}

impl TrackStatus {
    /// Опубликован ли (можно снять).
    #[must_use]
    pub const fn is_published(self) -> bool {
        matches!(self, Self::Published)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Published => "published",
            Self::Withdrawn => "withdrawn",
        }
    }
}

/// Фантомный маркер для [`TrackId`].
pub enum TrackMarker {}
/// Идентификатор трека.
pub type TrackId = Id<TrackMarker>;

/// Трек — корень агрегата music. Релизит только верифицированный артист; снять может
/// только он.
#[derive(Debug, Clone)]
pub struct Track {
    id: TrackId,
    uploader: UserId,
    title: TrackTitle,
    audio: AudioRef,
    genre: Option<Genre>,
    status: TrackStatus,
    created_at: Timestamp,
}

impl Track {
    /// Выпустить трек. Гейт верификации (ADR-0010): артист обязан быть верифицирован —
    /// статус читает `application`, инвариант держит домен.
    ///
    /// # Errors
    /// [`MusicError::NotVerified`], если артист не верифицирован.
    pub fn release(
        id: TrackId,
        uploader: UserId,
        verified: VerifiedStatus,
        draft: TrackDraft,
        now: Timestamp,
    ) -> Result<(Self, TrackReleased), MusicError> {
        if !verified.is_verified() {
            return Err(MusicError::NotVerified);
        }
        let track = Self {
            id,
            uploader,
            title: draft.title,
            audio: draft.audio,
            genre: draft.genre,
            status: TrackStatus::Published,
            created_at: now,
        };
        let event = TrackReleased {
            track: id,
            uploader,
        };
        Ok((track, event))
    }

    /// Снять трек (только артист, только из опубликованного).
    ///
    /// # Errors
    /// [`MusicError`]: не артист или трек уже снят.
    pub fn withdraw(&mut self, actor: UserId) -> Result<TrackWithdrawn, MusicError> {
        if actor != self.uploader {
            return Err(MusicError::NotUploader);
        }
        if !self.status.is_published() {
            return Err(MusicError::NotPublished);
        }
        self.status = TrackStatus::Withdrawn;
        Ok(TrackWithdrawn { track: self.id })
    }

    /// Восстановить агрегат из хранилища (`infrastructure`). Новый контекст — честный
    /// reconstitute, доменного хака не требуется.
    #[must_use]
    pub fn reconstitute(
        id: TrackId,
        uploader: UserId,
        title: TrackTitle,
        audio: AudioRef,
        genre: Option<Genre>,
        status: TrackStatus,
        created_at: Timestamp,
    ) -> Self {
        Self {
            id,
            uploader,
            title,
            audio,
            genre,
            status,
            created_at,
        }
    }

    #[must_use]
    pub const fn id(&self) -> TrackId {
        self.id
    }
    #[must_use]
    pub const fn uploader(&self) -> UserId {
        self.uploader
    }
    #[must_use]
    pub fn title(&self) -> &TrackTitle {
        &self.title
    }
    #[must_use]
    pub fn audio(&self) -> &AudioRef {
        &self.audio
    }
    #[must_use]
    pub fn genre(&self) -> Option<&Genre> {
        self.genre.as_ref()
    }
    #[must_use]
    pub const fn status(&self) -> TrackStatus {
        self.status
    }
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }
}

/// Событие: трек выпущен.
#[derive(Debug, Clone)]
pub struct TrackReleased {
    pub track: TrackId,
    pub uploader: UserId,
}

/// Событие: трек снят.
#[derive(Debug, Clone)]
pub struct TrackWithdrawn {
    pub track: TrackId,
}

/// Нарушение правил контекста music.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MusicError {
    /// Релизить может только верифицированный (ADR-0010).
    #[error("загрузка музыки доступна только верифицированным")]
    NotVerified,
    /// Менять трек может только его автор.
    #[error("действие доступно только автору трека")]
    NotUploader,
    /// Трек уже снят.
    #[error("трек уже снят")]
    NotPublished,
}

/// Хранилище треков.
#[async_trait::async_trait]
pub trait TrackRepository: Send + Sync {
    async fn find_by_id(&self, id: TrackId) -> Result<Option<Track>, crate::RepositoryError>;
    async fn save(&self, track: &Track) -> Result<(), crate::RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> TrackDraft {
        TrackDraft {
            title: TrackTitle::parse("Подвал").unwrap(),
            audio: AudioRef::parse("https://audio.example/track.mp3").unwrap(),
            genre: Some(Genre::parse("boom bap").unwrap()),
        }
    }

    fn published(uploader: UserId) -> Track {
        Track::release(
            Id::generate(),
            uploader,
            VerifiedStatus::Verified,
            draft(),
            Timestamp::now(),
        )
        .expect("верифицированный релизит")
        .0
    }

    #[test]
    fn title_and_audio_validate() {
        assert_eq!(TrackTitle::parse("  "), Err(TrackTitleError::Empty));
        assert!(matches!(
            TrackTitle::parse(&"x".repeat(201)),
            Err(TrackTitleError::TooLong { .. })
        ));
        assert_eq!(AudioRef::parse(""), Err(AudioRefError::Empty));
        assert_eq!(
            AudioRef::parse("ftp://host.x/a"),
            Err(AudioRefError::NotHttp)
        );
        assert_eq!(AudioRef::parse("https://"), Err(AudioRefError::Malformed));
        assert_eq!(
            AudioRef::parse("https://no spaces.x/a"),
            Err(AudioRefError::Malformed)
        );
        assert_eq!(
            AudioRef::parse("https://host.example/a.mp3")
                .unwrap()
                .as_str(),
            "https://host.example/a.mp3"
        );
    }

    #[test]
    fn casual_cannot_release() {
        let err = Track::release(
            Id::generate(),
            Id::generate(),
            VerifiedStatus::Casual,
            draft(),
            Timestamp::now(),
        )
        .unwrap_err();
        assert_eq!(err, MusicError::NotVerified);
    }

    #[test]
    fn verified_releases_published() {
        let uploader = Id::generate();
        let (track, event) = Track::release(
            Id::generate(),
            uploader,
            VerifiedStatus::Verified,
            draft(),
            Timestamp::now(),
        )
        .unwrap();
        assert!(track.status().is_published());
        assert_eq!(track.uploader(), uploader);
        assert_eq!(event.uploader, uploader);
    }

    #[test]
    fn only_uploader_withdraws() {
        let uploader = Id::generate();
        let mut track = published(uploader);
        assert_eq!(
            track.withdraw(Id::generate()).unwrap_err(),
            MusicError::NotUploader
        );
        assert!(track.withdraw(uploader).is_ok());
        assert_eq!(track.status(), TrackStatus::Withdrawn);
    }

    #[test]
    fn cannot_withdraw_twice() {
        let uploader = Id::generate();
        let mut track = published(uploader);
        track.withdraw(uploader).unwrap();
        assert_eq!(
            track.withdraw(uploader).unwrap_err(),
            MusicError::NotPublished
        );
    }
}
