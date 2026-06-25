//! Паттерн Specification: предикат над сущностью как объект, который можно
//! комбинировать (`and`/`or`/`negate`). Полезен, когда правило именованное и
//! переиспользуется в разных местах. Конкретные спецификации — рядом со своими
//! сущностями (см. [`crate::identity`]).

/// Бизнес-предикат над `T`.
pub trait Specification<T: ?Sized> {
    /// Удовлетворяет ли кандидат спецификации.
    fn is_satisfied_by(&self, candidate: &T) -> bool;

    /// Логическое И.
    fn and<S: Specification<T>>(self, other: S) -> And<Self, S>
    where
        Self: Sized,
    {
        And(self, other)
    }

    /// Логическое ИЛИ.
    fn or<S: Specification<T>>(self, other: S) -> Or<Self, S>
    where
        Self: Sized,
    {
        Or(self, other)
    }

    /// Логическое НЕ. (Назван `negate`, а не `not`, чтобы не путать с `std::ops::Not`.)
    fn negate(self) -> Not<Self>
    where
        Self: Sized,
    {
        Not(self)
    }
}

/// Конъюнкция двух спецификаций.
pub struct And<A, B>(A, B);
/// Дизъюнкция двух спецификаций.
pub struct Or<A, B>(A, B);
/// Отрицание спецификации.
pub struct Not<A>(A);

impl<T: ?Sized, A: Specification<T>, B: Specification<T>> Specification<T> for And<A, B> {
    fn is_satisfied_by(&self, candidate: &T) -> bool {
        self.0.is_satisfied_by(candidate) && self.1.is_satisfied_by(candidate)
    }
}

impl<T: ?Sized, A: Specification<T>, B: Specification<T>> Specification<T> for Or<A, B> {
    fn is_satisfied_by(&self, candidate: &T) -> bool {
        self.0.is_satisfied_by(candidate) || self.1.is_satisfied_by(candidate)
    }
}

impl<T: ?Sized, A: Specification<T>> Specification<T> for Not<A> {
    fn is_satisfied_by(&self, candidate: &T) -> bool {
        !self.0.is_satisfied_by(candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct GreaterThan(i32);
    struct EvenNumber;

    impl Specification<i32> for GreaterThan {
        fn is_satisfied_by(&self, candidate: &i32) -> bool {
            *candidate > self.0
        }
    }
    impl Specification<i32> for EvenNumber {
        fn is_satisfied_by(&self, candidate: &i32) -> bool {
            *candidate % 2 == 0
        }
    }

    #[test]
    fn combinators_compose() {
        let spec = GreaterThan(10).and(EvenNumber);
        assert!(spec.is_satisfied_by(&12));
        assert!(!spec.is_satisfied_by(&11)); // нечётное
        assert!(!spec.is_satisfied_by(&8)); // не больше 10

        let spec = GreaterThan(10).or(EvenNumber);
        assert!(spec.is_satisfied_by(&8));

        let spec = EvenNumber.negate();
        assert!(spec.is_satisfied_by(&7));
    }
}
