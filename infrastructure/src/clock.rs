use babangida_application::Clock;
use babangida_shared::Timestamp;

/// Системные часы (UTC). Без состояния — `Copy`, чтобы свободно передавать в
/// несколько use-case'ов.
#[derive(Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}
