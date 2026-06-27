-- Открытое API: персональные API-ключи (ADR-0018). Храним только хэш секрета
-- (SHA-256), сырой токен показывается владельцу один раз при выпуске. Поиск при
-- аутентификации — по уникальному хэшу. Статус — text + CHECK (домен — source of
-- truth значений, ADR-0003). Удаление юзера забирает его ключи (CASCADE).
CREATE TABLE api_keys (
    id         uuid        PRIMARY KEY,
    owner_id   uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    label      text        NOT NULL,
    key_hash   text        NOT NULL UNIQUE,
    status     text        NOT NULL CHECK (status IN ('active', 'revoked')),
    created_at timestamptz NOT NULL
);

-- Ключи владельца на экране управления (read-модель CQRS, ADR-0004).
CREATE INDEX api_keys_by_owner ON api_keys (owner_id, created_at DESC);
