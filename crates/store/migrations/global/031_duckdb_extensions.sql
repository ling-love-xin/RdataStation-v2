-- 031 DuckDB 扩展的登记与留痕（2026-11-14）
--
-- 与 `plugins` 表刻意分开：扩展是「进程内 dlopen 的单个文件」，没有插件目录、
-- 没有清单、没有贡献点，也不需要 checksums.json（宿主自己校验）。
-- 两者只共享「全局装、项目引用」的心智（plugin-architecture.md §7），物理不混。
--
-- `duckdb_extensions`（终态）：
--   extension_name  扩展名（如 excel / oracle_scanner）
--   kernel_version  DuckDB 内核版本（如 v1.1.5）；升级后是**另一行**，不覆盖旧的
--   platform        windows_amd64 / linux_amd64 / osx_arm64（DuckDB 自己的平台串，
--                   不是 rustc 的 x86_64-pc-windows-msvc；由 install_path 解析而来）
--   source          official | community | local_file
--   file_path       `duckdb_extensions()` 给出的权威 install_path（不自己拼 v<x>/<platform>）
--   sha256          慢指纹：只在安装后 / 指纹变化 / 用户点校验时算（不在每次建连算）
--   size_bytes      与 sha256 一起用于「为什么这条记录会失效」的排障
--   state           installed | mismatch | missing
--   last_error      最后一次失败的可读原因（失败也要留痕，别只回 UI 状态行）
--   主键 (extension_name, kernel_version, platform)：磁盘上这三个才算一个文件
--
-- `extension_operations`（历史）：
--   与 `driver_operations` 分开的理由——那边的列是 plugin_id / driver_id，
--   扩展两者都没有；before/after 的语义也不同（那边记驱动状态差异，这边记文件指纹）。
--   action：install / load / verify / import / remove / missing
--   actor：  ui | runtime | migration | cli
CREATE TABLE IF NOT EXISTS duckdb_extensions (
    extension_name TEXT NOT NULL,
    kernel_version TEXT NOT NULL,
    platform       TEXT NOT NULL,
    source         TEXT NOT NULL,
    file_path      TEXT,
    sha256         TEXT,
    size_bytes     INTEGER,
    state          TEXT NOT NULL DEFAULT 'installed',
    last_error     TEXT,
    installed_at   TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    verified_at    TIMESTAMP,
    PRIMARY KEY (extension_name, kernel_version, platform)
);

CREATE TABLE IF NOT EXISTS extension_operations (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    scope          TEXT NOT NULL,
    actor          TEXT NOT NULL,
    action         TEXT NOT NULL,
    extension_name TEXT,
    kernel_version TEXT,
    platform       TEXT,
    project_id     TEXT,
    result         TEXT NOT NULL,
    reason         TEXT,
    before_json    TEXT,
    after_json     TEXT,
    created_at     TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_ext_ops_time ON extension_operations(occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_ext_ops_extension ON extension_operations(extension_name);
CREATE INDEX IF NOT EXISTS idx_ext_ops_action ON extension_operations(action);
