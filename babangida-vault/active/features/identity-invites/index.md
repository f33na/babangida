---
status: in-progress
owner: @fen
priority: P0
created: 2026-06-25
target: 2026-07-10
---

# Identity & Invites — первый сквозной срез

## Why

Закрытый вход по инвайтам — гейт качества комьюнити (ADR-0005). Без него нет ни
регистрации, ни ленты, ни профиля. Это фундамент всего продукта.

## Scope

Сквозной срез: выдача/приём инвайта, регистрация по инвайту, профиль, лента (чтение).
НЕ в этой итерации: верификация-флоу, messaging, маркет, музыка, медиа в постах.

## Success criteria

Юзер по инвайту регистрируется, получает профиль, постит, видит ленту; квота 2 +
кулдаун 12ч держатся под конкуренцией (ADR-0011); интеграционные тесты против БД зелёные.

## Technical

Домен/CQRS — Prompt 1 ([[decisions/0003-domain-as-source-of-truth]],
[[decisions/0004-cqrs-read-write-split]]). Инвайт-инвариант — [[decisions/0005-invite-model]].
Атомарность выдачи — [[decisions/0011-invite-issuance-atomicity]]. БД/миграции —
[[decisions/0009-nix-flake-ci-db]]. Паттерны: [[patterns/repository]], [[patterns/error-handling]].

## Implementation

Ветка `feat/identity-invites` (PR появится с remote). Сквозной срез реализован:
`infrastructure` (sqlx, atomic issuance/registration) + `api` (axum) + миграции.

## Log

- 2026-06-25: создано.
- 2026-06-25: domain+application (Prompt 1).
- 2026-06-25: infrastructure + api; сквозной HTTP-тест против БD из nix зелёный
  (выдача → регистрация → профиль → пост → лента; квота/кулдаун держатся — ADR-0011).
  Статус остаётся `in-progress` до ревью/мерджа.
