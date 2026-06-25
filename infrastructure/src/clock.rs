use babangida_application::Clock;
use babangida_shared::Timestamp;

/// Системные часы (UTC).
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}
