---
status: in-progress
owner: @fen
priority: P1
created: 2026-06-27
target: 2026-07-25
---

# Verification — процесс заявки на верификацию

## Why

ADR-0010 ввёл верификацию как гейт привилегий и пометил сам процесс будущей работой.
Пока (ADR-0014) статус выдаётся вручную: админ зовёт `VerifyUser` по своему усмотрению,
у юзера нет точки входа, у админа — очереди. Непрозрачно и не масштабируется. Нужен
процесс: юзер подаёт заявку → админ рассматривает (одобряет/отклоняет с причиной).
Анти-ВК: подача — в профиле, очередь — экран админа в той же сети.

## Scope

Полный вертикальный срез: контекст `verification` (домен + application + infra + api) +
web-UI. НЕ здесь: авто-критерии верификации, уведомления (пуш/почта), отзыв
верификации (де-верификация), полноценный журнал решений, мобильный UI.

## Success criteria

Юзер подаёт заявку (одна открытая за раз; уже верифицированному — нельзя). Очередь
видит только админ. Решает только админ; одобрение атомарно делает юзера verified и
открывает маркет. Отказ с причиной не финален — можно подать новую. Повторное решение
по заявке отклоняется. Юнит- и интеграционные тесты против БД зелёные; web компилируется
на ssr и wasm/hydrate.

## Technical

Модель — [[decisions/0016-verification-process]] (Proposed, ждёт акцепта). Гейт — из
[[decisions/0010-verification-model]]; атомарность одобрения — по образцу
[[decisions/0011-invite-issuance-atomicity]]/[[decisions/0012-messaging-community-model]];
чтение — read-модели [[decisions/0004-cqrs-read-write-split]]; не трогаем замороженный
домен [[decisions/0003-domain-as-source-of-truth]] — у нового контекста честный
`reconstitute`.

## Implementation

Ветка `master`. `domain/verification`: `VerificationRequest` (+ `RequestNote`/
`DecisionReason`/`RequestStatus`), машина `pending → approved | rejected`
(`approve`/`reject` только из `pending`), порт `VerificationRequestRepository`,
`VerificationError`. `application`: `RequestVerification`/`ApproveVerification`/
`RejectVerification`, порт `VerificationDecisionTx` (атомарно заявка+статус), read-модели
`PendingVerifications`/`MyVerification`, `ApplicationError` += `Verification`/`Conflict`.
`infrastructure`: миграция `0007` (партиал-уникальный индекс на `pending`),
`PgVerificationRequestRepository`, `PgVerificationReadModel`,
`PgVerificationDecisionTxFactory` (`SELECT FOR UPDATE`). `api`: `POST /verification/requests`,
`GET /verification/requests` (очередь, админ), `GET /verification/me`,
`POST /verification/requests/{id}/approve|reject` (админ). web (Leptos): экран
`/verification` (свой статус + форма заявки/повторной заявки + очередь админа с
approve/reject), приглашение в своём профиле, ссылка в нав.

## Log

- 2026-06-27: полный срез фичи. Домен `verification` (6 юнит-тестов), application
  (`RequestVerification`/`ApproveVerification`/`RejectVerification` + read-модели,
  7 юнит-тестов с фейками), infra (миграция 0007 + Pg-адаптеры + решающая транзакция),
  api (5 эндпоинтов + маппинг 409/403). Интеграционный тест `verification_it` против
  postgres зелёный: заявка → дубль 409 → очередь только админ → не-админ не решает →
  одобрение верифицирует и открывает маркет → повтор/повторная заявка 409 → отказ с
  причиной пускает новую заявку (re-request). web: экран `/verification` + приглашение
  в профиле + ссылка в нав; ssr и wasm/hydrate компилируются. ADR-0016 — Proposed,
  ждёт акцепта пользователя.
