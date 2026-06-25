---
status: in-progress
owner: @fen
priority: P1
created: 2026-06-25
target: 2026-07-20
---

# Messaging & Communities — переписка и сообщества

## Why

После сквозного среза identity нужен социальный слой: личная переписка и сообщества
(закрытые группы и паблики). Это держит людей внутри сети, не выводя их в сторонние
мессенджеры. Анти-ВК: всё в общей ленте/нави, не отдельными приложениями.

## Scope

Контексты `messaging` (DM) и `community` (группы/паблики). В этой итерации (Prompt 5):
доменное ядро + use-cases (application). НЕ здесь: HTTP-срез (infra+api), UI, связь
пост↔сообщество в `content`, граф контактов, настройки приватности DM.

## Success criteria

Инварианты держатся в домене: писать в диалог может только участник; в сообществе
всегда есть владелец, право публикации зависит от типа (closed — участники, public —
модераторы), вступление в closed — только по приглашению. Юнит-тесты зелёные.

## Technical

Модель — [[decisions/0012-messaging-community-model]] (**Proposed**). Источник правды —
[[decisions/0003-domain-as-source-of-truth]]; чтение — [[decisions/0004-cqrs-read-write-split]];
соцфункции без верификации — [[decisions/0010-verification-model]]. Атомарность вступления/
ролей под конкуренцией — по образцу [[decisions/0011-invite-issuance-atomicity]] (на HTTP-срезе).

## Implementation

Ветка `feat/identity-invites` (пока единый PR; messaging-ветку отделим при мердже).
`domain`: контексты `messaging` (`Conversation`/`Message`) и `community` (`Group` + роли).
`application`: `SendMessage`, `FoundGroup`/`JoinGroup`/`LeaveGroup`/`SetMemberRole`,
read-модели `Inbox`/`Thread`/`Group`.

## Log

- 2026-06-25: создано; domain+application (Prompt 5). ADR-0012 — Proposed, ждёт акцепта на ревью.
