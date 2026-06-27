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
доменное ядро + use-cases + сквозной HTTP-срез (миграции, infra, api, тесты против БД)
+ публикация в сообщество (контент паблика — в общей ленте, анти-ВК).
НЕ здесь: UI, граф контактов, настройки приватности DM, лента закрытой группы (нужна auth).

## Success criteria

Инварианты держатся в домене: писать в диалог может только участник; в сообществе
всегда есть владелец, право публикации зависит от типа (closed — участники, public —
модераторы), вступление в closed — только по приглашению. Под конкуренцией инвариант
владельца держит блокировка строки группы. Юнит- и интеграционные тесты против БД зелёные.

## Technical

Модель — [[decisions/0012-messaging-community-model]] (Accepted). Источник правды —
[[decisions/0003-domain-as-source-of-truth]]; чтение — [[decisions/0004-cqrs-read-write-split]];
соцфункции без верификации — [[decisions/0010-verification-model]]. Атомарность вступления/
ролей под конкуренцией — по образцу [[decisions/0011-invite-issuance-atomicity]] (на HTTP-срезе).

## Implementation

Ветка `feat/identity-invites` (пока единый PR; messaging-ветку отделим при мердже).
`domain`: контексты `messaging` (`Conversation`/`Message`) и `community` (`Group` + роли).
`application`: `SendMessage`, `FoundGroup`/`JoinGroup`/`LeaveGroup`/`SetMemberRole`,
read-модели `Inbox`/`Thread`/`Group`, tx-порт членства `GroupMembershipTx`.
`infrastructure`: миграция `0003`, Pg-репозитории/read-модели, атомарная tx членства
(блокировка строки группы). `api`: `/messages`,`/inbox`,`/conversations/{id}/thread`,
`/groups*`. Гонка создания диалога закрыта UNIQUE-парой + перечитыванием.

## Log

- 2026-06-25: создано; domain+application (Prompt 5). ADR-0012 — Proposed, ждёт акцепта на ревью.
- 2026-06-26: HTTP-срез (часть 2) — миграции/infra/api; атомарность членства (блокировка
  группы, ADR-0012). e2e и concurrency-тесты против postgres зелёные. ADR-0012 всё ещё Proposed.
- 2026-06-26: связь пост↔сообщество (миграция `0004` `group_posts`; `PostToGroup` +
  `Group::authorize_post`; общая лента отдаёт посты пабликов с меткой, посты закрытых
  скрыты от анонимной ленты). Агрегат `content::Post` не тронут. e2e против БД зелёный.
- 2026-06-26: ADR-0012 акцептован (Accepted) пользователем.
- 2026-06-27: web-UI P1 — страница сообщества `/g/:slug` (карточка `GET /groups/{slug}`:
  имя, тип, счётчик участников) + вступление (`POST /groups/{id}/join`) и пост в сообщество
  (`POST /groups/{id}/posts`) для залогиненных. Вход на страницу — по чипу группы в общей
  ленте (анти-ВК: сообщество — срез ленты, не отдельное приложение). Лента получила composer
  поста (`POST /posts`) и чип группы у постов пабликов. uikit += `TextArea`. ssr+wasm зелёные.
- 2026-06-27: web-UI личные сообщения. Инбокс `/messages` (`GET /inbox`: собеседник +
  последнее сообщение), тред `/messages/:id?with=handle` (`GET /conversations/{id}/thread`)
  с ответом, старт переписки с чужого профиля («написать @handle»). Узкое место «recipient —
  UserId, не handle» снято: server-fn `send_message(recipient_handle)` сам резолвит handle→UserId
  через `GET /profiles/{handle}`, UI оперирует только handle. Нав += «сообщения». Анти-ВК: DM
  стартует с профиля и живёт в одном инбоксе, не отдельным мессенджером. ssr+wasm зелёные.
- 2026-06-27: лента стала viewer-aware (закрыт бэкенд-пробел). Раньше пост закрытой группы
  не видел никто; теперь его видит участник в общей ленте (зеркало доменного read-правила
  closed=участники, ADR-0012). `FeedReadModel.recent` += `viewer: Option<UserId>`, SQL +=
  `EXISTS group_members` под зрителя (анонимной выдаче — только публичное). api: `/feed` берёт
  `Option<CurrentUser>` (новый `OptionalFromRequestParts` для `CurrentUser`: нет/истёкший токен
  → аноним, сбой хранилища → наружу). Интеграционный тест: аноним и не-участник пост бункера
  не видят, участник — видит с меткой группы. Все it-тесты против postgres зелёные.
