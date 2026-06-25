-- Срез messaging + community (Prompt 5). Перечисления — text + CHECK: домен
-- source of truth для значений (ADR-0003), БД проверяет принадлежность набору.

-- Личная переписка (DM). Пара участников канонизируется в домене (меньший UUID
-- первым) и хранится как (user_lo, user_hi); UNIQUE делает (a,b) и (b,a) одним
-- диалогом. CHECK lo<>hi и lo<hi — зеркало доменного инварианта (не само-диалог,
-- канонический порядок).
CREATE TABLE conversations (
    id         uuid        PRIMARY KEY,
    user_lo    uuid        NOT NULL REFERENCES users (id),
    user_hi    uuid        NOT NULL REFERENCES users (id),
    opened_at  timestamptz NOT NULL,
    CONSTRAINT conversation_pair_canonical CHECK (user_lo < user_hi),
    UNIQUE (user_lo, user_hi)
);

CREATE TABLE messages (
    id              uuid        PRIMARY KEY,
    conversation_id uuid        NOT NULL REFERENCES conversations (id),
    author_id       uuid        NOT NULL REFERENCES users (id),
    body            text        NOT NULL,
    sent_at         timestamptz NOT NULL
);

-- Тред диалога по времени (read-модель CQRS, ADR-0004).
CREATE INDEX messages_by_conversation ON messages (conversation_id, sent_at, id);

-- Сообщества: закрытые группы и паблики (ADR-0012).
CREATE TABLE groups (
    id         uuid        PRIMARY KEY,
    slug       text        NOT NULL UNIQUE,
    name       text        NOT NULL,
    kind       text        NOT NULL CHECK (kind IN ('closed', 'public')),
    created_at timestamptz NOT NULL
);

-- Членство и роли. PK (group_id, user_id) = один член один раз. Удаление группы
-- забирает членства (CASCADE), удаление юзера запрещено, пока он где-то состоит.
CREATE TABLE group_members (
    group_id uuid NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    user_id  uuid NOT NULL REFERENCES users (id),
    role     text NOT NULL CHECK (role IN ('owner', 'moderator', 'member')),
    PRIMARY KEY (group_id, user_id)
);

-- Состав сообщества (карточка/счётчик участников).
CREATE INDEX group_members_by_group ON group_members (group_id);
