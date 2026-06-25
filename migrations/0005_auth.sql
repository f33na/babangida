-- Аутентификация: учётные данные и сессии (ADR-0013). Контекст auth отдельный от
-- identity: хранит «как доказать, что ты — это ты», не «кто ты».

-- Учётные данные: один хэш пароля на юзера. Хэш непрозрачен для БД (PHC-строка
-- argon2id, считается на границе). Удаление юзера забирает креды (CASCADE).
CREATE TABLE credentials (
    user_id        uuid        PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    password_hash  text        NOT NULL,
    established_at timestamptz NOT NULL
);

-- Сессии: непрозрачный токен у клиента, срок жизни. UNIQUE(token) — поиск и
-- распознавание по нему. На MVP токен хранится как есть (ADR-0013: риск дампа БД
-- зафиксирован, хэш токена при хранении — будущее ужесточение). Удаление юзера
-- забирает его сессии (CASCADE).
CREATE TABLE sessions (
    id         uuid        PRIMARY KEY,
    token      text        NOT NULL UNIQUE,
    user_id    uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    issued_at  timestamptz NOT NULL,
    expires_at timestamptz NOT NULL
);
