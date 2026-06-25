-- Маркетплейс: товары (ADR-0014). Статус — text + CHECK (домен — source of truth для
-- значений, ADR-0003). Цена — целые рубли (MVP), CHECK > 0 зеркалит инвариант домена.
-- Удаление юзера забирает его товары (CASCADE).
CREATE TABLE listings (
    id          uuid        PRIMARY KEY,
    seller_id   uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    title       text        NOT NULL,
    price       bigint      NOT NULL CHECK (price > 0),
    description text,
    status      text        NOT NULL CHECK (status IN ('active', 'sold', 'withdrawn')),
    created_at  timestamptz NOT NULL
);

-- Общий раздел маркета — свежие активные товары (read-модель CQRS, ADR-0004).
CREATE INDEX listings_active_recent ON listings (created_at DESC) WHERE status = 'active';
-- Товары продавца на профиле (анти-ВК).
CREATE INDEX listings_by_seller ON listings (seller_id, created_at DESC);
