-- 迁移版本：007
-- 数据库类型：SQLite
-- 作用：为 project_info.path 添加索引，加速按路径查找项目
-- 更新时间：2026-05-12

CREATE INDEX IF NOT EXISTS idx_project_info_path ON project_info(path);