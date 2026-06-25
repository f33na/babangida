# Стек и версии

Канонический список версий проекта (ADR-0001). Точные пины внешних крейтов —
`Cargo.toml` (`[workspace.dependencies]`); тулчейн — `rust-toolchain.toml`. Этот
файл — человекочитаемая сводка «что и почему».

## Язык и сборка

- Rust stable, pinned `1.95.0`, edition `2024`. Один язык на весь стек.
- Cargo workspace, монорепо (ADR-0002). Окружение/CI/БД — Nix flake (ADR-0009).

## Backend

- `axum` 0.8 — HTTP. `tokio` 1 — рантайм. `tower`/`tower-http` — middleware.
- PostgreSQL 16 + `sqlx` 0.9 (postgres, tls-rustls, миграции). Без ORM — SQL явный.
  `sqlx-cli` приходит из nixpkgs (сейчас 0.9.0); версия трекает nixpkgs.

## Web и mobile

- Web: `leptos` 0.7, SSR + гидратация (ADR-0006). Target `wasm32-unknown-unknown`.
- Mobile: `dioxus` 0.6, нативный шелл (ADR-0007 — самая рискованная ставка).
- UI шарится через `uikit` (ADR-0008): токены и логика, не цельный рендер.

## Cross-cutting

- `serde`/`serde_json` — сериализация. `thiserror` 2 — ошибки. `uuid` 1, `time` 0.3.
- `tracing` — логи/трейсинг.

## Инструменты разработки

- `sqlx-cli` (миграции), `pg-start`/`pg-stop` (локальный postgres из flake).
- CI: `nix run .#ci` → fmt + clippy (`-D warnings`) + check + test.
