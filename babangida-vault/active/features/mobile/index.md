---
status: in-progress
owner: @fen
priority: P2
created: 2026-06-27
target: 2026-08-15
---

# Mobile — клиенты iOS/Android на Dioxus

## Why

Соцсети нужны мобильные клиенты под обе платформы. Соло-команда не может писать домен
и UI трижды — отсюда Dioxus и шеринг кода с web (ADR-0007, самая рискованная ставка).
PoC (Prompt 4) доказал шеринг `uikit`/`domain`. Дальше — реальные экраны с данными из
API. Анти-ВК: единая оболочка (общая лента/вход), не отдельные приложения по функциям.

## Scope

Срез 1 (этот): общий крейт `mobile-ui` (экраны + навигация + data-client), тонкие
шеллы `mobile-ios`/`mobile-android`, экраны лента (реальные данные) + вход (сессия в
памяти). НЕ здесь: упаковка под устройство (.ipa/.apk, `dx`+Xcode/Android SDK), стор-
ревью, пуши/камера, секьюр-стор токена, профиль/маркет/сообщения/сообщества на mobile,
светлая тема, dioxus-router.

## Success criteria

`mobile-ui` и шеллы компилируются и в UI-ядре (`cargo check`), и под фичей `shell`
(нативный рендерер на хосте). Лента тянет `GET /feed`, вход кладёт токен сессии в
память и переключает на ленту. clippy `-D` + fmt чисто. Реальный запуск на устройстве —
вне этого среза (риск ADR-0007).

## Technical

Архитектура — [[decisions/0015-mobile-client-architecture]] (Accepted): общий
`mobile-ui`, прямой HTTP через `reqwest` (без SSR), сигнальная навигация, токен в
памяти. Реализует [[decisions/0007-mobile-dioxus-native-shell]]; делит UI —
[[decisions/0008-uikit-shared-crate]]; контракт API — как у web
[[decisions/0006-web-leptos-ssr]].

## Implementation

Ветка `feat/identity-invites`. `uikit` (фича `dioxus`) += Dioxus-`Button`/`Field`.
`mobile-ui`: модуль `client` (`fetch_feed`/`login` на `reqwest`), `App` с сигнал-enum
навигацией (контекст), `Nav`, `FeedScreen` (`use_resource`), `LoginScreen` (форма →
токен в памяти → лента). `mobile-ios`/`mobile-android` сведены к `pub use App` +
`launch()` под `shell`. Домен/шелл-граница ADR-0007 держится (план Б стоит только UI).

## Log

- 2026-06-27: срез 1. ADR-0015 (Accepted). Общий `mobile-ui`: data-client (`reqwest`),
  экраны лента (`GET /feed`) + вход (сессия в памяти, Bearer), сигнальная навигация.
  Шеллы стали тонкими (делегируют в `mobile_ui::App`). `uikit` += Dioxus `Button`/`Field`.
  Компилируется в UI-ядре и под `shell` (хост) на обеих платформах; clippy `-D` + fmt;
  тесты воркспейса зелёные. Запуск на устройстве не проверялся (риск ADR-0007).
