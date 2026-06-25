//! Postgres-адаптеры контекста community: репозиторий сообществ, атомарная
//! транзакция членства (ADR-0012, блокировка строки группы) и read-модель карточки.

use async_trait::async_trait;
use babangida_application::query::{GroupReadModel, GroupView};
use babangida_application::{GroupMembershipTx, GroupMembershipTxFactory};
use babangida_domain::RepositoryError;
use babangida_domain::community::{
    Group, GroupId, GroupKind, GroupName, GroupPostRepository, GroupRepository, GroupSlug,
    MembershipRole,
};
use babangida_domain::content::Post;
use babangida_domain::identity::UserId;
use babangida_shared::{Id, Timestamp};
use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::map_sqlx;
use crate::pool::Db;

fn corrupt(what: &str) -> RepositoryError {
    RepositoryError::Unavailable(format!("повреждённые данные сообщества: {what}"))
}

fn parse_role(raw: &str) -> Result<MembershipRole, RepositoryError> {
    MembershipRole::parse(raw).map_err(|_| corrupt("роль участника"))
}

/// Реконституция агрегата из строк БД через доменный API (patterns/repository):
/// берём любого владельца «основателем», остальных добавляем; дополнительных
/// владельцев сначала вводим модераторами, затем повышаем (`add_member` не
/// назначает Owner). Домен не трогаем — он source of truth (ADR-0003).
fn reconstitute_group(
    id: Uuid,
    slug: String,
    name: String,
    kind: String,
    created_at: OffsetDateTime,
    members: Vec<(Uuid, String)>,
) -> Result<Group, RepositoryError> {
    let slug = GroupSlug::parse(&slug).map_err(|_| corrupt("слаг"))?;
    let name = GroupName::parse(&name).map_err(|_| corrupt("название"))?;
    let kind = GroupKind::parse(&kind).map_err(|_| corrupt("тип"))?;
    let created = Timestamp::from_offset(created_at);

    let parsed: Vec<(UserId, MembershipRole)> = members
        .into_iter()
        .map(|(u, r)| Ok((Id::from_uuid(u), parse_role(&r)?)))
        .collect::<Result<_, RepositoryError>>()?;

    let seed = parsed
        .iter()
        .find(|(_, r)| *r == MembershipRole::Owner)
        .map(|(u, _)| *u)
        .ok_or_else(|| corrupt("нет владельца"))?;

    let (mut group, _) = Group::found(Id::from_uuid(id), slug, name, kind, seed, created);
    for (user, role) in parsed.into_iter().filter(|(u, _)| *u != seed) {
        let initial = if role == MembershipRole::Owner {
            MembershipRole::Moderator
        } else {
            role
        };
        group
            .add_member(seed, user, initial, created)
            .map_err(|_| corrupt("членство"))?;
        if role == MembershipRole::Owner {
            group
                .set_role(seed, user, MembershipRole::Owner, created)
                .map_err(|_| corrupt("владелец"))?;
        }
    }
    Ok(group)
}

/// Заменить состав группы целиком (под блокировкой строки группы это безопасно).
async fn sync_members(
    tx: &mut Transaction<'static, Postgres>,
    group: &Group,
) -> Result<(), RepositoryError> {
    sqlx::query("DELETE FROM group_members WHERE group_id = $1")
        .bind(group.id().as_uuid())
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx)?;
    for m in group.members() {
        sqlx::query("INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, $3)")
            .bind(group.id().as_uuid())
            .bind(m.user.as_uuid())
            .bind(m.role.as_str())
            .execute(&mut **tx)
            .await
            .map_err(map_sqlx)?;
    }
    Ok(())
}

/// Репозиторий сообществ на Postgres. `save` используется для основания (вставка
/// нового сообщества с владельцем); изменения состава идут через
/// [`PgGroupMembershipTxFactory`] под блокировкой (ADR-0012).
pub struct PgGroupRepository {
    db: Db,
}

impl PgGroupRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl GroupRepository for PgGroupRepository {
    async fn find_by_id(&self, id: GroupId) -> Result<Option<Group>, RepositoryError> {
        let meta: Option<(String, String, String, OffsetDateTime)> =
            sqlx::query_as("SELECT slug, name, kind, created_at FROM groups WHERE id = $1")
                .bind(id.as_uuid())
                .fetch_optional(&self.db)
                .await
                .map_err(map_sqlx)?;
        let Some((slug, name, kind, created)) = meta else {
            return Ok(None);
        };
        let members = load_members(&self.db, id.as_uuid()).await?;
        Ok(Some(reconstitute_group(
            id.as_uuid(),
            slug,
            name,
            kind,
            created,
            members,
        )?))
    }

    async fn find_by_slug(&self, slug: &GroupSlug) -> Result<Option<Group>, RepositoryError> {
        let meta: Option<(Uuid, String, String, OffsetDateTime)> =
            sqlx::query_as("SELECT id, name, kind, created_at FROM groups WHERE slug = $1")
                .bind(slug.as_str())
                .fetch_optional(&self.db)
                .await
                .map_err(map_sqlx)?;
        let Some((id, name, kind, created)) = meta else {
            return Ok(None);
        };
        let members = load_members(&self.db, id).await?;
        Ok(Some(reconstitute_group(
            id,
            slug.as_str().to_owned(),
            name,
            kind,
            created,
            members,
        )?))
    }

    async fn save(&self, group: &Group) -> Result<(), RepositoryError> {
        let mut tx = self.db.begin().await.map_err(map_sqlx)?;
        sqlx::query(
            "INSERT INTO groups (id, slug, name, kind, created_at) VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (id) DO UPDATE SET slug = EXCLUDED.slug, name = EXCLUDED.name, \
             kind = EXCLUDED.kind",
        )
        .bind(group.id().as_uuid())
        .bind(group.slug().as_str())
        .bind(group.name().as_str())
        .bind(group.kind().as_str())
        .bind(group.created_at().into_offset())
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        sync_members(&mut tx, group).await?;
        tx.commit().await.map_err(map_sqlx)?;
        Ok(())
    }
}

async fn load_members(db: &Db, group_id: Uuid) -> Result<Vec<(Uuid, String)>, RepositoryError> {
    sqlx::query_as("SELECT user_id, role FROM group_members WHERE group_id = $1")
        .bind(group_id)
        .fetch_all(db)
        .await
        .map_err(map_sqlx)
}

/// Фабрика атомарных транзакций членства (ADR-0012): блокировка строки группы
/// сериализует изменения состава/ролей, держа инвариант «всегда есть владелец».
pub struct PgGroupMembershipTxFactory {
    db: Db,
}

impl PgGroupMembershipTxFactory {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl GroupMembershipTxFactory for PgGroupMembershipTxFactory {
    async fn begin(&self) -> Result<Box<dyn GroupMembershipTx>, RepositoryError> {
        let tx = self.db.begin().await.map_err(map_sqlx)?;
        Ok(Box::new(PgGroupMembershipTx { tx: Some(tx) }))
    }
}

struct PgGroupMembershipTx {
    tx: Option<Transaction<'static, Postgres>>,
}

impl PgGroupMembershipTx {
    fn tx(&mut self) -> Result<&mut Transaction<'static, Postgres>, RepositoryError> {
        self.tx
            .as_mut()
            .ok_or_else(|| RepositoryError::Unavailable("транзакция уже завершена".to_owned()))
    }
}

#[async_trait]
impl GroupMembershipTx for PgGroupMembershipTx {
    async fn lock_group(&mut self, id: GroupId) -> Result<Option<Group>, RepositoryError> {
        let tx = self.tx()?;
        // Блокируем строку группы: сериализует параллельные изменения состава (ADR-0012).
        let meta: Option<(String, String, String, OffsetDateTime)> = sqlx::query_as(
            "SELECT slug, name, kind, created_at FROM groups WHERE id = $1 FOR UPDATE",
        )
        .bind(id.as_uuid())
        .fetch_optional(&mut **tx)
        .await
        .map_err(map_sqlx)?;
        let Some((slug, name, kind, created)) = meta else {
            return Ok(None);
        };
        let members: Vec<(Uuid, String)> =
            sqlx::query_as("SELECT user_id, role FROM group_members WHERE group_id = $1")
                .bind(id.as_uuid())
                .fetch_all(&mut **tx)
                .await
                .map_err(map_sqlx)?;
        Ok(Some(reconstitute_group(
            id.as_uuid(),
            slug,
            name,
            kind,
            created,
            members,
        )?))
    }

    async fn save(&mut self, group: &Group) -> Result<(), RepositoryError> {
        let tx = self.tx()?;
        sync_members(tx, group).await
    }

    async fn commit(&mut self) -> Result<(), RepositoryError> {
        if let Some(tx) = self.tx.take() {
            tx.commit().await.map_err(map_sqlx)?;
        }
        Ok(())
    }
}

/// Публикация поста в сообщество: пост (`content.Post`) и связь пост↔сообщество
/// пишутся атомарно в одной транзакции. Агрегат `Post` не меняется (ADR-0003).
pub struct PgGroupPostRepository {
    db: Db,
}

impl PgGroupPostRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl GroupPostRepository for PgGroupPostRepository {
    async fn publish(&self, post: &Post, group: GroupId) -> Result<(), RepositoryError> {
        let mut tx = self.db.begin().await.map_err(map_sqlx)?;
        sqlx::query("INSERT INTO posts (id, author_id, body, created_at) VALUES ($1, $2, $3, $4)")
            .bind(post.id().as_uuid())
            .bind(post.author().as_uuid())
            .bind(post.body().as_str())
            .bind(post.created_at().into_offset())
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx)?;
        sqlx::query("INSERT INTO group_posts (post_id, group_id) VALUES ($1, $2)")
            .bind(post.id().as_uuid())
            .bind(group.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx)?;
        tx.commit().await.map_err(map_sqlx)?;
        Ok(())
    }
}

/// Read-модель карточки сообщества по слагу: метаданные + число участников.
pub struct PgGroupReadModel {
    db: Db,
}

impl PgGroupReadModel {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl GroupReadModel for PgGroupReadModel {
    async fn by_slug(&self, slug: &str) -> Result<Option<GroupView>, RepositoryError> {
        let row: Option<(Uuid, String, String, String, i64)> = sqlx::query_as(
            "SELECT g.id, g.slug, g.name, g.kind, count(gm.user_id) \
             FROM groups g LEFT JOIN group_members gm ON gm.group_id = g.id \
             WHERE g.slug = $1 GROUP BY g.id, g.slug, g.name, g.kind",
        )
        .bind(slug)
        .fetch_optional(&self.db)
        .await
        .map_err(map_sqlx)?;
        Ok(row.map(|(id, slug, name, kind, count)| GroupView {
            group_id: Id::from_uuid(id),
            slug,
            name,
            kind,
            member_count: u32::try_from(count).unwrap_or(0),
        }))
    }
}
