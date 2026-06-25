# AGENTS.md — `uikit`

Перед работой прочитай `../babangida-vault/COMMON.md`. Релевантные ADR: 0008, 0006, 0007.
Финальные дизайн-токены фиксируются в `../babangida-vault/stable/design-system.md`.

## Роль

Дизайн-система: токены (цвета, spacing, типографика) и UI-компоненты, общие для
web (Leptos) и mobile (Dioxus).

## Границы

- Не зависит на `domain`/`application`/`infrastructure`/`api`. Только UI-зависимости.
- Реально шарятся токены и логика, не цельный рендер — модели Leptos/Dioxus разные.
