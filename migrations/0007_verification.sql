-- Верификация: процесс получения статуса (ADR-0016). Сам статус — на users.verified
-- (контекст identity, ADR-0010); здесь живёт только процесс: заявка → рассмотрение
-- админом → решение. Статус заявки — text + CHECK (домен — source of truth значений,
-- ADR-0003). Удаление юзера забирает его заявки (CASCADE); удаление админа-решателя
-- лишь обнуляет ссылку (SET NULL) — история решения остаётся.
CREATE TABLE verification_requests (
    id              uuid        PRIMARY KEY,
    requester_id    uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    status          text        NOT NULL CHECK (status IN ('pending', 'approved', 'rejected')),
    note            text,
    decided_by      uuid        REFERENCES users (id) ON DELETE SET NULL,
    decision_reason text,
    created_at      timestamptz NOT NULL,
    decided_at      timestamptz
);

-- Одна открытая заявка на юзера: партиал-уникальный индекс добивает гонку подачи
-- (read-проверка в application оптимистична, ADR-0016).
CREATE UNIQUE INDEX verification_one_pending_per_user
    ON verification_requests (requester_id) WHERE status = 'pending';

-- Очередь админа — ожидающие рассмотрения, старые сверху (FIFO, read-модель CQRS).
CREATE INDEX verification_pending_queue
    ON verification_requests (created_at) WHERE status = 'pending';

-- Статус заявки в UI юзера — последняя по времени.
CREATE INDEX verification_by_requester
    ON verification_requests (requester_id, created_at DESC);
