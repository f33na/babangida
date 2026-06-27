---
status: in-progress
owner: @fen
priority: P1
created: 2026-06-27
target: 2026-07-25
---

# Music — релизы треков за гейтом верификации

## Why

Музыка — смысловой центр underground hip-hop сети (Prompt 7). Соц-ядро и процесс
верификации готовы; пора дать артистам выкладывать треки. Барьер (верификация) стоит
там, где есть права (авторские) и риск, основной онбординг остаётся лёгким. Анти-ВК:
музыка — часть той же сети (профиль + общий раздел `/music`), не отдельный плеер.

## Scope

Полный вертикальный срез: контекст `music` (домен + application + infra + api) +
web-UI. Аудио на MVP — внешняя ссылка ([`AudioRef`] = URL), без хостинга байтов. НЕ
здесь: объектное хранилище/стриминг/загрузка байтов, альбомы/релизы, прослушивания/
лайки, обложки, попадание релиза в общую ленту постов, мобильный UI.

## Success criteria

Выложить трек может только верифицированный (casual → отказ); снять может только автор и
только из опубликованного. Анти-ВК: треки видны в профиле артиста и в общем разделе
`/music`; снятый уходит из публичной выдачи. Юнит- и интеграционные тесты против БД
зелёные; web компилируется на ssr и wasm/hydrate.

## Technical

Модель — [[decisions/0017-music-model]] (Proposed, ждёт акцепта). Гейт — из
[[decisions/0010-verification-model]] (процесс — [[decisions/0016-verification-process]]);
модель и анти-ВК по образцу [[decisions/0014-marketplace-model]]; «домен решает,
application читает» — [[decisions/0011-invite-issuance-atomicity]]/
[[decisions/0003-domain-as-source-of-truth]]; чтение — read-модели
[[decisions/0004-cqrs-read-write-split]].

## Implementation

Ветка `main`. `domain/music`: `Track` (+ `TrackTitle`/`AudioRef`/`Genre`/`TrackStatus`/
`TrackDraft`), `Track::release` с гейтом `VerifiedStatus`, `withdraw` (только автор),
порт `TrackRepository`, `MusicError`, честный `reconstitute`. `application`:
`ReleaseTrack`/`WithdrawTrack`, read-модели `recent`/`by_artist` (`TrackView`),
`ApplicationError` += `Music`. `infrastructure`: миграция `0008` (`tracks`),
`PgTrackRepository`, `PgMusicReadModel` (published-only). `api`: `POST /tracks`,
`POST /tracks/{id}/withdraw` (под `CurrentUser`), `GET /music`,
`GET /profiles/{handle}/tracks` (публичные); маппинг `NotVerified`/`NotUploader`→403,
`NotPublished`→409. web (Leptos): экран `/music` (раздел + форма релиза для verified),
треки в профиле артиста (снятие для автора), `uikit::TrackCard`, ссылка в нав.

## Log

- 2026-06-27: полный срез фичи. Домен `music` (5 юнит-тестов), application
  (`ReleaseTrack`/`WithdrawTrack` + read-модели, 4 юнит-теста с фейками), infra
  (миграция 0008 + Pg-адаптеры), api (4 эндпоинта + маппинг). Интеграционный
  `music_it` против postgres зелёный: casual→403, без сессии→401, верификация→релиз→
  виден в /music и профиле, кривой URL→422, не-автор snять→403, автор снимает→уходит из
  выдачи, повторное снятие→409. web: экран `/music` + треки в профиле + `TrackCard` +
  ссылка в нав; ssr и wasm/hydrate компилируются. ADR-0017 — Proposed, ждёт акцепта.
