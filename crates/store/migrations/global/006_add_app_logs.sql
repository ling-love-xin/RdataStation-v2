CREATE TABLE IF NOT EXISTS app_logs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp   TEXT NOT NULL,
    level       TEXT NOT NULL CHECK(level IN ('TRACE', 'DEBUG', 'INFO', 'WARN', 'ERROR')),
    target      TEXT NOT NULL,
    message     TEXT NOT NULL,
    fields      TEXT,
    file        TEXT,
    line        INTEGER,
    session_id  TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_logs_timestamp ON app_logs(timestamp);
CREATE INDEX IF NOT EXISTS idx_logs_level ON app_logs(level);
CREATE INDEX IF NOT EXISTS idx_logs_target ON app_logs(target);
CREATE INDEX IF NOT EXISTS idx_logs_session ON app_logs(session_id);