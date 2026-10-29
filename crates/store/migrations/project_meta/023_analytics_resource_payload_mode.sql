-- 023_analytics_resource_payload_mode.sql
--
-- M6 资产库 / 分析存档：File 档归档粒度两档（copy / reference）。
-- 设计依据：docs/architecture/analytics_resource/analytics-resource-architecture.md §2.2
--           + 2026-10-13 语义裁决（File 档 copy/reference 两档粒度，默认 copy）。
--
-- 原则：
--   1. **只增列**（老库直接升级）；
--   2. 旧行一律默认 payload_mode = 'copy'（历史行为 = move 进 resources/，语义归并到 copy）；
--   3. reference 模式的"源地址"单独进 source_path 列，不占用 file_rel_path 的
--      "resources/ 下相对路径"语义（resolve 守卫不因此放宽）。

-- 归档粒度：copy（默认）= 本体复制进 resources/ + 只读、草稿保留为工作副本；
--          reference = 只存元数据（地址 + 文件名 + 指纹 + 版本记录）、本体留原位、打开校验指纹。
ALTER TABLE analytics_resources
    ADD COLUMN payload_mode TEXT NOT NULL DEFAULT 'copy'
    CHECK (payload_mode IN ('copy', 'reference'));

-- reference 模式的源**绝对**路径（copy 模式为 NULL）。
-- 打开 / 取回 / 历史副本 / 指纹校验都从它解析本体；源被删 / 改 → "源已变更，快照失效"。
ALTER TABLE analytics_resources ADD COLUMN source_path TEXT;
