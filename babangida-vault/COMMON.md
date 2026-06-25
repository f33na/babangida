# Babangida Vault — операционная схема

Короткий чек-лист на каждую сессию. Читаешь перед кодом. Понять сам фреймворк —
`../docs/SHUBINFRAMEWORK.md`; правила конкретного крейта — его `AGENTS.md`.
При расхождении прав этот файл (он ближе к коду).

## Структура

- `stable/` — редко меняется, source of truth для «почему» (`stack.md`, `glossary.md`; `architecture`/`design-system`/`api-style` добавляются по мере появления).
- `active/` — `features/` (по bounded context), `decisions/` (ADR), `patterns/`.
- `log/` — append-only: `feedback/`, `incidents/`, `retrospectives/`. Не редактировать.

## Source of truth

- ТИПЫ / МОДЕЛИ / ИНВАРИАНТЫ → крейт `domain` (код). Vault их НЕ дублирует.
- РЕШЕНИЯ / КОНТЕКСТ → vault (ADR + `glossary` + `patterns`).

## Перед кодом

1. Найди фичу в `active/features/<slug>/index.md`.
2. Проверь `active/decisions/` на релевантные ADR.
3. Проверь `active/patterns/` на похожие паттерны и `gotchas.md`.

## Слои (clean) — направление зависимостей только внутрь

```
domain ⟵ application ⟵ infrastructure / api / web / mobile
```

- `domain` зависит только от `shared`; не зависит от axum/sqlx/leptos/dioxus.
- `application` → `domain`, `shared`. `infrastructure`/`api` → `application`, `domain`, `shared`.
- `uikit` самостоятелен. `web` → `application`/`uikit`. `mobile-*` → `domain`/`uikit`.
- Нарушить направление — серьёзная ошибка, даже если «так быстрее».

## На значимое решение / gotcha

Предложи ADR (формат `SHUBINFRAMEWORK.md` §5) или строку в `patterns`. Молча файлы
не создавай — сначала скажи, что нашёл.

## Чего не делать

- Не редактировать `log/` (append-only).
- Не плодить пустые папки на будущее.
- Не нарушать направление зависимостей слоёв.
- Не дублировать в хендлерах доменные правила (квота/кулдаун инвайта — из `domain`).
