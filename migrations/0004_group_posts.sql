-- Связь поста с сообществом (анти-ВК: контент сообществ течёт в общую ленту,
-- ADR-0012). Пост остаётся обычным content.Post — его агрегат не меняется
-- (ADR-0003); принадлежность сообществу хранится отдельной связью один-к-одному.
CREATE TABLE group_posts (
    post_id  uuid PRIMARY KEY REFERENCES posts (id) ON DELETE CASCADE,
    group_id uuid NOT NULL REFERENCES groups (id) ON DELETE CASCADE
);

-- Лента сообщества и фильтр общей ленты по типу группы.
CREATE INDEX group_posts_by_group ON group_posts (group_id);
