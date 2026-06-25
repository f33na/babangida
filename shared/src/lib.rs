//! Shared kernel babangida: cross-cutting примитивы (идентификаторы, время) без
//! бизнес-логики и без внешних фреймворков. Самый внутренний слой — не зависит
//! ни на один другой крейт workspace. См. `../../babangida-vault/COMMON.md`.

mod id;
mod time;

pub use id::{Id, IdParseError};
pub use time::{Duration, Timestamp};
