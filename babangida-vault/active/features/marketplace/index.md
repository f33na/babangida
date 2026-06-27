---
status: in-progress
owner: @fen
priority: P1
created: 2026-06-26
target: 2026-07-25
---

# Marketplace — барахолка за гейтом верификации

## Why

Первая привилегированная зона из ADR-0010: продажа вещей/железа внутри комьюнити.
Барьер (верификация) стоит там, где есть деньги и риск, основной онбординг остаётся
лёгким. Анти-ВК: маркет — часть той же сети (профиль + общий раздел), не отдельное
приложение.

## Scope

Контекст `marketplace`. Шаг 1 (этот): доменное ядро + use-cases без БД. Шаг 2:
HTTP-срез (миграция, infra, api, экстрактор-гейт, интеграционные тесты). НЕ здесь:
платежи/эскроу, категории, поиск, фото, переговоры о цене, мультивалюта.

## Success criteria

Выставить товар может только верифицированный (casual → отказ); статус (продано/снято)
меняет только продавец и только из активного. Верификацию выдаёт только админ.
Анти-ВК: товары видны в профиле продавца и в общем разделе. Юнит- и интеграционные
тесты против БД зелёные.

## Technical

Модель — [[decisions/0014-marketplace-model]] (Accepted). Гейт — из
[[decisions/0010-verification-model]]; «домен решает, application читает» — по образцу
[[decisions/0011-invite-issuance-atomicity]] и [[decisions/0003-domain-as-source-of-truth]];
чтение — read-модели [[decisions/0004-cqrs-read-write-split]].

## Implementation

Ветка `feat/identity-invites`. `domain/marketplace`: `Listing` (+ `ListingTitle`/
`Price`/`ListingDescription`/`ListingStatus`/`ListingDraft`), `Listing::list` с гейтом
`VerifiedStatus`, `mark_sold`/`withdraw` (только продавец), порт `ListingRepository`,
`MarketplaceError`. `application`: `CreateListing`/`MarkListingSold`/`WithdrawListing`,
`VerifyUser` (админ-грант), read-модели `active`/`by_seller` (`ListingView`),
`ApplicationError` += `Marketplace`/`Forbidden`. `api`: маппинг (`NotVerified`/
`NotSeller`→403, `NotActive`→409, `Forbidden`→403). Замороженные агрегаты не тронуты.

## Log

- 2026-06-26: создано; шаг 1 — доменное ядро `marketplace` + use-cases (без БД).
  5 доменных и 4 прикладных юнит-теста зелёные. ADR-0014 — Proposed, ждёт акцепта.
- 2026-06-26: ADR-0014 акцептован (Accepted) пользователем.
- 2026-06-26: шаг 2 — HTTP-срез. Миграция `0006` (listings). infra: `PgListingRepository`
  (реконституция через домен), `PgListingReadModel` (active/by_seller). api: `POST /listings`,
  `/listings/{id}/sold|withdraw` (под `CurrentUser`), `GET /market`, `GET /profiles/{handle}/listings`
  (публичные), `POST /users/{handle}/verify` (админ). Тест `marketplace_it` против postgres
  зелёный: casual→403, верификация→продажа→виден в маркете/профиле, не-продавец sold→403,
  sold уходит из маркета, не-админ verify→403, без сессии→401. Верификация действует сразу
  на той же сессии (статус перечитывается). Bootstrap-root по-прежнему открыт (ADR-0013).
- 2026-06-26: web-UI (Leptos SSR). Экран `/market` в общей нав (анти-ВК): публичный браузер
  активных листингов (`GET /market`) + форма выставления (`POST /listings` под Bearer),
  ошибка 403 → понятное «нужна верификация». uikit += `Field`/`Badge`/`ListingCard`.
  Компилируется на ssr и wasm/hydrate. (bootstrap-root закрыт отдельно — см. фичу auth.)
- 2026-06-27: web-UI P1 — товары продавца в профиле (`GET /profiles/{handle}/listings`),
  кнопки «продано»/«снять» (`/listings/{id}/sold|withdraw`) видны только владельцу активного
  листинга; список перечитывается после действия. Анти-ВК: барахолка живёт в профиле, а не
  отдельным разделом.
