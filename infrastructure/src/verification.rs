//! Postgres-адаптеры контекста verification (ADR-0016): репозиторий заявок,
//! read-модель (очередь админа + статус заявки) и атомарная транзакция решения
//! (блокировка строки заявки; одобрение пишет статус юзера в той же транзакции).
//! Реконституция — через честный `VerificationRequest::reconstitute` (новый контекст,
//! не замороженный — доменного хака не требуется, ADR-0016).

use async_trait::async_trait;
use babangida_application::query::{
    MyVerificationView, VerificationReadModel, VerificationRequestView,
};
use babangida_application::{VerificationDecisionTx, VerificationDecisionTxFactory};
use babangida_domain::RepositoryError;
use babangida_domain::identity::{User, UserId};
use babangida_domain::verification::{
    DecisionReason, DecisionRecord, RequestNote, RequestStatus, VerificationRequest,
    VerificationRequestId, VerificationRequestRepository,
};
use babangida_shared::{Id, Timestamp};
use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::map_sqlx;
use crate::identity::row_to_user;
use crate::pool::Db;

fn corrupt(what: &str) -> RepositoryError {
    RepositoryError::Unavailable(format!("повреждённая заявка верификации: {what}"))
}

fn parse_status(raw: &str) -> Result<RequestStatus, RepositoryError> {
    match raw {
        "pending" => Ok(RequestStatus::Pending),
        "approved" => Ok(RequestStatus::Approved),
        "rejected" => Ok(RequestStatus::Rejected),
        other => Err(corrupt(&format!("неизвестный статус {other}"))),
    }
}

/// Полная строка заявки (без handle) — для реконституции агрегата.
type RequestRow = (
    Uuid,
    String,
    Option<String>,
    Option<Uuid>,
    Option<String>,
    OffsetDateTime,
    Option<OffsetDateTime>,
);

fn reconstitute_request(id: Uuid, row: RequestRow) -> Result<VerificationRequest, RepositoryError> {
    let (requester, status, note, decided_by, decision_reason, created_at, decided_at) = row;
    let note = note
        .map(|n| RequestNote::parse(&n))
        .transpose()
        .map_err(|_| corrupt("записка"))?;
    let decision_reason = decision_reason
        .map(|r| DecisionReason::parse(&r))
        .transpose()
        .map_err(|_| corrupt("причина"))?;
    // Решение есть ⟺ заявка рассмотрена. Ключевой признак — `decided_at` (он не
    // обнуляется), а не `decided_by` (FK `SET NULL` мог обнулить его при удалении
    // админа — но факт решения остаётся, ADR-0016).
    let decision = decided_at.map(|at| DecisionRecord {
        by: decided_by.map(Id::from_uuid),
        reason: decision_reason,
        at: Timestamp::from_offset(at),
    });
    Ok(VerificationRequest::reconstitute(
        Id::from_uuid(id),
        Id::from_uuid(requester),
        parse_status(&status)?,
        note,
        decision,
        Timestamp::from_offset(created_at),
    ))
}

const SELECT_REQUEST: &str = "SELECT requester_id, status, note, decided_by, decision_reason, \
     created_at, decided_at FROM verification_requests WHERE id = $1";
/// Та же выборка под блокировкой строки (решающая транзакция, ADR-0016).
const SELECT_REQUEST_FOR_UPDATE: &str = "SELECT requester_id, status, note, decided_by, decision_reason, created_at, decided_at \
     FROM verification_requests WHERE id = $1 FOR UPDATE";

/// Репозиторий заявок на верификацию на Postgres.
pub struct PgVerificationRequestRepository {
    db: Db,
}

impl PgVerificationRequestRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl VerificationRequestRepository for PgVerificationRequestRepository {
    async fn find_by_id(
        &self,
        id: VerificationRequestId,
    ) -> Result<Option<VerificationRequest>, RepositoryError> {
        let row: Option<RequestRow> = sqlx::query_as(SELECT_REQUEST)
            .bind(id.as_uuid())
            .fetch_optional(&self.db)
            .await
            .map_err(map_sqlx)?;
        row.map(|r| reconstitute_request(id.as_uuid(), r))
            .transpose()
    }

    async fn find_pending_by_requester(
        &self,
        requester: UserId,
    ) -> Result<Option<VerificationRequest>, RepositoryError> {
        // Плоский кортеж: sqlx декодирует строку поколоночно, без вложенности.
        let row: Option<(
            Uuid,
            Uuid,
            String,
            Option<String>,
            Option<Uuid>,
            Option<String>,
            OffsetDateTime,
            Option<OffsetDateTime>,
        )> = sqlx::query_as(
            "SELECT id, requester_id, status, note, decided_by, decision_reason, created_at, \
             decided_at FROM verification_requests WHERE requester_id = $1 AND status = 'pending'",
        )
        .bind(requester.as_uuid())
        .fetch_optional(&self.db)
        .await
        .map_err(map_sqlx)?;
        row.map(
            |(id, requester_id, status, note, decided_by, reason, created_at, decided_at)| {
                reconstitute_request(
                    id,
                    (
                        requester_id,
                        status,
                        note,
                        decided_by,
                        reason,
                        created_at,
                        decided_at,
                    ),
                )
            },
        )
        .transpose()
    }

    async fn save(&self, request: &VerificationRequest) -> Result<(), RepositoryError> {
        // Запиской заявка не меняется после открытия; решение меняет статус/решателя.
        sqlx::query(
            "INSERT INTO verification_requests \
             (id, requester_id, status, note, decided_by, decision_reason, created_at, decided_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             ON CONFLICT (id) DO UPDATE SET status = EXCLUDED.status, \
             decided_by = EXCLUDED.decided_by, decision_reason = EXCLUDED.decision_reason, \
             decided_at = EXCLUDED.decided_at",
        )
        .bind(request.id().as_uuid())
        .bind(request.requester().as_uuid())
        .bind(request.status().as_str())
        .bind(request.note().map(RequestNote::as_str))
        .bind(request.decided_by().map(|u| u.as_uuid()))
        .bind(request.decision_reason().map(DecisionReason::as_str))
        .bind(request.created_at().into_offset())
        .bind(request.decided_at().map(Timestamp::into_offset))
        .execute(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }
}

/// Read-модель верификации: очередь админа и статус заявки юзера (ADR-0004/0016).
pub struct PgVerificationReadModel {
    db: Db,
}

impl PgVerificationReadModel {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl VerificationReadModel for PgVerificationReadModel {
    async fn pending(&self, limit: u32) -> Result<Vec<VerificationRequestView>, RepositoryError> {
        let rows: Vec<(Uuid, String, Option<String>, OffsetDateTime)> = sqlx::query_as(
            "SELECT v.id, u.handle, v.note, v.created_at \
             FROM verification_requests v JOIN users u ON u.id = v.requester_id \
             WHERE v.status = 'pending' \
             ORDER BY v.created_at ASC, v.id ASC LIMIT $1",
        )
        .bind(i64::from(limit))
        .fetch_all(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(rows
            .into_iter()
            .map(
                |(id, requester_handle, note, created_at)| VerificationRequestView {
                    request_id: Id::from_uuid(id),
                    requester_handle,
                    note,
                    created_at: Timestamp::from_offset(created_at),
                },
            )
            .collect())
    }

    async fn latest_for(
        &self,
        requester: UserId,
    ) -> Result<Option<MyVerificationView>, RepositoryError> {
        let row: Option<(
            Uuid,
            String,
            Option<String>,
            OffsetDateTime,
            Option<OffsetDateTime>,
        )> = sqlx::query_as(
            "SELECT id, status, decision_reason, created_at, decided_at \
                 FROM verification_requests WHERE requester_id = $1 \
                 ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(requester.as_uuid())
        .fetch_optional(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(row.map(
            |(id, status, decision_reason, created_at, decided_at)| MyVerificationView {
                request_id: Id::from_uuid(id),
                status,
                decision_reason,
                created_at: Timestamp::from_offset(created_at),
                decided_at: decided_at.map(Timestamp::from_offset),
            },
        ))
    }
}

/// Фабрика атомарных транзакций решения по заявке (ADR-0016).
pub struct PgVerificationDecisionTxFactory {
    db: Db,
}

impl PgVerificationDecisionTxFactory {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl VerificationDecisionTxFactory for PgVerificationDecisionTxFactory {
    async fn begin(&self) -> Result<Box<dyn VerificationDecisionTx>, RepositoryError> {
        let tx = self.db.begin().await.map_err(map_sqlx)?;
        Ok(Box::new(PgVerificationDecisionTx { tx: Some(tx) }))
    }
}

struct PgVerificationDecisionTx {
    tx: Option<Transaction<'static, Postgres>>,
}

impl PgVerificationDecisionTx {
    fn tx(&mut self) -> Result<&mut Transaction<'static, Postgres>, RepositoryError> {
        self.tx
            .as_mut()
            .ok_or_else(|| RepositoryError::Unavailable("транзакция уже завершена".to_owned()))
    }
}

#[async_trait]
impl VerificationDecisionTx for PgVerificationDecisionTx {
    async fn find_user(&mut self, id: UserId) -> Result<Option<User>, RepositoryError> {
        let tx = self.tx()?;
        let row: Option<(String, String, String, OffsetDateTime)> =
            sqlx::query_as("SELECT handle, role, verified, created_at FROM users WHERE id = $1")
                .bind(id.as_uuid())
                .fetch_optional(&mut **tx)
                .await
                .map_err(map_sqlx)?;
        row.map(|(h, r, v, c)| row_to_user(id.as_uuid(), h, r, v, c))
            .transpose()
    }

    async fn lock_request(
        &mut self,
        id: VerificationRequestId,
    ) -> Result<Option<VerificationRequest>, RepositoryError> {
        let tx = self.tx()?;
        // Блокируем строку заявки: сериализует параллельные решения (ADR-0016).
        let row: Option<RequestRow> = sqlx::query_as(SELECT_REQUEST_FOR_UPDATE)
            .bind(id.as_uuid())
            .fetch_optional(&mut **tx)
            .await
            .map_err(map_sqlx)?;
        row.map(|r| reconstitute_request(id.as_uuid(), r))
            .transpose()
    }

    async fn save_request(&mut self, request: &VerificationRequest) -> Result<(), RepositoryError> {
        let tx = self.tx()?;
        sqlx::query(
            "UPDATE verification_requests SET status = $2, decided_by = $3, \
             decision_reason = $4, decided_at = $5 WHERE id = $1",
        )
        .bind(request.id().as_uuid())
        .bind(request.status().as_str())
        .bind(request.decided_by().map(|u| u.as_uuid()))
        .bind(request.decision_reason().map(DecisionReason::as_str))
        .bind(request.decided_at().map(Timestamp::into_offset))
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    async fn save_user(&mut self, user: &User) -> Result<(), RepositoryError> {
        let tx = self.tx()?;
        // При одобрении меняется только статус верификации.
        sqlx::query("UPDATE users SET verified = $2 WHERE id = $1")
            .bind(user.id().as_uuid())
            .bind(if user.verified().is_verified() {
                "verified"
            } else {
                "casual"
            })
            .execute(&mut **tx)
            .await
            .map_err(map_sqlx)?;
        Ok(())
    }

    async fn commit(&mut self) -> Result<(), RepositoryError> {
        if let Some(tx) = self.tx.take() {
            tx.commit().await.map_err(map_sqlx)?;
        }
        Ok(())
    }
}
