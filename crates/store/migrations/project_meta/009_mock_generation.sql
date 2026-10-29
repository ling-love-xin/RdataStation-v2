-- 迁移版本：009
-- 描述：Mock 数据生成器持久化层
-- 日期：2026-05-09

CREATE TABLE IF NOT EXISTS mock_generation_tasks (
    id                TEXT PRIMARY KEY,
    table_name        TEXT NOT NULL,
    table_alias       TEXT,
    row_count         INTEGER NOT NULL,
    seed              INTEGER,
    locale            TEXT DEFAULT 'ZH_CN',
    scene_id          TEXT,
    save_format       TEXT,
    status            TEXT DEFAULT 'success',
    error_message     TEXT,
    generated_rows    INTEGER,
    generation_time_ms INTEGER,
    created_at        TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at        TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS mock_generation_columns (
    id                TEXT PRIMARY KEY,
    task_id           TEXT NOT NULL,
    column_name       TEXT NOT NULL,
    column_type       TEXT NOT NULL,
    generator         TEXT NOT NULL,
    generator_params  TEXT,
    null_ratio        REAL DEFAULT 0,
    is_unique         INTEGER DEFAULT 0,
    is_primary_key    INTEGER DEFAULT 0,
    is_foreign_key    INTEGER DEFAULT 0,
    ref_table         TEXT,
    ref_column        TEXT,
    comment           TEXT,
    confidence        TEXT,
    sort_order        INTEGER NOT NULL,
    FOREIGN KEY (task_id) REFERENCES mock_generation_tasks(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS mock_user_templates (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    description       TEXT,
    row_count         INTEGER NOT NULL DEFAULT 1000,
    seed              INTEGER,
    locale            TEXT DEFAULT 'ZH_CN',
    created_at        TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at        TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS mock_template_columns (
    id                TEXT PRIMARY KEY,
    template_id       TEXT NOT NULL,
    column_name       TEXT NOT NULL,
    column_type       TEXT NOT NULL,
    generator         TEXT NOT NULL,
    generator_params  TEXT,
    null_ratio        REAL DEFAULT 0,
    is_unique         INTEGER DEFAULT 0,
    is_primary_key    INTEGER DEFAULT 0,
    is_foreign_key    INTEGER DEFAULT 0,
    ref_table         TEXT,
    ref_column        TEXT,
    comment           TEXT,
    confidence        TEXT,
    sort_order        INTEGER NOT NULL,
    FOREIGN KEY (template_id) REFERENCES mock_user_templates(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_mock_tasks_time    ON mock_generation_tasks(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mock_columns_task  ON mock_generation_columns(task_id);
CREATE INDEX IF NOT EXISTS idx_mock_tpl_columns   ON mock_template_columns(template_id);