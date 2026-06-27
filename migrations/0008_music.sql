-- Музыка: треки (ADR-0017). Аудио на MVP — внешняя ссылка (`audio_url`), без хостинга
-- байтов. Статус — text + CHECK (домен — source of truth значений, ADR-0003).
-- Удаление юзера забирает его треки (CASCADE).
CREATE TABLE tracks (
    id          uuid        PRIMARY KEY,
    uploader_id uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    title       text        NOT NULL,
    audio_url   text        NOT NULL,
    genre       text,
    status      text        NOT NULL CHECK (status IN ('published', 'withdrawn')),
    created_at  timestamptz NOT NULL
);

-- Общий раздел музыки — свежие опубликованные треки (read-модель CQRS, ADR-0004).
CREATE INDEX tracks_published_recent ON tracks (created_at DESC) WHERE status = 'published';
-- Треки артиста на профиле (анти-ВК).
CREATE INDEX tracks_by_uploader ON tracks (uploader_id, created_at DESC);
