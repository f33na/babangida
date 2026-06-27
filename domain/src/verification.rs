//! Контекст verification: процесс получения статуса верификации (ADR-0016).
//! Сам статус живёт на [`crate::identity::User`] (замороженный агрегат identity —
//! source of truth `casual`/`verified`, ADR-0010); этот контекст владеет лишь
//! *процессом*: заявка → рассмотрение админом → одобрение/отказ.
//!
//! Машина состояний заявки — инвариант домена ([`VerificationRequest::approve`]/
//! [`reject`](VerificationRequest::reject) идут только из `Pending`). Кто имеет право
//! рассматривать (админ) и можно ли подавать (ещё не верифицирован, нет открытой
//! заявки) — решает `application`: это кросс-агрегатные правила, домен их не видит.
//!
//! Анти-ВК: подача заявки — действие в профиле/настройках, очередь — экран админа
//! внутри той же сети, не отдельный инструмент.

use babangida_shared::{Id, Timestamp};

use crate::identity::UserId;

/// Сопроводительная записка к заявке (зачем верифицировать — ссылки на треки и т.п.).
/// Опциональна на уровне агрегата; если задана — непустая, не длиннее [`MAX_LEN`](Self::MAX_LEN).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestNote(String);

impl RequestNote {
    /// Максимальная длина записки.
    pub const MAX_LEN: usize = 500;

    /// Распарсить записку.
    ///
    /// # Errors
    /// [`RequestNoteError`], если пусто или длиннее [`MAX_LEN`](Self::MAX_LEN).
    pub fn parse(input: &str) -> Result<Self, RequestNoteError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(RequestNoteError::Empty);
        }
        let len = trimmed.chars().count();
        if len > Self::MAX_LEN {
            return Err(RequestNoteError::TooLong { len });
        }
        Ok(Self(trimmed.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Нарушение правил [`RequestNote`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RequestNoteError {
    #[error("записка заявки пустая")]
    Empty,
    #[error("записка заявки слишком длинная: {len} символов (максимум 500)")]
    TooLong { len: usize },
}

/// Причина решения админа (особенно для отказа). Те же правила, что у [`RequestNote`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionReason(String);

impl DecisionReason {
    /// Максимальная длина причины.
    pub const MAX_LEN: usize = 500;

    /// Распарсить причину.
    ///
    /// # Errors
    /// [`DecisionReasonError`], если пусто или длиннее [`MAX_LEN`](Self::MAX_LEN).
    pub fn parse(input: &str) -> Result<Self, DecisionReasonError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(DecisionReasonError::Empty);
        }
        let len = trimmed.chars().count();
        if len > Self::MAX_LEN {
            return Err(DecisionReasonError::TooLong { len });
        }
        Ok(Self(trimmed.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Нарушение правил [`DecisionReason`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecisionReasonError {
    #[error("причина решения пустая")]
    Empty,
    #[error("причина решения слишком длинная: {len} символов (максимум 500)")]
    TooLong { len: usize },
}

/// Статус заявки на верификацию.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestStatus {
    /// Ждёт рассмотрения админом.
    Pending,
    /// Одобрена — юзер верифицирован.
    Approved,
    /// Отклонена — можно подать новую.
    Rejected,
}

impl RequestStatus {
    /// Ждёт ли рассмотрения (можно одобрить/отклонить).
    #[must_use]
    pub const fn is_pending(self) -> bool {
        matches!(self, Self::Pending)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }
}

/// Фантомный маркер для [`VerificationRequestId`].
pub enum VerificationRequestMarker {}
/// Идентификатор заявки на верификацию.
pub type VerificationRequestId = Id<VerificationRequestMarker>;

/// Заявка на верификацию — корень агрегата verification. Жизненный цикл:
/// `Pending` → `Approved` | `Rejected`. Терминальные состояния неизменны
/// (повторное решение → [`VerificationError::AlreadyDecided`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationRequest {
    id: VerificationRequestId,
    requester: UserId,
    status: RequestStatus,
    note: Option<RequestNote>,
    decided_by: Option<UserId>,
    decision_reason: Option<DecisionReason>,
    created_at: Timestamp,
    decided_at: Option<Timestamp>,
}

impl VerificationRequest {
    /// Открыть заявку (всегда `Pending`). Можно ли подавать (не верифицирован, нет
    /// другой открытой) — проверяет `application`; конструирование заявки само по
    /// себе валидно.
    #[must_use]
    pub fn open(
        id: VerificationRequestId,
        requester: UserId,
        note: Option<RequestNote>,
        now: Timestamp,
    ) -> (Self, VerificationRequested) {
        let request = Self {
            id,
            requester,
            status: RequestStatus::Pending,
            note,
            decided_by: None,
            decision_reason: None,
            created_at: now,
            decided_at: None,
        };
        let event = VerificationRequested {
            request: id,
            requester,
        };
        (request, event)
    }

    /// Одобрить заявку (только из `Pending`). Сам статус юзера ставит `application`
    /// через [`crate::identity::User::verify`] в той же транзакции (ADR-0016).
    ///
    /// # Errors
    /// [`VerificationError::AlreadyDecided`], если заявка уже рассмотрена.
    pub fn approve(
        &mut self,
        admin: UserId,
        reason: Option<DecisionReason>,
        now: Timestamp,
    ) -> Result<VerificationApproved, VerificationError> {
        self.require_pending()?;
        self.status = RequestStatus::Approved;
        self.decided_by = Some(admin);
        self.decision_reason = reason;
        self.decided_at = Some(now);
        Ok(VerificationApproved {
            request: self.id,
            requester: self.requester,
            by: admin,
        })
    }

    /// Отклонить заявку (только из `Pending`). После отказа юзер может подать новую.
    ///
    /// # Errors
    /// [`VerificationError::AlreadyDecided`], если заявка уже рассмотрена.
    pub fn reject(
        &mut self,
        admin: UserId,
        reason: Option<DecisionReason>,
        now: Timestamp,
    ) -> Result<VerificationRejected, VerificationError> {
        self.require_pending()?;
        self.status = RequestStatus::Rejected;
        self.decided_by = Some(admin);
        self.decision_reason = reason;
        self.decided_at = Some(now);
        Ok(VerificationRejected {
            request: self.id,
            requester: self.requester,
            by: admin,
        })
    }

    /// Восстановить агрегат из хранилища (`infrastructure`). В отличие от
    /// замороженного identity, у нового контекста есть честный reconstitute —
    /// доменного хака не требуется. Решение группируется в [`DecisionRecord`]
    /// (`Some` ⟺ заявка рассмотрена, `None` ⟺ `pending`).
    #[must_use]
    pub fn reconstitute(
        id: VerificationRequestId,
        requester: UserId,
        status: RequestStatus,
        note: Option<RequestNote>,
        decision: Option<DecisionRecord>,
        created_at: Timestamp,
    ) -> Self {
        let (decided_by, decision_reason, decided_at) = match decision {
            Some(d) => (d.by, d.reason, Some(d.at)),
            None => (None, None, None),
        };
        Self {
            id,
            requester,
            status,
            note,
            decided_by,
            decision_reason,
            created_at,
            decided_at,
        }
    }

    fn require_pending(&self) -> Result<(), VerificationError> {
        if self.status.is_pending() {
            Ok(())
        } else {
            Err(VerificationError::AlreadyDecided)
        }
    }

    #[must_use]
    pub const fn id(&self) -> VerificationRequestId {
        self.id
    }
    #[must_use]
    pub const fn requester(&self) -> UserId {
        self.requester
    }
    #[must_use]
    pub const fn status(&self) -> RequestStatus {
        self.status
    }
    #[must_use]
    pub fn note(&self) -> Option<&RequestNote> {
        self.note.as_ref()
    }
    #[must_use]
    pub const fn decided_by(&self) -> Option<UserId> {
        self.decided_by
    }
    #[must_use]
    pub fn decision_reason(&self) -> Option<&DecisionReason> {
        self.decision_reason.as_ref()
    }
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }
    #[must_use]
    pub const fn decided_at(&self) -> Option<Timestamp> {
        self.decided_at
    }
}

/// Запись о решении по заявке (кто, причина, когда) — для реконституции из БД.
/// `Some` ⟺ заявка рассмотрена (`approved`/`rejected`); `None` ⟺ `pending`. `by`
/// опционален: если админ-решатель позже удалён (FK `SET NULL`), факт решения
/// сохраняется, теряется только ссылка на автора.
#[derive(Debug, Clone)]
pub struct DecisionRecord {
    pub by: Option<UserId>,
    pub reason: Option<DecisionReason>,
    pub at: Timestamp,
}

/// Событие: подана заявка на верификацию.
#[derive(Debug, Clone)]
pub struct VerificationRequested {
    pub request: VerificationRequestId,
    pub requester: UserId,
}

/// Событие: заявка одобрена (юзер становится верифицированным).
#[derive(Debug, Clone)]
pub struct VerificationApproved {
    pub request: VerificationRequestId,
    pub requester: UserId,
    pub by: UserId,
}

/// Событие: заявка отклонена.
#[derive(Debug, Clone)]
pub struct VerificationRejected {
    pub request: VerificationRequestId,
    pub requester: UserId,
    pub by: UserId,
}

/// Нарушение правил контекста verification.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VerificationError {
    /// Заявка уже рассмотрена (одобрена или отклонена) — повторное решение запрещено.
    #[error("заявка на верификацию уже рассмотрена")]
    AlreadyDecided,
}

/// Хранилище заявок на верификацию.
#[async_trait::async_trait]
pub trait VerificationRequestRepository: Send + Sync {
    async fn find_by_id(
        &self,
        id: VerificationRequestId,
    ) -> Result<Option<VerificationRequest>, crate::RepositoryError>;
    /// Открытая (`Pending`) заявка юзера, если есть — гейт «одна заявка за раз».
    async fn find_pending_by_requester(
        &self,
        requester: UserId,
    ) -> Result<Option<VerificationRequest>, crate::RepositoryError>;
    async fn save(&self, request: &VerificationRequest) -> Result<(), crate::RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> VerificationRequest {
        VerificationRequest::open(
            Id::generate(),
            Id::generate(),
            Some(RequestNote::parse("залил три релиза, торгую битами").unwrap()),
            Timestamp::now(),
        )
        .0
    }

    #[test]
    fn note_and_reason_validate() {
        assert_eq!(RequestNote::parse("   "), Err(RequestNoteError::Empty));
        assert!(matches!(
            RequestNote::parse(&"x".repeat(501)),
            Err(RequestNoteError::TooLong { .. })
        ));
        assert_eq!(DecisionReason::parse(""), Err(DecisionReasonError::Empty));
        assert_eq!(RequestNote::parse(" ok ").unwrap().as_str(), "ok");
    }

    #[test]
    fn opened_request_is_pending() {
        let req = open();
        assert_eq!(req.status(), RequestStatus::Pending);
        assert!(req.decided_by().is_none());
        assert!(req.decided_at().is_none());
    }

    #[test]
    fn approve_transitions_and_records_admin() {
        let mut req = open();
        let admin = Id::generate();
        let event = req
            .approve(admin, None, Timestamp::now())
            .expect("из Pending можно одобрить");
        assert_eq!(req.status(), RequestStatus::Approved);
        assert_eq!(req.decided_by(), Some(admin));
        assert!(req.decided_at().is_some());
        assert_eq!(event.requester, req.requester());
        assert_eq!(event.by, admin);
    }

    #[test]
    fn reject_records_reason() {
        let mut req = open();
        let admin = Id::generate();
        let reason = DecisionReason::parse("аккаунт слишком новый").unwrap();
        req.reject(admin, Some(reason.clone()), Timestamp::now())
            .expect("из Pending можно отклонить");
        assert_eq!(req.status(), RequestStatus::Rejected);
        assert_eq!(req.decision_reason(), Some(&reason));
    }

    #[test]
    fn cannot_decide_twice() {
        let mut req = open();
        req.approve(Id::generate(), None, Timestamp::now()).unwrap();
        assert_eq!(
            req.approve(Id::generate(), None, Timestamp::now())
                .unwrap_err(),
            VerificationError::AlreadyDecided
        );
        assert_eq!(
            req.reject(Id::generate(), None, Timestamp::now())
                .unwrap_err(),
            VerificationError::AlreadyDecided
        );
    }

    #[test]
    fn rejected_cannot_be_approved() {
        let mut req = open();
        req.reject(Id::generate(), None, Timestamp::now()).unwrap();
        assert_eq!(
            req.approve(Id::generate(), None, Timestamp::now())
                .unwrap_err(),
            VerificationError::AlreadyDecided
        );
    }
}
