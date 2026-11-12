-- 027_analytics_resource_timeline.sql
-- 版本时间线：把「发生过什么」持久化到项目库。
--
-- 与运行时广播 ResourcesChanged 的区别：广播只驱动当次界面刷新，进程一结束就没了；
-- 业务误解往往发生在几个月后，那时能依赖的只有库里的记录。事件表是资产履历的唯一真相源。
--
-- 刻意不加外键：撤销归档会硬删资源行，但履历必须留下（"这条结论曾经存在过、后来被撤销"
-- 本身就是追溯要回答的问题）。清理只走本模块的永久删除路径，不做级联。
CREATE TABLE IF NOT EXISTS analytics_resource_events (
    id           TEXT PRIMARY KEY,
    resource_id  TEXT NOT NULL,
    version      INTEGER,
    event_type   TEXT NOT NULL,
    detail       TEXT,
    content_hash TEXT,
    occurred_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_are_resource_time
    ON analytics_resource_events (resource_id, occurred_at DESC, id DESC);
