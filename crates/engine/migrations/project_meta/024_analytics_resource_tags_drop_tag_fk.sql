-- 标签定义上移全局库（规格 docs/architecture/tag/tag-system-spec.md §7.2 执行路径第 3 步）：
-- analytics_resource_tags.tag_id 原外键指向项目库 analytics_tags(id)；定义迁到全局库后
-- 跨库无法用外键约束（§3.4 决策 D1：应用层解析），重建表去掉该外键，资源侧外键保留。
CREATE TABLE analytics_resource_tags_v024 (
    resource_id TEXT NOT NULL,
    tag_id      TEXT NOT NULL,
    PRIMARY KEY (resource_id, tag_id),
    FOREIGN KEY (resource_id) REFERENCES analytics_resources(id)
);

INSERT INTO analytics_resource_tags_v024 (resource_id, tag_id)
    SELECT resource_id, tag_id FROM analytics_resource_tags;

DROP TABLE analytics_resource_tags;

ALTER TABLE analytics_resource_tags_v024 RENAME TO analytics_resource_tags;

CREATE INDEX IF NOT EXISTS idx_art_resource ON analytics_resource_tags(resource_id);
CREATE INDEX IF NOT EXISTS idx_art_tag ON analytics_resource_tags(tag_id);
