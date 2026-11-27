-- 044: 表快照 GC 候选时刻（全局库）
--
-- 与项目库 039 同构：先记候选时刻，超过 grace 且仍无版本行引用才
-- DROP 物理表；任何不确定都保留。

ALTER TABLE table_snapshots ADD COLUMN gc_candidate_at TEXT;
