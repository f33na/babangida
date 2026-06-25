//! Контекст community: сообщества — закрытые группы и открытые паблики. Анти-ВК:
//! контент сообществ течёт в ОБЩУЮ ленту (контекст [`crate::content`]), а не в
//! отдельное приложение; здесь — членство, роли и право публикации/модерации.
//! Доступно всем без верификации (верификация гейтит маркет/музыку/API, ADR-0010).

use async_trait::async_trait;
use babangida_shared::{Id, Timestamp};

use crate::RepositoryError;
use crate::identity::UserId;

/// Название сообщества. 1..=60 символов после обрезки, без управляющих символов.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupName(String);

impl GroupName {
    /// Максимальная длина.
    pub const MAX_LEN: usize = 60;

    /// Распарсить название.
    ///
    /// # Errors
    /// [`GroupNameError`], если пусто, длиннее [`GroupName::MAX_LEN`] или содержит
    /// управляющие символы.
    pub fn parse(input: &str) -> Result<Self, GroupNameError> {
        let name = input.trim();
        if name.is_empty() {
            return Err(GroupNameError::Empty);
        }
        let len = name.chars().count();
        if len > Self::MAX_LEN {
            return Err(GroupNameError::TooLong { len });
        }
        if name.chars().any(char::is_control) {
            return Err(GroupNameError::ControlChar);
        }
        Ok(Self(name.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Нарушение правил [`GroupName`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GroupNameError {
    #[error("название пустое")]
    Empty,
    #[error("название слишком длинное: {len} символов")]
    TooLong { len: usize },
    #[error("название содержит управляющие символы")]
    ControlChar,
}

/// Слаг сообщества — уникальный @-идентификатор в URL. Правила как у `Handle`:
/// нормализуется в нижний регистр, 3..=30 символов, начинается с латинской буквы,
/// дальше `[a-z0-9_]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GroupSlug(String);

impl GroupSlug {
    /// Минимальная длина.
    pub const MIN_LEN: usize = 3;
    /// Максимальная длина.
    pub const MAX_LEN: usize = 30;

    /// Распарсить и нормализовать слаг.
    ///
    /// # Errors
    /// [`GroupSlugError`], если длина или символы нарушают правила.
    pub fn parse(input: &str) -> Result<Self, GroupSlugError> {
        let normalized = input.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Err(GroupSlugError::Empty);
        }
        let len = normalized.chars().count();
        if len < Self::MIN_LEN {
            return Err(GroupSlugError::TooShort { len });
        }
        if len > Self::MAX_LEN {
            return Err(GroupSlugError::TooLong { len });
        }
        for (i, c) in normalized.chars().enumerate() {
            let ok = if i == 0 {
                c.is_ascii_lowercase()
            } else {
                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'
            };
            if !ok {
                return Err(if i == 0 {
                    GroupSlugError::InvalidStart
                } else {
                    GroupSlugError::InvalidChar(c)
                });
            }
        }
        Ok(Self(normalized))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Нарушение правил [`GroupSlug`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GroupSlugError {
    #[error("слаг пустой")]
    Empty,
    #[error("слаг слишком короткий: {len} символов")]
    TooShort { len: usize },
    #[error("слаг слишком длинный: {len} символов")]
    TooLong { len: usize },
    #[error("слаг должен начинаться с латинской буквы")]
    InvalidStart,
    #[error("недопустимый символ в слаге: {0:?}")]
    InvalidChar(char),
}

/// Тип сообщества. Определяет вступление, чтение и право публикации.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupKind {
    /// Закрытая группа: вступление — только по приглашению модератора; читать и
    /// писать могут только участники. Ощущение приватного чата.
    Closed,
    /// Паблик: вступление свободное, читают все; публикуют только модераторы.
    /// Ощущение вещания.
    Public,
}

impl GroupKind {
    /// Каноническое строковое представление.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::Public => "public",
        }
    }

    /// Распарсить из строки (регистронезависимо).
    ///
    /// # Errors
    /// [`GroupKindError`], если значение не из известного набора.
    pub fn parse(input: &str) -> Result<Self, GroupKindError> {
        match input.trim().to_ascii_lowercase().as_str() {
            "closed" => Ok(Self::Closed),
            "public" => Ok(Self::Public),
            _ => Err(GroupKindError),
        }
    }

    /// Можно ли вступить самостоятельно, без приглашения.
    #[must_use]
    pub const fn allows_self_join(self) -> bool {
        matches!(self, Self::Public)
    }
}

/// Значение типа сообщества не из известного набора.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("неизвестный тип сообщества")]
pub struct GroupKindError;

/// Роль участника в сообществе.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MembershipRole {
    /// Владелец: полный контроль, нельзя убрать.
    Owner,
    /// Модератор: управляет участниками и публикацией.
    Moderator,
    /// Рядовой участник.
    Member,
}

impl MembershipRole {
    /// Каноническое строковое представление.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Moderator => "moderator",
            Self::Member => "member",
        }
    }

    /// Распарсить из строки (регистронезависимо).
    ///
    /// # Errors
    /// [`MembershipRoleError`], если значение не из известного набора.
    pub fn parse(input: &str) -> Result<Self, MembershipRoleError> {
        match input.trim().to_ascii_lowercase().as_str() {
            "owner" => Ok(Self::Owner),
            "moderator" => Ok(Self::Moderator),
            "member" => Ok(Self::Member),
            _ => Err(MembershipRoleError),
        }
    }

    /// Имеет ли роль права модерации (управление участниками/публикацией).
    #[must_use]
    pub const fn can_moderate(self) -> bool {
        matches!(self, Self::Owner | Self::Moderator)
    }
}

/// Значение роли не из известного набора.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("неизвестная роль участника")]
pub struct MembershipRoleError;

/// Членство юзера в сообществе.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Membership {
    pub user: UserId,
    pub role: MembershipRole,
}

/// Фантомный маркер для типизированного [`GroupId`].
pub enum GroupMarker {}
/// Идентификатор сообщества.
pub type GroupId = Id<GroupMarker>;

/// Сообщество — корень агрегата. Членство и роли держатся внутри агрегата, чтобы
/// инварианты (всегда есть владелец, права на действия) проверялись в одном месте.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    id: GroupId,
    slug: GroupSlug,
    name: GroupName,
    kind: GroupKind,
    members: Vec<Membership>,
    created_at: Timestamp,
}

impl Group {
    /// Основать сообщество. Основатель становится владельцем ([`MembershipRole::Owner`]).
    #[must_use]
    pub fn found(
        id: GroupId,
        slug: GroupSlug,
        name: GroupName,
        kind: GroupKind,
        founder: UserId,
        now: Timestamp,
    ) -> (Self, GroupFounded) {
        let group = Self {
            id,
            slug,
            name,
            kind,
            members: vec![Membership {
                user: founder,
                role: MembershipRole::Owner,
            }],
            created_at: now,
        };
        let event = GroupFounded {
            group: id,
            owner: founder,
            kind,
            founded_at: now,
        };
        (group, event)
    }

    /// Самостоятельно вступить в сообщество (только паблик).
    ///
    /// # Errors
    /// - [`CommunityError::JoinNotAllowed`] — в закрытую группу так нельзя.
    /// - [`CommunityError::AlreadyMember`] — юзер уже участник.
    pub fn join(&mut self, user: UserId, now: Timestamp) -> Result<MemberJoined, CommunityError> {
        if !self.kind.allows_self_join() {
            return Err(CommunityError::JoinNotAllowed);
        }
        if self.is_member(user) {
            return Err(CommunityError::AlreadyMember);
        }
        self.members.push(Membership {
            user,
            role: MembershipRole::Member,
        });
        Ok(MemberJoined {
            group: self.id,
            user,
            role: MembershipRole::Member,
            at: now,
        })
    }

    /// Добавить участника по решению модератора (путь вступления в закрытую группу).
    ///
    /// # Errors
    /// - [`CommunityError::NotPermitted`] — `actor` не модератор.
    /// - [`CommunityError::CannotAssignOwner`] — владельца так не назначают.
    /// - [`CommunityError::AlreadyMember`] — юзер уже участник.
    pub fn add_member(
        &mut self,
        actor: UserId,
        user: UserId,
        role: MembershipRole,
        now: Timestamp,
    ) -> Result<MemberJoined, CommunityError> {
        self.require_moderator(actor)?;
        if role == MembershipRole::Owner {
            return Err(CommunityError::CannotAssignOwner);
        }
        if self.is_member(user) {
            return Err(CommunityError::AlreadyMember);
        }
        self.members.push(Membership { user, role });
        Ok(MemberJoined {
            group: self.id,
            user,
            role,
            at: now,
        })
    }

    /// Сменить роль участника. Только владелец; нельзя оставить группу без владельца.
    ///
    /// # Errors
    /// - [`CommunityError::NotPermitted`] — `actor` не владелец.
    /// - [`CommunityError::TargetNotMember`] — цель не участник.
    /// - [`CommunityError::SoleOwner`] — попытка снять последнего владельца.
    pub fn set_role(
        &mut self,
        actor: UserId,
        target: UserId,
        role: MembershipRole,
        now: Timestamp,
    ) -> Result<MemberRoleChanged, CommunityError> {
        if self.role_of(actor) != Some(MembershipRole::Owner) {
            return Err(CommunityError::NotPermitted);
        }
        let old_role = self
            .role_of(target)
            .ok_or(CommunityError::TargetNotMember)?;
        if old_role == MembershipRole::Owner
            && role != MembershipRole::Owner
            && self.owner_count() == 1
        {
            return Err(CommunityError::SoleOwner);
        }
        if let Some(m) = self.members.iter_mut().find(|m| m.user == target) {
            m.role = role;
        }
        Ok(MemberRoleChanged {
            group: self.id,
            user: target,
            old_role,
            new_role: role,
            at: now,
        })
    }

    /// Убрать участника (модерация). Владельца убрать нельзя.
    ///
    /// # Errors
    /// - [`CommunityError::NotPermitted`] — `actor` не модератор.
    /// - [`CommunityError::TargetNotMember`] — цель не участник.
    /// - [`CommunityError::CannotRemoveOwner`] — цель — владелец.
    pub fn remove_member(
        &mut self,
        actor: UserId,
        target: UserId,
        now: Timestamp,
    ) -> Result<MemberRemoved, CommunityError> {
        self.require_moderator(actor)?;
        let role = self
            .role_of(target)
            .ok_or(CommunityError::TargetNotMember)?;
        if role == MembershipRole::Owner {
            return Err(CommunityError::CannotRemoveOwner);
        }
        self.members.retain(|m| m.user != target);
        Ok(MemberRemoved {
            group: self.id,
            user: target,
            at: now,
        })
    }

    /// Выйти из сообщества. Последний владелец сначала передаёт владение.
    ///
    /// # Errors
    /// - [`CommunityError::NotMember`] — юзер не участник.
    /// - [`CommunityError::SoleOwner`] — единственный владелец не может выйти.
    pub fn leave(&mut self, user: UserId, now: Timestamp) -> Result<MemberLeft, CommunityError> {
        let role = self.role_of(user).ok_or(CommunityError::NotMember)?;
        if role == MembershipRole::Owner && self.owner_count() == 1 {
            return Err(CommunityError::SoleOwner);
        }
        self.members.retain(|m| m.user != user);
        Ok(MemberLeft {
            group: self.id,
            user,
            at: now,
        })
    }

    /// Может ли юзер публиковать в сообщество: в закрытой группе — любой участник,
    /// в паблике — только модераторы.
    #[must_use]
    pub fn can_post(&self, user: UserId) -> bool {
        match self.role_of(user) {
            None => false,
            Some(role) => match self.kind {
                GroupKind::Closed => true,
                GroupKind::Public => role.can_moderate(),
            },
        }
    }

    /// Может ли наблюдатель читать сообщество: паблик — все, закрытая — участники.
    #[must_use]
    pub fn allows_read(&self, viewer: Option<UserId>) -> bool {
        match self.kind {
            GroupKind::Public => true,
            GroupKind::Closed => viewer.is_some_and(|u| self.is_member(u)),
        }
    }

    /// Роль юзера в сообществе, если он участник.
    #[must_use]
    pub fn role_of(&self, user: UserId) -> Option<MembershipRole> {
        self.members.iter().find(|m| m.user == user).map(|m| m.role)
    }

    #[must_use]
    pub fn is_member(&self, user: UserId) -> bool {
        self.role_of(user).is_some()
    }

    #[must_use]
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    fn owner_count(&self) -> usize {
        self.members
            .iter()
            .filter(|m| m.role == MembershipRole::Owner)
            .count()
    }

    fn require_moderator(&self, actor: UserId) -> Result<(), CommunityError> {
        match self.role_of(actor) {
            Some(role) if role.can_moderate() => Ok(()),
            _ => Err(CommunityError::NotPermitted),
        }
    }

    #[must_use]
    pub const fn id(&self) -> GroupId {
        self.id
    }

    #[must_use]
    pub fn slug(&self) -> &GroupSlug {
        &self.slug
    }

    #[must_use]
    pub fn name(&self) -> &GroupName {
        &self.name
    }

    #[must_use]
    pub const fn kind(&self) -> GroupKind {
        self.kind
    }

    #[must_use]
    pub fn members(&self) -> &[Membership] {
        &self.members
    }

    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }
}

/// Нарушение правил сообщества.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommunityError {
    #[error("в эту группу нельзя вступить самостоятельно")]
    JoinNotAllowed,
    #[error("уже участник сообщества")]
    AlreadyMember,
    #[error("действие требует прав модератора")]
    NotPermitted,
    #[error("владельца нельзя назначить этим действием")]
    CannotAssignOwner,
    #[error("цель не участник сообщества")]
    TargetNotMember,
    #[error("нельзя убрать владельца сообщества")]
    CannotRemoveOwner,
    #[error("нельзя оставить сообщество без владельца")]
    SoleOwner,
    #[error("не участник сообщества")]
    NotMember,
}

/// Сообщество основано.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupFounded {
    pub group: GroupId,
    pub owner: UserId,
    pub kind: GroupKind,
    pub founded_at: Timestamp,
}

/// Участник вступил/добавлен.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberJoined {
    pub group: GroupId,
    pub user: UserId,
    pub role: MembershipRole,
    pub at: Timestamp,
}

/// Роль участника изменена.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRoleChanged {
    pub group: GroupId,
    pub user: UserId,
    pub old_role: MembershipRole,
    pub new_role: MembershipRole,
    pub at: Timestamp,
}

/// Участник убран модератором.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRemoved {
    pub group: GroupId,
    pub user: UserId,
    pub at: Timestamp,
}

/// Участник вышел сам.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberLeft {
    pub group: GroupId,
    pub user: UserId,
    pub at: Timestamp,
}

/// Доменное событие контекста community.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommunityEvent {
    GroupFounded(GroupFounded),
    MemberJoined(MemberJoined),
    MemberRoleChanged(MemberRoleChanged),
    MemberRemoved(MemberRemoved),
    MemberLeft(MemberLeft),
}

impl From<GroupFounded> for CommunityEvent {
    fn from(event: GroupFounded) -> Self {
        Self::GroupFounded(event)
    }
}
impl From<MemberJoined> for CommunityEvent {
    fn from(event: MemberJoined) -> Self {
        Self::MemberJoined(event)
    }
}
impl From<MemberRoleChanged> for CommunityEvent {
    fn from(event: MemberRoleChanged) -> Self {
        Self::MemberRoleChanged(event)
    }
}
impl From<MemberRemoved> for CommunityEvent {
    fn from(event: MemberRemoved) -> Self {
        Self::MemberRemoved(event)
    }
}
impl From<MemberLeft> for CommunityEvent {
    fn from(event: MemberLeft) -> Self {
        Self::MemberLeft(event)
    }
}

/// Хранилище сообществ (порт; реализация — в `infrastructure`).
#[async_trait]
pub trait GroupRepository: Send + Sync {
    async fn find_by_id(&self, id: GroupId) -> Result<Option<Group>, RepositoryError>;
    async fn find_by_slug(&self, slug: &GroupSlug) -> Result<Option<Group>, RepositoryError>;
    async fn save(&self, group: &Group) -> Result<(), RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid() -> UserId {
        Id::generate()
    }

    fn group(kind: GroupKind, owner: UserId) -> Group {
        Group::found(
            Id::generate(),
            GroupSlug::parse("podval").unwrap(),
            GroupName::parse("Подвал").unwrap(),
            kind,
            owner,
            Timestamp::now(),
        )
        .0
    }

    #[test]
    fn slug_normalizes_like_handle() {
        assert_eq!(GroupSlug::parse(" Podval_1 ").unwrap().as_str(), "podval_1");
        assert_eq!(
            GroupSlug::parse("ab"),
            Err(GroupSlugError::TooShort { len: 2 })
        );
        assert_eq!(GroupSlug::parse("1bad"), Err(GroupSlugError::InvalidStart));
    }

    #[test]
    fn kind_and_role_roundtrip() {
        assert_eq!(GroupKind::parse("PUBLIC").unwrap(), GroupKind::Public);
        assert_eq!(GroupKind::parse("xz"), Err(GroupKindError));
        assert_eq!(
            MembershipRole::parse("Owner").unwrap(),
            MembershipRole::Owner
        );
        assert!(MembershipRole::Moderator.can_moderate());
        assert!(!MembershipRole::Member.can_moderate());
    }

    #[test]
    fn founder_is_owner() {
        let owner = uid();
        let (g, event) = Group::found(
            Id::generate(),
            GroupSlug::parse("podval").unwrap(),
            GroupName::parse("Подвал").unwrap(),
            GroupKind::Public,
            owner,
            Timestamp::now(),
        );
        assert_eq!(g.role_of(owner), Some(MembershipRole::Owner));
        assert_eq!(g.member_count(), 1);
        assert_eq!(event.owner, owner);
    }

    #[test]
    fn self_join_public_then_blocks_duplicate() {
        let mut g = group(GroupKind::Public, uid());
        let u = uid();
        assert_eq!(
            g.join(u, Timestamp::now()).unwrap().role,
            MembershipRole::Member
        );
        assert_eq!(
            g.join(u, Timestamp::now()).unwrap_err(),
            CommunityError::AlreadyMember
        );
    }

    #[test]
    fn self_join_closed_is_blocked() {
        let mut g = group(GroupKind::Closed, uid());
        assert_eq!(
            g.join(uid(), Timestamp::now()).unwrap_err(),
            CommunityError::JoinNotAllowed
        );
    }

    #[test]
    fn moderator_adds_member_but_not_owner() {
        let owner = uid();
        let mut g = group(GroupKind::Closed, owner);
        let newbie = uid();
        assert!(
            g.add_member(owner, newbie, MembershipRole::Member, Timestamp::now())
                .is_ok()
        );
        assert!(g.is_member(newbie));
        assert_eq!(
            g.add_member(owner, uid(), MembershipRole::Owner, Timestamp::now())
                .unwrap_err(),
            CommunityError::CannotAssignOwner
        );
    }

    #[test]
    fn non_moderator_cannot_add() {
        let mut g = group(GroupKind::Closed, uid());
        let stranger = uid();
        assert_eq!(
            g.add_member(stranger, uid(), MembershipRole::Member, Timestamp::now())
                .unwrap_err(),
            CommunityError::NotPermitted
        );
    }

    #[test]
    fn cannot_demote_or_remove_sole_owner() {
        let owner = uid();
        let mut g = group(GroupKind::Closed, owner);
        assert_eq!(
            g.set_role(owner, owner, MembershipRole::Member, Timestamp::now())
                .unwrap_err(),
            CommunityError::SoleOwner
        );
        assert_eq!(
            g.leave(owner, Timestamp::now()).unwrap_err(),
            CommunityError::SoleOwner
        );
    }

    #[test]
    fn owner_promotes_then_can_step_down() {
        let owner = uid();
        let mut g = group(GroupKind::Closed, owner);
        let heir = uid();
        g.add_member(owner, heir, MembershipRole::Member, Timestamp::now())
            .unwrap();
        g.set_role(owner, heir, MembershipRole::Owner, Timestamp::now())
            .unwrap();
        assert_eq!(g.role_of(heir), Some(MembershipRole::Owner));
        // теперь владельцев двое — прежний может выйти
        assert!(g.leave(owner, Timestamp::now()).is_ok());
        assert_eq!(g.role_of(owner), None);
    }

    #[test]
    fn remove_member_guards_owner_and_membership() {
        let owner = uid();
        let mut g = group(GroupKind::Closed, owner);
        let member = uid();
        g.add_member(owner, member, MembershipRole::Member, Timestamp::now())
            .unwrap();
        assert!(g.remove_member(owner, member, Timestamp::now()).is_ok());
        assert!(!g.is_member(member));
        assert_eq!(
            g.remove_member(owner, owner, Timestamp::now()).unwrap_err(),
            CommunityError::CannotRemoveOwner
        );
        assert_eq!(
            g.remove_member(owner, uid(), Timestamp::now()).unwrap_err(),
            CommunityError::TargetNotMember
        );
    }

    #[test]
    fn posting_policy_differs_by_kind() {
        let owner = uid();
        let member = uid();

        let mut closed = group(GroupKind::Closed, owner);
        closed
            .add_member(owner, member, MembershipRole::Member, Timestamp::now())
            .unwrap();
        assert!(closed.can_post(owner));
        assert!(closed.can_post(member)); // в закрытой пишут все участники
        assert!(!closed.can_post(uid())); // не участник — нет

        let mut public = group(GroupKind::Public, owner);
        public.join(member, Timestamp::now()).unwrap();
        assert!(public.can_post(owner)); // модератор/владелец — да
        assert!(!public.can_post(member)); // рядовой участник паблика — нет
    }

    #[test]
    fn read_policy_differs_by_kind() {
        let owner = uid();
        let closed = group(GroupKind::Closed, owner);
        assert!(closed.allows_read(Some(owner)));
        assert!(!closed.allows_read(Some(uid())));
        assert!(!closed.allows_read(None));

        let public = group(GroupKind::Public, owner);
        assert!(public.allows_read(None));
    }
}
