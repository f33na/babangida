//! Прикладные порты для недетерминированных операций. Держим их вне домена,
//! чтобы доменные правила оставались чистыми; реализации — в `infrastructure`.

use babangida_domain::identity::InviteCode;
use babangida_shared::Timestamp;

/// Источник текущего времени (адаптер часов).
pub trait Clock: Send + Sync {
    /// Текущий момент.
    fn now(&self) -> Timestamp;
}

/// Генератор кодов приглашений. Случайность — здесь, на границе, не в домене:
/// домен код только валидирует.
pub trait InviteCodeFactory: Send + Sync {
    /// Сгенерировать новый валидный код.
    fn generate(&self) -> InviteCode;
}
