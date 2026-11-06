-- 插件登记按 code 去重（数据修复 + 唯一约束）
--
-- 背景：plugins 表主键是安装实例 uuid，另有 UNIQUE(code, version)；同 code 换版本升级时
-- 两条约束都不冲突，旧版本行会留下来（同样指向 plugins/<id>），于是 get_all_plugins
-- 出现重复登记、卸载后留幽灵行。运行时已改为「按 code 合并」（plugin_store::register_plugin），
-- 这里把历史库里已经存在的重复行收掉，并加唯一索引防止再有第二条路径写进重复行。
--
-- 保留规则：同 code 保留 updated_at 最新的一行（时间相同按 rowid 取最后写入的那行）。
DELETE FROM plugins
WHERE rowid IN (
    SELECT rowid
    FROM (
        SELECT rowid,
               ROW_NUMBER() OVER (
                   PARTITION BY code
                   ORDER BY updated_at DESC, rowid DESC
               ) AS rn
        FROM plugins
    )
    WHERE rn > 1
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_plugins_code ON plugins(code);
