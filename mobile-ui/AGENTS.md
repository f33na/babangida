# AGENTS.md — `mobile-ui`

Перед работой прочитай `../babangida-vault/COMMON.md`. Релевантные ADR: 0007, 0008, 0015.

## Роль

Общий mobile-UI: Dioxus-экраны, сигнальная навигация и data-client поверх HTTP-API
(ADR-0015). Шеллы `mobile-ios`/`mobile-android` — тонкие, делегируют в `App`; нативный
рендерер включают они (фича `shell`).

## Границы

- Зависит на `uikit` (фича `dioxus`: токены + Dioxus-виджеты) и сеть (`reqwest`).
- Данные — прямой HTTP к тому же API, что web (модуль `client`), не через
  `infrastructure`/`application` напрямую. DTO минимальные, зеркалят JSON `api`.
- Рендерер сюда не тянем (его включают шеллы) — `cargo check` собирает UI-ядро.
- Тема пока тёмная (инлайн-токены); навигация — сигнал-enum (без `dioxus-router`).
- Сессия — токен в памяти; секьюр-стор/персист — после PoC (ADR-0015).