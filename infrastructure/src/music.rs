//! Postgres-адаптеры контекста music (ADR-0017): репозиторий треков и read-модели
//! раздела музыки/профиля. Реконституция — через честный `Track::reconstitute`
//! (новый контекст, не замороженный). Аудио — внешняя ссылка (`audio_url`), MVP.

use async_trait::async_trait;
use babangida_application::query::{MusicReadModel, TrackView};
use babangida_domain::RepositoryError;
use babangida_domain::music::{
    AudioRef, Genre, Track, TrackId, TrackRepository, TrackStatus, TrackTitle,
};
use babangida_shared::{Id, Timestamp};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::map_sqlx;
use crate::pool::Db;

fn corrupt(what: &str) -> RepositoryError {
    RepositoryError::Unavailable(format!("повреждённый трек в БД: {what}"))
}

fn parse_status(raw: &str) -> Result<TrackStatus, RepositoryError> {
    match raw {
        "published" => Ok(TrackStatus::Published),
        "withdrawn" => Ok(TrackStatus::Withdrawn),
        other => Err(corrupt(&format!("неизвестный статус {other}"))),
    }
}

/// Строка трека из БД (без handle артиста).
type TrackRow = (Uuid, String, String, Option<String>, String, OffsetDateTime);

fn reconstitute_track(id: Uuid, row: TrackRow) -> Result<Track, RepositoryError> {
    let (uploader, title, audio, genre, status, created_at) = row;
    Ok(Track::reconstitute(
        Id::from_uuid(id),
        Id::from_uuid(uploader),
        TrackTitle::parse(&title).map_err(|_| corrupt("название"))?,
        AudioRef::parse(&audio).map_err(|_| corrupt("ссылка"))?,
        genre
            .map(|g| Genre::parse(&g))
            .transpose()
            .map_err(|_| corrupt("жанр"))?,
        parse_status(&status)?,
        Timestamp::from_offset(created_at),
    ))
}

/// Репозиторий треков на Postgres.
pub struct PgTrackRepository {
    db: Db,
}

impl PgTrackRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl TrackRepository for PgTrackRepository {
    async fn find_by_id(&self, id: TrackId) -> Result<Option<Track>, RepositoryError> {
        let row: Option<TrackRow> = sqlx::query_as(
            "SELECT uploader_id, title, audio_url, genre, status, created_at \
             FROM tracks WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.db)
        .await
        .map_err(map_sqlx)?;
        row.map(|r| reconstitute_track(id.as_uuid(), r)).transpose()
    }

    async fn save(&self, track: &Track) -> Result<(), RepositoryError> {
        // Изменяемое поле после релиза — только статус (снятие).
        sqlx::query(
            "INSERT INTO tracks (id, uploader_id, title, audio_url, genre, status, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT (id) DO UPDATE SET status = EXCLUDED.status",
        )
        .bind(track.id().as_uuid())
        .bind(track.uploader().as_uuid())
        .bind(track.title().as_str())
        .bind(track.audio().as_str())
        .bind(track.genre().map(Genre::as_str))
        .bind(track.status().as_str())
        .bind(track.created_at().into_offset())
        .execute(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }
}

/// Строка трека с handle артиста (для read-моделей).
type TrackViewRow = (
    Uuid,
    Uuid,
    String,
    String,
    String,
    Option<String>,
    String,
    OffsetDateTime,
);

fn row_to_view(row: TrackViewRow) -> TrackView {
    let (id, uploader, artist_handle, title, audio_url, genre, status, created_at) = row;
    TrackView {
        track_id: Id::from_uuid(id),
        uploader: Id::from_uuid(uploader),
        artist_handle,
        title,
        audio_url,
        genre,
        status,
        created_at: Timestamp::from_offset(created_at),
    }
}

/// Read-модель треков: общий раздел и треки артиста (ADR-0004). Публично видны
/// только опубликованные; снятые из выдачи уходят.
pub struct PgMusicReadModel {
    db: Db,
}

impl PgMusicReadModel {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl MusicReadModel for PgMusicReadModel {
    async fn recent(&self, limit: u32) -> Result<Vec<TrackView>, RepositoryError> {
        let rows: Vec<TrackViewRow> = sqlx::query_as(
            "SELECT t.id, t.uploader_id, u.handle, t.title, t.audio_url, t.genre, t.status, t.created_at \
             FROM tracks t JOIN users u ON u.id = t.uploader_id \
             WHERE t.status = 'published' \
             ORDER BY t.created_at DESC, t.id DESC LIMIT $1",
        )
        .bind(i64::from(limit))
        .fetch_all(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(rows.into_iter().map(row_to_view).collect())
    }

    async fn by_artist(&self, handle: &str, limit: u32) -> Result<Vec<TrackView>, RepositoryError> {
        let rows: Vec<TrackViewRow> = sqlx::query_as(
            "SELECT t.id, t.uploader_id, u.handle, t.title, t.audio_url, t.genre, t.status, t.created_at \
             FROM tracks t JOIN users u ON u.id = t.uploader_id \
             WHERE u.handle = $1 AND t.status = 'published' \
             ORDER BY t.created_at DESC, t.id DESC LIMIT $2",
        )
        .bind(handle)
        .bind(i64::from(limit))
        .fetch_all(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(rows.into_iter().map(row_to_view).collect())
    }
}
