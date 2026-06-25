---
status: in-progress
owner: @fen
priority: P0
created: 2026-06-26
target: 2026-07-10
---

# Authentication — аутентификация и сессии

## Why

До сих пор «текущий пользователь» проброшен в API параметрами (`author`/`viewer`) —
писать можно от чужого имени. Маркет (Prompt 6) с гейтом верификации (ADR-0010) и лента
закрытых групп требуют достоверно знать, кто запрашивает и верифицирован ли он. Нужен
слой аутентификации: как юзер доказывает себя и как это переживает запросы.

## Scope

Контекст `auth` (пароль + серверные сессии). Шаг 1 (этот): доменное ядро + use-cases без
БД. Шаг 2: HTTP-срез — миграция, argon2id-хэшер и CSPRNG-фабрика токенов, Pg-репозитории,
эндпоинты login/logout, экстрактор текущего юзера (кука/Bearer), пароль в регистрации
(атомарно), интеграционные тесты. НЕ здесь: сброс/смена пароля, 2FA, magic-link,
rate-limit логина, скользящий срок сессии, хэш токена при хранении.

## Success criteria

Логин по верному паролю выпускает сессию; неверный пароль и неизвестный handle дают одну
ошибку (анти-энумерация). По токену распознаётся текущий юзер с `verified`. Истёкшая,
отсутствующая или погашенная (logout) сессия — `Unauthenticated`. Пароль никогда не
сохраняется и не логируется. Домен auth чист (без I/O), юнит- и интеграционные тесты
зелёные.

## Technical

Модель — [[decisions/0013-authentication-sessions]] (Accepted). Источник правды и
«хэш/токен на границе» — [[decisions/0003-domain-as-source-of-truth]]; гейт привилегий,
который это разблокирует — [[decisions/0010-verification-model]]; атомарность кредов при
регистрации — по образцу [[decisions/0011-invite-issuance-atomicity]].

## Implementation

Ветка `feat/identity-invites`. `domain/auth`: VO `Password`/`PasswordHash`/`SessionToken`,
агрегаты `Credential`/`Session` (`SESSION_TTL`, `is_active`), порты
`CredentialRepository`/`SessionRepository`, `AuthError`. `application`: порты
`PasswordHasher`/`SessionTokenFactory`, use-cases `EstablishCredential`/`LogIn`/`LogOut`/
`Authenticate`, `ApplicationError::Auth`. `api`: маппинг `Auth → 401`. Замороженный
`identity::User` не тронут — auth отдельным контекстом.

## Log

- 2026-06-26: создано; шаг 1 — доменное ядро `auth` + use-cases (без БД). 7 доменных и
  7 прикладных юнит-тестов зелёные. ADR-0013 — Proposed, ждёт акцепта на ревью.
- 2026-06-26: шаг 2a — HTTP-срез (часть 1). Миграция `0005` (credentials, sessions);
  infra: argon2id-хэшер, CSPRNG-фабрика токенов, Pg-репозитории кредов/сессий; пароль
  в регистрации (атомарно в tx регистрации); эндпоинты `POST /login` (set-cookie +
  токен), `POST /logout`, `GET /me`; экстрактор `CurrentUser` (Bearer/кука → `Authenticate`).
  Write-хендлеры пока на параметрах (раскатка `CurrentUser` — 2b). Тест `auth_it` против
  postgres зелёный (register+пароль → login → /me → logout → 401). Найден пробол: root
  без кредов (см. ADR-0013 Consequences). ADR-0013 всё ещё Proposed.
- 2026-06-26: ADR-0013 акцептован (Accepted) пользователем.
