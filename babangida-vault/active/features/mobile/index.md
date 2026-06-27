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

Срез 1: общий `mobile-ui`, тонкие шеллы, лента + вход (сессия в памяти).
Срез 2: апгрейд до Dioxus 0.7, `dioxus-router`, экраны профиль/маркет/инбокс-тред/
сообщество, viewer-aware лента под токеном, светлая тема (CSS-переменные), секьюр-стор
токена (Keychain), конфиг упаковки под устройство. НЕ здесь (по-прежнему): реальная
сборка/подпись .ipa/.apk (нужен device-тулчейн + Apple/Google аккаунты — риск ADR-0007),
стор-ревью, пуши/камера, Android Keystore (заглушка), создание листинга/sold-withdraw на
mobile, персист темы между запусками.

## Success criteria

`mobile-ui` и шеллы компилируются и в UI-ядре (`cargo check`), и под фичей `shell`
(нативный рендерер на хосте). Лента тянет `GET /feed`, вход кладёт токен сессии в
память и переключает на ленту. clippy `-D` + fmt чисто. Реальный запуск на устройстве —
вне этого среза (риск ADR-0007).

## Technical

Архитектура — [[decisions/0015-mobile-client-architecture]] (Accepted): общий
`mobile-ui`, прямой HTTP через `reqwest` (без SSR), токен в секьюр-сторе. Реализует
[[decisions/0007-mobile-dioxus-native-shell]]; делит UI — [[decisions/0008-uikit-shared-crate]];
контракт API — как у web [[decisions/0006-web-leptos-ssr]]. Стек: Dioxus 0.7 +
`dioxus-router`, `keyring` (Keychain), CSS-переменные темы из `uikit::theme`.

### Упаковка под устройство

`dx` (Dioxus CLI) по `Dioxus.toml` в шелл-крейтах; бинарь-вход — `src/main.rs` под
фичей `shell`. iOS: `dx serve|build --platform ios` (нужен Xcode + подпись Apple).
Android: `dx serve|build --platform android` (нужен Android SDK/NDK + JDK). В CI/этом
окружении не собирается (нет тулчейна/аккаунтов) — риск ADR-0007. `Dioxus.toml` cargo
игнорирует, на `check/clippy/test` не влияет.

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
- 2026-06-27: срез 2. **Dioxus 0.6→0.7** (drop-in: те же фичи компилируются). Навигация
  на **`dioxus-router`** (`Routable`/`Router`/`Link`/`Outlet`): лента, `/login`, `/u/:handle`,
  `/market`, `/messages`, `/messages/:id?:with`, `/g/:slug` под единой `Shell`. Экраны
  профиль (карточка + товары + «написать»), маркет (товары + продавец-ссылка), инбокс +
  тред (ответ), сообщество (карточка + вступление + пост). **viewer-aware лента** — токен
  как Bearer в `fetch_feed`. **Светлая тема** — CSS-переменные из `uikit::theme::css_vars`,
  инжект в `<style>`, переключатель в нав; `uikit` Dioxus-компоненты переведены на
  `var(--…)`; += `Card`/`Badge`. **Секьюр-стор токена** — `keyring` (Keychain iOS/macOS;
  Android — заглушка под cfg, JNI-путь позже); токен персистится, на старте `/me`
  возобновляет сессию. **Упаковка**: `Dioxus.toml` + `src/main.rs` (бин под `shell`) в
  обоих шеллах. Проверка: UI-ядро + `shell` на хосте обе платформы; workspace clippy `-D`
  + fmt + тесты зелёные. Запуск на устройстве и `dx`-сборка не проверялись (нет тулчейна,
  риск ADR-0007).
