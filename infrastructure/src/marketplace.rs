//! Postgres-адаптеры контекста marketplace (ADR-0014): репозиторий товаров и
//! read-модели маркета/профиля. Реконституция агрегата — через доменный API
//! (patterns/repository), т.к. домен не трогаем.

use async_trait::async_trait;
use babangida_application::query::{ListingReadModel, ListingView};
use babangida_domain::RepositoryError;
use babangida_domain::identity::VerifiedStatus;
use babangida_domain::marketplace::{
    Listing, ListingDescription, ListingDraft, ListingId, ListingRepository, ListingTitle, Price,
};
use babangida_shared::{Id, Timestamp};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::map_sqlx;
use crate::pool::Db;

/// Строка товара из БД (без handle продавца).
type ListingRow = (Uuid, String, i64, Option<String>, String, OffsetDateTime);

fn corrupt(what: &str) -> RepositoryError {
    RepositoryError::Unavailable(format!("повреждённый товар в БД: {what}"))
}

fn build_draft(
    title: String,
    price: i64,
    description: Option<String>,
) -> Result<ListingDraft, RepositoryError> {
    Ok(ListingDraft {
        title: ListingTitle::parse(&title).map_err(|_| corrupt("заголовок"))?,
        price: Price::parse(u64::try_from(price).map_err(|_| corrupt("цена"))?)
            .map_err(|_| corrupt("цена"))?,
        description: description
            .map(|d| ListingDescription::parse(&d))
            .transpose()
            .map_err(|_| corrupt("описание"))?,
    })
}

/// Реконституция товара через доменный API: создаём активный (гейт обходим
/// `Verified` — он уже был пройден при выставлении), затем доводим до статуса из БД.
fn reconstitute_listing(
    id: Uuid,
    seller: Uuid,
    row: ListingRow,
) -> Result<Listing, RepositoryError> {
    let (db_seller, title, price, description, status, created_at) = row;
    debug_assert_eq!(db_seller, seller);
    let seller = Id::from_uuid(seller);
    let draft = build_draft(title, price, description)?;
    let (mut listing, _event) = Listing::list(
        Id::from_uuid(id),
        seller,
        VerifiedStatus::Verified,
        draft,
        Timestamp::from_offset(created_at),
    )
    .map_err(|_| corrupt("реконституция"))?;
    match status.as_str() {
        "active" => {}
        "sold" => {
            listing.mark_sold(seller).map_err(|_| corrupt("статус"))?;
        }
        "withdrawn" => {
            listing.withdraw(seller).map_err(|_| corrupt("статус"))?;
        }
        other => return Err(corrupt(&format!("неизвестный статус {other}"))),
    }
    Ok(listing)
}

/// Репозиторий товаров на Postgres.
pub struct PgListingRepository {
    db: Db,
}

impl PgListingRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ListingRepository for PgListingRepository {
    async fn find_by_id(&self, id: ListingId) -> Result<Option<Listing>, RepositoryError> {
        let row: Option<ListingRow> = sqlx::query_as(
            "SELECT seller_id, title, price, description, status, created_at \
             FROM listings WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.db)
        .await
        .map_err(map_sqlx)?;
        row.map(|r| {
            let seller = r.0;
            reconstitute_listing(id.as_uuid(), seller, r)
        })
        .transpose()
    }

    async fn save(&self, listing: &Listing) -> Result<(), RepositoryError> {
        // Изменяемое поле после создания — только статус (продано/снято).
        sqlx::query(
            "INSERT INTO listings (id, seller_id, title, price, description, status, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT (id) DO UPDATE SET status = EXCLUDED.status",
        )
        .bind(listing.id().as_uuid())
        .bind(listing.seller().as_uuid())
        .bind(listing.title().as_str())
        .bind(
            i64::try_from(listing.price().rubles())
                .map_err(|_| RepositoryError::Unavailable("цена вне диапазона".to_owned()))?,
        )
        .bind(listing.description().map(ListingDescription::as_str))
        .bind(listing.status().as_str())
        .bind(listing.created_at().into_offset())
        .execute(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }
}

/// Строка товара с handle продавца (для read-моделей).
type ListingViewRow = (
    Uuid,
    Uuid,
    String,
    String,
    i64,
    Option<String>,
    String,
    OffsetDateTime,
);

fn row_to_view(row: ListingViewRow) -> Result<ListingView, RepositoryError> {
    let (id, seller, seller_handle, title, price, description, status, created_at) = row;
    Ok(ListingView {
        listing_id: Id::from_uuid(id),
        seller: Id::from_uuid(seller),
        seller_handle,
        title,
        price_rubles: u64::try_from(price)
            .map_err(|_| RepositoryError::Unavailable("цена вне диапазона".to_owned()))?,
        description,
        status,
        created_at: Timestamp::from_offset(created_at),
    })
}

/// Read-модель товаров: общий раздел и товары продавца (ADR-0004).
pub struct PgListingReadModel {
    db: Db,
}

impl PgListingReadModel {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ListingReadModel for PgListingReadModel {
    async fn active(&self, limit: u32) -> Result<Vec<ListingView>, RepositoryError> {
        let rows: Vec<ListingViewRow> = sqlx::query_as(
            "SELECT l.id, l.seller_id, u.handle, l.title, l.price, l.description, l.status, l.created_at \
             FROM listings l JOIN users u ON u.id = l.seller_id \
             WHERE l.status = 'active' \
             ORDER BY l.created_at DESC, l.id DESC LIMIT $1",
        )
        .bind(i64::from(limit))
        .fetch_all(&self.db)
        .await
        .map_err(map_sqlx)?;
        rows.into_iter().map(row_to_view).collect()
    }

    async fn by_seller(
        &self,
        handle: &str,
        limit: u32,
    ) -> Result<Vec<ListingView>, RepositoryError> {
        let rows: Vec<ListingViewRow> = sqlx::query_as(
            "SELECT l.id, l.seller_id, u.handle, l.title, l.price, l.description, l.status, l.created_at \
             FROM listings l JOIN users u ON u.id = l.seller_id \
             WHERE u.handle = $1 \
             ORDER BY l.created_at DESC, l.id DESC LIMIT $2",
        )
        .bind(handle)
        .bind(i64::from(limit))
        .fetch_all(&self.db)
        .await
        .map_err(map_sqlx)?;
        rows.into_iter().map(row_to_view).collect()
    }
}
