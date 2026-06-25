use core::marker::PhantomData;
use core::{fmt, hash};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Типизированный идентификатор поверх UUID. Параметр `T` — фантомный маркер
/// сущности: компилятор не даёт перепутать `Id<UserMarker>` и `Id<PostMarker>`.
///
/// Домен идентификаторы не генерирует (это не его забота и не чистая операция) —
/// он принимает готовый `Id`. Генерация ([`Id::generate`]) живёт на границе:
/// в `application`/`infrastructure`.
pub struct Id<T> {
    value: Uuid,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    /// Обернуть готовый UUID.
    #[must_use]
    pub const fn from_uuid(value: Uuid) -> Self {
        Self {
            value,
            _marker: PhantomData,
        }
    }

    /// Новый сортируемый идентификатор (UUID v7). Вызывается на границе, не в домене.
    #[must_use]
    pub fn generate() -> Self {
        Self::from_uuid(Uuid::now_v7())
    }

    /// Распарсить из строки. Ошибка, если это не валидный UUID.
    ///
    /// # Errors
    /// [`IdParseError`], если `input` не является UUID.
    pub fn parse(input: &str) -> Result<Self, IdParseError> {
        Uuid::parse_str(input)
            .map(Self::from_uuid)
            .map_err(|_| IdParseError)
    }

    /// Внутренний UUID.
    #[must_use]
    pub const fn as_uuid(&self) -> Uuid {
        self.value
    }
}

// Ручные impl'ы: derive навесил бы лишние границы `T: Clone`/`T: Eq` и т.п.,
// хотя `T` — лишь фантомный маркер и в значении не присутствует.
impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Id<T> {}

impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T> Eq for Id<T> {}

impl<T> hash::Hash for Id<T> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

impl<T> fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({})", self.value)
    }
}

impl<T> fmt::Display for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.value, f)
    }
}

impl<T> Serialize for Id<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.value.serialize(serializer)
    }
}

impl<'de, T> Deserialize<'de> for Id<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Uuid::deserialize(deserializer).map(Self::from_uuid)
    }
}

/// Строка не является валидным UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("невалидный идентификатор: ожидался UUID")]
pub struct IdParseError;

#[cfg(test)]
mod tests {
    use super::*;

    struct Marker;
    type TestId = Id<Marker>;

    #[test]
    fn generate_is_unique() {
        assert_ne!(TestId::generate(), TestId::generate());
    }

    #[test]
    fn roundtrip_through_string() {
        let id = TestId::generate();
        let parsed = TestId::parse(&id.to_string()).expect("должен распарситься");
        assert_eq!(id, parsed);
    }

    #[test]
    fn parse_rejects_garbage() {
        assert_eq!(TestId::parse("не-uuid"), Err(IdParseError));
    }
}
