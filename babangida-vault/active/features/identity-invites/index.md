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

PR: _в работе на ветке `feat/identity-invites`_.

## Log

- 2026-06-25: создано; идёт реализация infrastructure (sqlx) + api (axum).
