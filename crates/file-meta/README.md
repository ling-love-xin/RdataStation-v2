# rds-file-meta

文件元信息管理系统：记录每个文件**快照式**的元信息（sidecar 模式）。

**设计文档在 `docs/architecture/file-meta/`，本文件只作入口指针**（项目约定：crate 内不复制设计）。

| 想了解 | 去哪 |
| --- | --- |
| 为什么这样设计（需求 / 快照语义 / sidecar 位置策略 / `extra` 扩展 / 数据结构 / API / 消费方 / 实现状态） | `docs/architecture/file-meta/file-meta-design.md` |
| 从哪开始说 / 依赖方向 / 当前状态 / 验证 | `docs/architecture/file-meta/README.md` |

## 依赖方向（硬约束）

file-meta 是**基础元数据 crate**，不依赖任何应用模块：

```
editor / scratchpad / … → file-meta
```

## 当前状态（2026-10-28）

- ✅ P0 全部实现：元信息读写、显式路径读写（`read_meta_at` / `write_meta_at`）、书签管理、默认值、清理函数、`schema_version`（12 单测全绿）
- ✅ 行号漂移校正（方案 A）：内核 `LineDelta` + 编辑器书签平移
- ✅ 草稿箱连接元信息迁移（第③步）：scratchpad 用 `_at` API 落集中隐藏目录 `.RSmeta/scratchpad/meta/`
- ⏳ 标签（tag）：按约定不在此 crate 实现，后续接 `crates/tag`

## 验证

```sh
cargo test -p rds-file-meta
```
