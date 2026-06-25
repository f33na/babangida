use core::ops::Add;

use time::OffsetDateTime;

/// Длительность. Реэкспорт из `time`, чтобы домен не зависел от крейта `time` напрямую.
pub use time::Duration;

/// Момент времени в UTC. Домен получает `Timestamp` снаружи и сам системные часы
/// не читает — так доменные правила (например кулдаун инвайта) остаются чистыми и
/// детерминированно тестируемыми. Текущее время добывается на границе через
/// [`Timestamp::now`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Timestamp(OffsetDateTime);

impl Timestamp {
    /// Текущее время UTC. Вызывается на границе (адаптер часов), не в домене.
    #[must_use]
    pub fn now() -> Self {
        Self(OffsetDateTime::now_utc())
    }

    /// Обернуть конкретный момент (для адаптеров и тестов).
    #[must_use]
    pub const fn from_offset(value: OffsetDateTime) -> Self {
        Self(value)
    }

    /// Внутренний `OffsetDateTime`.
    #[must_use]
    pub const fn into_offset(self) -> OffsetDateTime {
        self.0
    }

    /// Сколько прошло от `earlier` до `self`. Отрицательно, если `earlier` позже.
    #[must_use]
    pub fn duration_since(self, earlier: Self) -> Duration {
        self.0 - earlier.0
    }
}

impl Add<Duration> for Timestamp {
    type Output = Self;

    /// # Panics
    /// Если результат выходит за поддерживаемый диапазон дат. На практике
    /// используется с малыми длительностями (кулдаун, тесты). Для произвольных
    /// длительностей с внешним вводом сначала проверяй диапазон.
    fn add(self, rhs: Duration) -> Self {
        Self(self.0 + rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_since_is_signed() {
        let t0 = Timestamp::now();
        let later = t0 + Duration::hours(13);
        assert_eq!(later.duration_since(t0), Duration::hours(13));
        assert_eq!(t0.duration_since(later), Duration::hours(-13));
    }

    #[test]
    fn ordering_follows_time() {
        let t0 = Timestamp::now();
        assert!(t0 < t0 + Duration::seconds(1));
    }
}
