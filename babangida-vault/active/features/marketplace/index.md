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

Модель — [[decisions/0014-marketplace-model]] (**Proposed**). Гейт — из
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
