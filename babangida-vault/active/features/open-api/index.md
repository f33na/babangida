---
status: in-progress
owner: @fen
priority: P1
created: 2026-06-27
target: 2026-07-25
---

# Open API — программный доступ под персональными ключами

## Why

Третья и последняя привилегированная зона из ADR-0010 (после маркета и музыки):
программный доступ к сети для интеграций и power-users. Барьер (верификация) стоит там,
где риск злоупотребления; основной онбординг остаётся лёгким. Анти-ВК: открытое API —
доступ к той же сети (те же read-модели и use-cases), не отдельный продукт.

## Scope

Полный вертикальный срез: контекст `openapi` (домен + application + infra + api) +
web-UI управления ключами. Аутентификация — персональные ключи; поверхность `/api/v1` —
чтение + запись от имени владельца. НЕ здесь: OAuth/сторонние приложения, гранулярные
scope'ы, rate-limiting/квоты, `last_used`/аудит, мобильный UI.

## Success criteria

Выпустить ключ может только верифицированный (casual → отказ); отозвать — только
владелец. Ключ аутентифицирует `/api/v1` (нет/неизвестный/отозванный → 401). Чтение
(лента/профиль/музыка/маркет) и запись (пост/трек) идут от имени владельца. Секрет
хранится хэшем, показывается один раз. Юнит- и интеграционные тесты против БД зелёные;
web компилируется на ssr и wasm/hydrate.

## Technical

Модель — [[decisions/0018-open-api-model]] (Proposed, ждёт акцепта). Гейт — из
[[decisions/0010-verification-model]] (процесс — [[decisions/0016-verification-process]]);
секрет/токен по образцу [[decisions/0013-authentication-sessions]]; модель и анти-ВК по
образцу [[decisions/0014-marketplace-model]]/[[decisions/0017-music-model]]; чтение —
read-модели [[decisions/0004-cqrs-read-write-split]].

## Implementation

Ветка `main`. `domain/openapi`: `ApiKey` (+ `ApiKeyToken`/`ApiKeyHash`/`ApiKeyLabel`/
`ApiKeyStatus`), `ApiKey::issue` с гейтом `VerifiedStatus`, `revoke` (владелец), порт
`ApiKeyRepository`, `OpenApiError`, честный `reconstitute`. `application`: порты
`ApiKeyFactory`/`ApiKeyHasher`, use-cases `IssueApiKey`/`RevokeApiKey`/`AuthenticateApiKey`,
read-модель `ApiKeysOf`, `ApplicationError` += `OpenApi`. `infrastructure`: миграция
`0009` (`api_keys`), `PgApiKeyRepository`, `PgApiKeyReadModel`, `RandomApiKeyFactory`,
`Sha256ApiKeyHasher` (зависимость `sha2`). `api`: управление под сессией
(`POST/GET /api-keys`, `POST /api-keys/{id}/revoke`), экстрактор `ApiCaller`, `/api/v1`
чтение (`me`/`feed`/`profiles/{handle}`/`music`/`market`) + запись (`posts`/`tracks`).
web (Leptos): экран `/api-keys` (выпуск для verified с показом секрета один раз, список
+ отзыв), ссылка в нав.

## Log

- 2026-06-27: полный срез фичи. Домен `openapi` (6 юнит-тестов), application
  (`IssueApiKey`/`RevokeApiKey`/`AuthenticateApiKey` + read-модель, 4 юнит-теста с
  фейками), infra (миграция 0009 + Pg-адаптеры + CSPRNG-фабрика + SHA-256-хэшер), api
  (управление + экстрактор `ApiCaller` + `/api/v1` чтение и запись). Интеграционный
  `openapi_it` против postgres зелёный: casual не выпускает→403, ключ показывается раз,
  виден в списке без секрета, аутентифицирует `/api/v1/me`, нет/неизвестный→401, пост и
  трек через API, чужой не отзывает→403, владелец отзывает→ключ перестаёт работать→401,
  повторный отзыв→409. web: экран `/api-keys` + ссылка в нав; ssr и wasm/hydrate
  компилируются. ADR-0018 — Proposed, ждёт акцепта. **Этим закрыт роадмап (Prompt 0-8).**
