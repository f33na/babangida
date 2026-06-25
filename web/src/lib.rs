//! Web-клиент babangida: Leptos SSR (ADR-0006). Экраны строятся из `babangida-uikit`,
//! данные — через HTTP API. См. `../../babangida-vault/COMMON.md`.

pub mod app;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(app::App);
}
