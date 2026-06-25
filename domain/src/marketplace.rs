//! Контекст marketplace: барахолка. Продажа — привилегия верифицированных (ADR-0010):
//! гейт держим инвариантом домена ([`Listing::list`] требует [`VerifiedStatus`]),
//! `application` лишь читает статус и передаёт сюда — как с квотой инвайта
//! ([`crate::identity::Invite::issue`], ADR-0011/0003).
//!
//! Анти-ВК: товары живут в профиле и общем разделе маркета внутри той же сети, не
//! отдельным приложением. Модель — ADR-0014.

use babangida_shared::{Id, Timestamp};

use crate::identity::{UserId, VerifiedStatus};

/// Заголовок товара. 1..=120 символов.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListingTitle(String);

impl ListingTitle {
    /// Максимальная длина заголовка.
    pub const MAX_LEN: usize = 120;

    /// Распарсить заголовок.
    ///
    /// # Errors
    /// [`ListingTitleError`], если пусто или длиннее [`MAX_LEN`](Self::MAX_LEN).
    pub fn parse(input: &str) -> Result<Self, ListingTitleError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(ListingTitleError::Empty);
        }
        let len = trimmed.chars().count();
        if len > Self::MAX_LEN {
            return Err(ListingTitleError::TooLong { len });
        }
        Ok(Self(trimmed.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Нарушение правил [`ListingTitle`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ListingTitleError {
    #[error("заголовок товара пустой")]
    Empty,
    #[error("заголовок товара слишком длинный: {len} символов (максимум 120)")]
    TooLong { len: usize },
}

/// Описание товара. 1..=4000 символов.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListingDescription(String);

impl ListingDescription {
    /// Максимальная длина описания.
    pub const MAX_LEN: usize = 4000;

    /// Распарсить описание.
    ///
    /// # Errors
    /// [`ListingDescriptionError`], если пусто или длиннее [`MAX_LEN`](Self::MAX_LEN).
    pub fn parse(input: &str) -> Result<Self, ListingDescriptionError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(ListingDescriptionError::Empty);
        }
        let len = trimmed.chars().count();
        if len > Self::MAX_LEN {
            return Err(ListingDescriptionError::TooLong { len });
        }
        Ok(Self(trimmed.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Нарушение правил [`ListingDescription`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ListingDescriptionError {
    #[error("описание товара пустое")]
    Empty,
    #[error("описание товара слишком длинное: {len} символов (максимум 4000)")]
    TooLong { len: usize },
}

/// Цена в целых рублях (MVP: без копеек и валют). Строго положительная.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Price(u64);

impl Price {
    /// Верхняя граница (защита от мусора/переполнения отображения).
    pub const MAX: u64 = 1_000_000_000;

    /// Распарсить цену из целого числа рублей.
    ///
    /// # Errors
    /// [`PriceError`], если 0 или больше [`MAX`](Self::MAX).
    pub fn parse(rubles: u64) -> Result<Self, PriceError> {
        if rubles == 0 {
            return Err(PriceError::Zero);
        }
        if rubles > Self::MAX {
            return Err(PriceError::TooLarge);
        }
        Ok(Self(rubles))
    }

    /// Сумма в рублях.
    #[must_use]
    pub const fn rubles(self) -> u64 {
        self.0
    }
}

/// Нарушение правил [`Price`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PriceError {
    #[error("цена не может быть нулевой")]
    Zero,
    #[error("цена слишком большая")]
    TooLarge,
}

/// Черновик товара — то, что задаёт продавец (без id/времени/статуса).
#[derive(Debug, Clone)]
pub struct ListingDraft {
    pub title: ListingTitle,
    pub price: Price,
    pub description: Option<ListingDescription>,
}

/// Статус товара.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListingStatus {
    /// Активен — в продаже.
    Active,
    /// Продан.
    Sold,
    /// Снят продавцом.
    Withdrawn,
}

impl ListingStatus {
    /// В продаже ли (можно отметить проданным/снять).
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Sold => "sold",
            Self::Withdrawn => "withdrawn",
        }
    }
}

/// Фантомный маркер для [`ListingId`].
pub enum ListingMarker {}
/// Идентификатор товара.
pub type ListingId = Id<ListingMarker>;

/// Товар на барахолке — корень агрегата marketplace. Создаётся только
/// верифицированным продавцом; менять статус может только он.
#[derive(Debug, Clone)]
pub struct Listing {
    id: ListingId,
    seller: UserId,
    title: ListingTitle,
    price: Price,
    description: Option<ListingDescription>,
    status: ListingStatus,
    created_at: Timestamp,
}

impl Listing {
    /// Выставить товар. Гейт верификации (ADR-0010): продавец обязан быть
    /// верифицирован — статус читает `application`, инвариант держит домен.
    ///
    /// # Errors
    /// [`MarketplaceError::NotVerified`], если продавец не верифицирован.
    pub fn list(
        id: ListingId,
        seller: UserId,
        verified: VerifiedStatus,
        draft: ListingDraft,
        now: Timestamp,
    ) -> Result<(Self, ListingPosted), MarketplaceError> {
        if !verified.is_verified() {
            return Err(MarketplaceError::NotVerified);
        }
        let listing = Self {
            id,
            seller,
            title: draft.title,
            price: draft.price,
            description: draft.description,
            status: ListingStatus::Active,
            created_at: now,
        };
        let event = ListingPosted {
            listing: id,
            seller,
        };
        Ok((listing, event))
    }

    /// Отметить проданным (только продавец, только из активного).
    ///
    /// # Errors
    /// [`MarketplaceError`]: не продавец или товар уже не активен.
    pub fn mark_sold(&mut self, actor: UserId) -> Result<ListingSold, MarketplaceError> {
        self.require_seller(actor)?;
        self.require_active()?;
        self.status = ListingStatus::Sold;
        Ok(ListingSold { listing: self.id })
    }

    /// Снять с продажи (только продавец, только из активного).
    ///
    /// # Errors
    /// [`MarketplaceError`]: не продавец или товар уже не активен.
    pub fn withdraw(&mut self, actor: UserId) -> Result<ListingWithdrawn, MarketplaceError> {
        self.require_seller(actor)?;
        self.require_active()?;
        self.status = ListingStatus::Withdrawn;
        Ok(ListingWithdrawn { listing: self.id })
    }

    fn require_seller(&self, actor: UserId) -> Result<(), MarketplaceError> {
        if actor == self.seller {
            Ok(())
        } else {
            Err(MarketplaceError::NotSeller)
        }
    }

    fn require_active(&self) -> Result<(), MarketplaceError> {
        if self.status.is_active() {
            Ok(())
        } else {
            Err(MarketplaceError::NotActive)
        }
    }

    #[must_use]
    pub const fn id(&self) -> ListingId {
        self.id
    }
    #[must_use]
    pub const fn seller(&self) -> UserId {
        self.seller
    }
    #[must_use]
    pub fn title(&self) -> &ListingTitle {
        &self.title
    }
    #[must_use]
    pub const fn price(&self) -> Price {
        self.price
    }
    #[must_use]
    pub fn description(&self) -> Option<&ListingDescription> {
        self.description.as_ref()
    }
    #[must_use]
    pub const fn status(&self) -> ListingStatus {
        self.status
    }
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }
}

/// Событие: товар выставлен.
#[derive(Debug, Clone)]
pub struct ListingPosted {
    pub listing: ListingId,
    pub seller: UserId,
}

/// Событие: товар продан.
#[derive(Debug, Clone)]
pub struct ListingSold {
    pub listing: ListingId,
}

/// Событие: товар снят с продажи.
#[derive(Debug, Clone)]
pub struct ListingWithdrawn {
    pub listing: ListingId,
}

/// Нарушение правил контекста marketplace.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MarketplaceError {
    /// Продавать может только верифицированный (ADR-0010).
    #[error("продажа доступна только верифицированным")]
    NotVerified,
    /// Менять товар может только его продавец.
    #[error("действие доступно только продавцу товара")]
    NotSeller,
    /// Товар уже не активен (продан или снят).
    #[error("товар уже не активен")]
    NotActive,
}

/// Хранилище товаров.
#[async_trait::async_trait]
pub trait ListingRepository: Send + Sync {
    async fn find_by_id(&self, id: ListingId) -> Result<Option<Listing>, crate::RepositoryError>;
    async fn save(&self, listing: &Listing) -> Result<(), crate::RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> ListingDraft {
        ListingDraft {
            title: ListingTitle::parse("MPC 2000XL").unwrap(),
            price: Price::parse(45000).unwrap(),
            description: Some(ListingDescription::parse("живой, без проблем").unwrap()),
        }
    }

    fn verified_listing(seller: UserId) -> Listing {
        Listing::list(
            Id::generate(),
            seller,
            VerifiedStatus::Verified,
            draft(),
            Timestamp::now(),
        )
        .expect("верифицированный может выставить")
        .0
    }

    #[test]
    fn title_and_price_validate() {
        assert_eq!(ListingTitle::parse("  "), Err(ListingTitleError::Empty));
        assert!(matches!(
            ListingTitle::parse(&"x".repeat(121)),
            Err(ListingTitleError::TooLong { .. })
        ));
        assert_eq!(Price::parse(0), Err(PriceError::Zero));
        assert!(matches!(
            Price::parse(2_000_000_000),
            Err(PriceError::TooLarge)
        ));
        assert_eq!(Price::parse(100).unwrap().rubles(), 100);
    }

    #[test]
    fn casual_cannot_list() {
        let err = Listing::list(
            Id::generate(),
            Id::generate(),
            VerifiedStatus::Casual,
            draft(),
            Timestamp::now(),
        )
        .unwrap_err();
        assert_eq!(err, MarketplaceError::NotVerified);
    }

    #[test]
    fn verified_lists_active() {
        let seller = Id::generate();
        let (listing, event) = Listing::list(
            Id::generate(),
            seller,
            VerifiedStatus::Verified,
            draft(),
            Timestamp::now(),
        )
        .unwrap();
        assert!(listing.status().is_active());
        assert_eq!(listing.seller(), seller);
        assert_eq!(event.seller, seller);
    }

    #[test]
    fn only_seller_changes_status() {
        let seller = Id::generate();
        let mut listing = verified_listing(seller);
        assert_eq!(
            listing.mark_sold(Id::generate()).unwrap_err(),
            MarketplaceError::NotSeller
        );
        assert!(listing.mark_sold(seller).is_ok());
        assert_eq!(listing.status(), ListingStatus::Sold);
    }

    #[test]
    fn cannot_change_inactive() {
        let seller = Id::generate();
        let mut listing = verified_listing(seller);
        listing.withdraw(seller).unwrap();
        assert_eq!(
            listing.mark_sold(seller).unwrap_err(),
            MarketplaceError::NotActive
        );
        assert_eq!(
            listing.withdraw(seller).unwrap_err(),
            MarketplaceError::NotActive
        );
    }
}
