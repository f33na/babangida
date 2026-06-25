//! Контекст identity-invites: юзеры, инвайты, верификация.
//!
//! Здесь живёт критический инвариант инвайта (ADR-0005): максимум
//! [`MAX_ACTIVE_INVITES`] активных приглашений, кулдаун [`INVITE_COOLDOWN`] между
//! выдачами, админ — без лимита. Единственный путь выпустить инвайт — гейтированный
//! конструктор [`Invite::issue`].

mod invite;
mod repository;
mod specifications;
mod user;
mod value_objects;

pub use invite::{
    INVITE_COOLDOWN, IdentityEvent, Invite, InviteAccepted, InviteError, InviteId, InviteIssued,
    InviteMarker, InviteStatus, IssuanceContext, MAX_ACTIVE_INVITES,
};
pub use repository::{InviteRepository, UserRepository};
pub use specifications::{ActiveInvite, VerifiedUser};
pub use user::{User, UserId, UserMarker};
pub use value_objects::{
    Handle, HandleError, InviteCode, InviteCodeError, InviteQuota, UserRole, VerifiedStatus,
};
