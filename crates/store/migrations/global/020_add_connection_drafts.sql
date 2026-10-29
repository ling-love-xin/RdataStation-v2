-- 020: 连接对话框暂存列表（多连接连续编辑）
--
-- M3 连接模块 C5（原型设计 §2.2）：数据源对话框内可一次暂存 / 连续编辑多条连接，
-- 关闭应用后仍可继续。按列表顺序保存条目快照：
--   - saved_id 非空表示该条目对应一条已保存连接（点击走编辑回读）；
--   - 其余列为表单快照；**不含密码**（凭据不落此表，只随正式保存写入连接库）。
--
-- 幂等：IF NOT EXISTS；迁移按版本号只执行一次。

CREATE TABLE IF NOT EXISTS connection_drafts (
    position            INTEGER PRIMARY KEY,
    name                TEXT NOT NULL DEFAULT '',
    saved_id            TEXT,
    driver_name         TEXT NOT NULL DEFAULT '',
    url                 TEXT NOT NULL DEFAULT '',
    username            TEXT NOT NULL DEFAULT '',
    remark              TEXT NOT NULL DEFAULT '',
    scope               TEXT NOT NULL DEFAULT '',
    project_path        TEXT NOT NULL DEFAULT '',
    ssl_mode            TEXT NOT NULL DEFAULT '',
    ssl_ca              TEXT NOT NULL DEFAULT '',
    ssl_cert            TEXT NOT NULL DEFAULT '',
    ssl_key             TEXT NOT NULL DEFAULT '',
    cache_path          TEXT NOT NULL DEFAULT '',
    duckdb_fed          INTEGER NOT NULL DEFAULT 1,
    active_tab          INTEGER NOT NULL DEFAULT 0,
    hops_json           TEXT NOT NULL DEFAULT '[]',
    props_json          TEXT NOT NULL DEFAULT '[]',
    sec_overrides_json  TEXT NOT NULL DEFAULT '[]',
    auth_ref            TEXT,
    network_ref         TEXT,
    env                 TEXT,
    updated_at          TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
