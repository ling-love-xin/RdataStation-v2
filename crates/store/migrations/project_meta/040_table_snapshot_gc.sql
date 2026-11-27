-- 039: 表快照 GC 候选时刻（项目库）
--
-- 裁剪 / 永久删除只清版本行引用，物理快照由后台 GC 回收。GC 第一次发现
-- 某份快照已无版本行引用时写 `gc_candidate_at`；只有候选时间超过 grace
-- 才真正 DROP，给崩溃恢复和并发读留出窗口。

ALTER TABLE table_snapshots ADD COLUMN gc_candidate_at TEXT;
