# AGENTS.md — `web`

Перед работой прочитай `../babangida-vault/COMMON.md`. Релевантные ADR: 0006, 0008.

## Роль

Web-клиент: Leptos SSR (+ гидратация, ADR-0006). Экраны строятся из `uikit`
(`features=["leptos"]`), данные — из HTTP `api` через server-функции (reqwest).

## Границы

- Зависит на `uikit` + leptos-экосистему; на бэкенд — только по HTTP (не на `application`/`infrastructure`).
- `ssr`/`hydrate` — взаимоисключающие фичи; reqwest/axum/tokio только под `ssr`; в wasm-бандл серверное не попадает.
- axum здесь 0.7 (этого требует `leptos_axum` 0.7), `api` — на 0.8. Сборка — `cargo leptos` из flake.
- Фичи держим плотно в общей ленте/профиле/нави — не отдельными приложениями.
