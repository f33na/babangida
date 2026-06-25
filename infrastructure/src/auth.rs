//! Postgres- и crypto-адаптеры контекста auth (ADR-0013): хэшер паролей (argon2id),
//! генератор токенов сессий (CSPRNG) и репозитории кредов/сессий. Случайность и
//! тяжёлая криптография — здесь, на границе; домен хранит только результат.

use argon2::Argon2;
use argon2::password_hash::{
    PasswordHash as Phc, PasswordHasher as _, PasswordVerifier as _, SaltString,
};
use async_trait::async_trait;
use babangida_application::{PasswordHasher, SessionTokenFactory};
use babangida_domain::RepositoryError;
use babangida_domain::auth::{
    Credential, CredentialRepository, Password, PasswordHash, Session, SessionRepository,
    SessionToken,
};
use babangida_domain::identity::UserId;
use babangida_shared::{Id, Timestamp};
use rand::Rng;
use rand::distributions::Alphanumeric;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::map_sqlx;
use crate::pool::Db;

/// Хэширование паролей argon2id (дефолтные параметры крейта). Соль — случайная на
/// каждый пароль; результат — PHC-строка, непрозрачная для домена.
pub struct Argon2PasswordHasher;

impl PasswordHasher for Argon2PasswordHasher {
    fn hash(&self, password: &Password) -> PasswordHash {
        let salt_bytes: [u8; 16] = rand::random();
        let salt = SaltString::encode_b64(&salt_bytes).expect("16 байт — валидная соль");
        let phc = Argon2::default()
            .hash_password(password.expose().as_bytes(), &salt)
            .expect("argon2: хэширование валидного пароля не падает")
            .to_string();
        PasswordHash::from_storage(phc)
    }

    fn verify(&self, password: &Password, hash: &PasswordHash) -> bool {
        // Повреждённый/чужой формат хэша → просто «не совпало», не паника.
        match Phc::new(hash.as_str()) {
            Ok(parsed) => Argon2::default()
                .verify_password(password.expose().as_bytes(), &parsed)
                .is_ok(),
            Err(_) => false,
        }
    }
}

/// Генератор токенов сессий: 48 ASCII-буквенно-цифровых символов из CSPRNG
/// (~285 бит). Алфавит — подмножество допустимого в [`SessionToken`].
pub struct RandomSessionTokenFactory;

impl SessionTokenFactory for RandomSessionTokenFactory {
    fn generate(&self) -> SessionToken {
        let raw: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(48)
            .map(char::from)
            .collect();
        SessionToken::parse(&raw).expect("48 буквенно-цифровых — валидный токен")
    }
}

/// Репозиторий учётных данных на Postgres.
pub struct PgCredentialRepository {
    db: Db,
}

impl PgCredentialRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl CredentialRepository for PgCredentialRepository {
    async fn find_by_user(&self, user: UserId) -> Result<Option<Credential>, RepositoryError> {
        let row: Option<(String, OffsetDateTime)> = sqlx::query_as(
            "SELECT password_hash, established_at FROM credentials WHERE user_id = $1",
        )
        .bind(user.as_uuid())
        .fetch_optional(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(row.map(|(hash, ts)| {
            Credential::establish(
                user,
                PasswordHash::from_storage(hash),
                Timestamp::from_offset(ts),
            )
        }))
    }

    async fn save(&self, credential: &Credential) -> Result<(), RepositoryError> {
        sqlx::query(
            "INSERT INTO credentials (user_id, password_hash, established_at) VALUES ($1, $2, $3) \
             ON CONFLICT (user_id) DO UPDATE SET password_hash = EXCLUDED.password_hash, \
             established_at = EXCLUDED.established_at",
        )
        .bind(credential.user().as_uuid())
        .bind(credential.hash().as_str())
        .bind(credential.established_at().into_offset())
        .execute(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }
}

/// Репозиторий сессий на Postgres.
pub struct PgSessionRepository {
    db: Db,
}

impl PgSessionRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl SessionRepository for PgSessionRepository {
    async fn find_by_token(
        &self,
        token: &SessionToken,
    ) -> Result<Option<Session>, RepositoryError> {
        let row: Option<(Uuid, Uuid, OffsetDateTime, OffsetDateTime)> = sqlx::query_as(
            "SELECT id, user_id, issued_at, expires_at FROM sessions WHERE token = $1",
        )
        .bind(token.as_str())
        .fetch_optional(&self.db)
        .await
        .map_err(map_sqlx)?;
        // Реконституция сессии через доменный API (patterns/repository): ttl
        // восстанавливаем как expires - issued, домен сам пересчитает истечение.
        Ok(row.map(|(id, user, issued, expires)| {
            let issued = Timestamp::from_offset(issued);
            let ttl = Timestamp::from_offset(expires).duration_since(issued);
            let (session, _event) = Session::issue(
                Id::from_uuid(id),
                token.clone(),
                Id::from_uuid(user),
                issued,
                ttl,
            );
            session
        }))
    }

    async fn save(&self, session: &Session) -> Result<(), RepositoryError> {
        sqlx::query(
            "INSERT INTO sessions (id, token, user_id, issued_at, expires_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(session.id().as_uuid())
        .bind(session.token().as_str())
        .bind(session.user().as_uuid())
        .bind(session.issued_at().into_offset())
        .bind(session.expires_at().into_offset())
        .execute(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    async fn delete(&self, token: &SessionToken) -> Result<(), RepositoryError> {
        sqlx::query("DELETE FROM sessions WHERE token = $1")
            .bind(token.as_str())
            .execute(&self.db)
            .await
            .map_err(map_sqlx)?;
        Ok(())
    }
}
