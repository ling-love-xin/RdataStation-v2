-- 015: 表分区/继承父表信息（S3-② 分区子表折叠用）
-- PG 内省时经 pg_inherits 取得父表名与分区标记；MySQL/SQLite/DuckDB 无此概念，
-- 两列保持 NULL / 0。折叠逻辑在 navigator_service（子表不单独列出，父表徽标计数），
-- 因此 parent 必须随表落盘，L2 命中时才能还原折叠结果。

ALTER TABLE tables
    ADD COLUMN parent_table TEXT;

ALTER TABLE tables
    ADD COLUMN is_partition INTEGER DEFAULT 0;
