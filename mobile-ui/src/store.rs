//! Секьюр-стор токена сессии (ADR-0015). iOS/macOS — Keychain через `keyring`
//! (компилируется и работает на хосте). Android — отдельный путь к Keystore через JNI;
//! пока заглушка (токен живёт только в памяти сессии), реализуется при упаковке под
//! Android (ADR-0007). Единый интерфейс: [`load`]/[`save`]/[`clear`].

const SERVICE: &str = "babangida";
const ACCOUNT: &str = "session-token";

#[cfg(not(target_os = "android"))]
mod imp {
    use super::{ACCOUNT, SERVICE};

    fn entry() -> Option<keyring::Entry> {
        keyring::Entry::new(SERVICE, ACCOUNT).ok()
    }

    /// Прочитать сохранённый токен (`None`, если нет/ошибка стора).
    pub fn load() -> Option<String> {
        entry()?.get_password().ok()
    }

    /// Сохранить токен сессии.
    pub fn save(token: &str) {
        if let Some(e) = entry() {
            let _ = e.set_password(token);
        }
    }

    /// Удалить токен (logout).
    pub fn clear() {
        if let Some(e) = entry() {
            let _ = e.delete_credential();
        }
    }
}

#[cfg(target_os = "android")]
mod imp {
    //! Android: Keystore через JNI — TODO при упаковке под Android (ADR-0007/0015).
    //! Пока no-op: токен живёт только в памяти текущей сессии.
    pub fn load() -> Option<String> {
        None
    }
    pub fn save(_token: &str) {}
    pub fn clear() {}
}

pub use imp::{clear, load, save};
