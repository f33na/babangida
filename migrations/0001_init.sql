-- Первый срез identity-invites/social/content (Prompt 2).
-- Типы-перечисления храним как text + CHECK: домен — source of truth для значений
-- (ADR-0003), БД лишь проверяет принадлежность набору.

CREATE TABLE users (
    id          uuid        PRIMARY KEY,
    handle      text        NOT NULL UNIQUE,
    role        text        NOT NULL CHECK (role IN ('member', 'admin')),
    verified    text        NOT NULL CHECK (verified IN ('casual', 'verified')),
    created_at  timestamptz NOT NULL
);

CREATE TABLE invites (
    id           uuid        PRIMARY KEY,
    code         text        NOT NULL UNIQUE,
    inviter_id   uuid        NOT NULL REFERENCES users (id),
    status       text        NOT NULL CHECK (status IN ('active', 'accepted')),
    accepted_by  uuid        REFERENCES users (id),
    accepted_at  timestamptz,
    created_at   timestamptz NOT NULL,
    -- инвариант целостности статуса: accepted ⇔ есть кто и когда принял
    CONSTRAINT invite_accepted_consistency CHECK (
        (status = 'active'   AND accepted_by IS NULL AND accepted_at IS NULL) OR
        (status = 'accepted' AND accepted_by IS NOT NULL AND accepted_at IS NOT NULL)
    )
);

-- Для подсчёта активных инвайтов инвайтера (квота, ADR-0005).
CREATE INDEX invites_active_by_inviter ON invites (inviter_id) WHERE status = 'active';
-- Для времени последней выдачи (кулдаун).
CREATE INDEX invites_by_inviter_created ON invites (inviter_id, created_at DESC);

CREATE TABLE profiles (
    user_id      uuid PRIMARY KEY REFERENCES users (id),
    display_name text NOT NULL,
    subculture   text NOT NULL CHECK (subculture IN ('underground', 'casual', 'skin', 'hiphop')),
    bio          text
);

CREATE TABLE posts (
    id         uuid        PRIMARY KEY,
    author_id  uuid        NOT NULL REFERENCES users (id),
    body       text        NOT NULL,
    created_at timestamptz NOT NULL
);

-- Лента: свежие посты (read-модель CQRS, ADR-0004).
CREATE INDEX posts_recent ON posts (created_at DESC, id DESC);
