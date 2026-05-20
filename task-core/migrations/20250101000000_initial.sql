CREATE TABLE IF NOT EXISTS tasks (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    assignee    TEXT NOT NULL DEFAULT '',
    start_time  TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'pending'
                CHECK(status IN ('pending','in_progress','completed','cancelled'))
);
