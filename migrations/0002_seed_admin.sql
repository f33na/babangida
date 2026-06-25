-- Bootstrap-инвайтер: без существующего юзера некому выдать первый инвайт
-- (инвайт-онли, ADR-0005). Сид-админ `root` для dev/первого запуска; в реальном
-- проде заводится отдельно. Идемпотентно.
INSERT INTO users (id, handle, role, verified, created_at)
VALUES (
    '00000000-0000-0000-0000-000000000001',
    'root',
    'admin',
    'verified',
    '2026-06-25T00:00:00Z'
)
ON CONFLICT (id) DO NOTHING;
