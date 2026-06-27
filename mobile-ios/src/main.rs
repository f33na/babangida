//! Бинарь-точка входа под устройство для `dx` (Dioxus CLI). Только под фичей `shell`
//! (нативный рендерер); без неё — пустой `main`, чтобы `cargo check --workspace` не
//! тянул device-тулчейн. Сборка: `dx serve|build --platform ios` (нужен Xcode, ADR-0007).

fn main() {
    #[cfg(feature = "shell")]
    babangida_mobile_ios::launch();
}
