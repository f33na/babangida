# babangida-vault — карта

Контекст и решения проекта babangida (underground RU hip-hop соцсеть по инвайтам)
в едином markdown-репо. Что не записано здесь или в коде — для нового агента не
существует. Фреймворк: `../docs/SHUBINFRAMEWORK.md`. Старт сессии: `COMMON.md`.

## Где что лежит

- `COMMON.md` — операционный чек-лист на сессию.
- `stable/stack.md` — стек и версии. `stable/glossary.md` — ubiquitous language.
- `active/decisions/` — ADR (ниже). `active/features/` — страницы фич. `active/patterns/` — rust-паттерны и gotchas.
- `log/` — append-only: фидбэк, инциденты, ретро.

## ADR (сквозная нумерация, immutable после Accepted)

- [[decisions/0001-stack]] — технологический стек.
- [[decisions/0002-monorepo-cargo-workspace]] — монорепо на cargo workspace.
- [[decisions/0003-domain-as-source-of-truth]] — `domain` как source of truth.
- [[decisions/0004-cqrs-read-write-split]] — CQRS: разделение чтения и записи.
- [[decisions/0005-invite-model]] — инвайт-модель (2 активных, ~12ч, админ без лимита).
- [[decisions/0006-web-leptos-ssr]] — web на Leptos SSR.
- [[decisions/0007-mobile-dioxus-native-shell]] — mobile на Dioxus (главный риск).
- [[decisions/0008-uikit-shared-crate]] — общий `uikit`.
- [[decisions/0009-nix-flake-ci-db]] — Nix flake: окружение, БД, CI.
- [[decisions/0010-verification-model]] — верификация (гейт маркета/музыки/open API).
- [[decisions/0011-invite-issuance-atomicity]] — атомарность выдачи инвайта (tx + блокировка).
- [[decisions/0012-messaging-community-model]] — messaging (DM) и сообщества (группы/паблики). **Proposed**.
