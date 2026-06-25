# Паттерн: error-handling по слоям

Каждый слой — свой тип ошибки; наружу маппится через `#[from]`/явный маппинг.
Детали нижнего слоя выше по стеку не текут. Всё на `thiserror`.

## Слои

- **Value objects** (`domain`): `HandleError`, `InviteCodeError`, … — нарушение инварианта при парсинге.
- **Агрегаты** (`domain`): `InviteError` (квота/кулдаун/статус) — нарушение доменного правила.
- **Порты** (`domain`): `RepositoryError { NotFound, Conflict, Unavailable }` — абстрактный сбой хранилища; адаптер мапит сюда ошибки sqlx (детали драйвера не наружу).
- **Use-cases** (`application`): `ApplicationError` оборачивает `InviteError` и `RepositoryError` через `#[from]`, плюс `NotFound(&'static str)`.
- **HTTP** (`api`): маппинг `ApplicationError` → статус. Ориентир: `QuotaExceeded`/`CooldownActive`/VO-ошибки → 4xx (422/409), `NotFound` → 404, `Unavailable` → 503. Тело — без утечки внутренних деталей.

## Принцип

Доменные правила (квота/кулдаун) проверяет домен и возвращает типизированную ошибку;
`api` её только переводит в HTTP-код, не перепроверяет (ADR-0003/0005).
