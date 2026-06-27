//! Postgres- и crypto-адаптеры контекста openapi (ADR-0018): репозиторий ключей,
//! read-модель списка, генератор секрета (CSPRNG) и хэшер (SHA-256). Случайность и
//! криптография — здесь, на границе; домен хранит только хэш. Реконституция — через
//! честный `ApiKey::reconstitute`.

use async_trait::async_trait;
use babangida_application::query::{ApiKeyReadModel, ApiKeyView};
use babangida_application::{ApiKeyFactory, ApiKeyHasher};
use babangida_domain::RepositoryError;
use babangida_domain::identity::UserId;
use babangida_domain::openapi::{
    ApiKey, ApiKeyHash, ApiKeyId, ApiKeyLabel, ApiKeyRepository, ApiKeyStatus, ApiKeyToken,
};
use babangida_shared::{Id, Timestamp};
use rand::Rng;
use rand::distributions::Alphanumeric;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::map_sqlx;
use crate::pool::Db;

fn corrupt(what: &str) -> RepositoryError {
    RepositoryError::Unavailable(format!("повреждённый API-ключ в БД: {what}"))
}

fn parse_status(raw: &str) -> Result<ApiKeyStatus, RepositoryError> {
    match raw {
        "active" => Ok(ApiKeyStatus::Active),
        "revoked" => Ok(ApiKeyStatus::Revoked),
        other => Err(corrupt(&format!("неизвестный статус {other}"))),
    }
}

/// Генератор секрета ключа: префикс `bbg_` + 40 ASCII-буквенно-цифровых из CSPRNG
/// (~238 бит). Префикс — чтобы ключ узнавался; алфавит допустим в [`ApiKeyToken`].
pub struct RandomApiKeyFactory;

impl ApiKeyFactory for RandomApiKeyFactory {
    fn generate(&self) -> ApiKeyToken {
        let suffix: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(40)
            .map(char::from)
            .collect();
        ApiKeyToken::parse(&format!("bbg_{suffix}")).expect("bbg_ + 40 алфанум — валидный токен")
    }
}

/// Хэшер ключей SHA-256 (hex). Быстрый детерминированный хэш: ключ высокоэнтропийный,
/// медленный argon2 (для паролей) не нужен, а поиск по хэшу — на каждом запросе.
pub struct Sha256ApiKeyHasher;

impl ApiKeyHasher for Sha256ApiKeyHasher {
    fn hash(&self, token: &ApiKeyToken) -> ApiKeyHash {
        let digest = Sha256::digest(token.as_str().as_bytes());
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        ApiKeyHash::from_storage(hex)
    }
}

/// Строка ключа из БД (без owner — он в WHERE/SELECT по контексту).
type KeyRow = (Uuid, String, String, String, OffsetDateTime);

fn reconstitute_key(id: Uuid, row: KeyRow) -> Result<ApiKey, RepositoryError> {
    let (owner, label, key_hash, status, created_at) = row;
    Ok(ApiKey::reconstitute(
        Id::from_uuid(id),
        Id::from_uuid(owner),
        ApiKeyLabel::parse(&label).map_err(|_| corrupt("метка"))?,
        ApiKeyHash::from_storage(key_hash),
        parse_status(&status)?,
        Timestamp::from_offset(created_at),
    ))
}

/// Репозиторий API-ключей на Postgres.
pub struct PgApiKeyRepository {
    db: Db,
}

impl PgApiKeyRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ApiKeyRepository for PgApiKeyRepository {
    async fn find_by_id(&self, id: ApiKeyId) -> Result<Option<ApiKey>, RepositoryError> {
        let row: Option<KeyRow> = sqlx::query_as(
            "SELECT owner_id, label, key_hash, status, created_at FROM api_keys WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.db)
        .await
        .map_err(map_sqlx)?;
        row.map(|r| reconstitute_key(id.as_uuid(), r)).transpose()
    }

    async fn find_by_hash(&self, hash: &ApiKeyHash) -> Result<Option<ApiKey>, RepositoryError> {
        let row: Option<(Uuid, Uuid, String, String, String, OffsetDateTime)> = sqlx::query_as(
            "SELECT id, owner_id, label, key_hash, status, created_at FROM api_keys \
             WHERE key_hash = $1",
        )
        .bind(hash.as_str())
        .fetch_optional(&self.db)
        .await
        .map_err(map_sqlx)?;
        row.map(|(id, owner, label, key_hash, status, created_at)| {
            reconstitute_key(id, (owner, label, key_hash, status, created_at))
        })
        .transpose()
    }

    async fn save(&self, key: &ApiKey) -> Result<(), RepositoryError> {
        // Изменяемое поле после выпуска — только статус (отзыв).
        sqlx::query(
            "INSERT INTO api_keys (id, owner_id, label, key_hash, status, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (id) DO UPDATE SET status = EXCLUDED.status",
        )
        .bind(key.id().as_uuid())
        .bind(key.owner().as_uuid())
        .bind(key.label().as_str())
        .bind(key.hash().as_str())
        .bind(key.status().as_str())
        .bind(key.created_at().into_offset())
        .execute(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }
}

/// Read-модель ключей владельца (ADR-0004). Секрет не отдаётся.
pub struct PgApiKeyReadModel {
    db: Db,
}

impl PgApiKeyReadModel {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ApiKeyReadModel for PgApiKeyReadModel {
    async fn by_owner(&self, owner: UserId) -> Result<Vec<ApiKeyView>, RepositoryError> {
        let rows: Vec<(Uuid, String, String, OffsetDateTime)> = sqlx::query_as(
            "SELECT id, label, status, created_at FROM api_keys \
             WHERE owner_id = $1 ORDER BY created_at DESC, id DESC",
        )
        .bind(owner.as_uuid())
        .fetch_all(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(rows
            .into_iter()
            .map(|(id, label, status, created_at)| ApiKeyView {
                key_id: Id::from_uuid(id),
                label,
                status,
                created_at: Timestamp::from_offset(created_at),
            })
            .collect())
    }
}
