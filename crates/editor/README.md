# crates/editor — SQL 编辑器（M-编辑器）

> **一句话**：一个内核 + 三档能力——文本模式（不与数据库通信）/ SQL 模式（连接 + 执行 + 结果）/ 分析模式（单元 + 会话 + 输出）。

**设计文档在 `docs/architecture/editor/`，本文件只作入口指针**（项目约定：crate 内不复制设计）。

| 想了解 | 去哪 |
| --- | --- |
| 从哪开始读 / 边界 / 代码地图 / 改前必守约束 | `docs/architecture/editor/README.md` |
| 长什么样、怎么交互（三模式解剖 / 交互 / 主题映射） | `docs/architecture/editor/editor-prototype-design.md` |
| 为什么这样设计（概念模型 / 数据流 / D1–D23 决策 / 现状与已知问题） | `docs/architecture/editor/editor-architecture.md` |
| 做到哪了、下一步（Phase 0 / 1a / 1b / 1c） | `docs/architecture/editor/editor-dev-plan.md` |
| 怎么用（入口 / 导览 / 典型流程 / 快捷键 / FAQ / **USIT 验收清单**） | `docs/architecture/editor/editor-user-guide.md` |
| 可交互示意稿 | `docs/architecture/editor/editor-prototype.html` |

## 依赖方向（硬约束）

```
workbench → editor → engine / database / shared
```

editor **不得**依赖 workbench（壳层）或任何 Feature 的视图。

## 当前状态

Phase 0（地基）：`model`（文档 / 模式 / 只读 / 能力表）+ `mode`（模式判定规则表）已落地；
语句切分在 `engine::sql::split`（SQL 文本原语属 engine，本 crate 消费而不重复实现）。

## 验证

```sh
cargo check -p rds-editor --all-targets -j 2
cargo test -p rds-editor --lib -j 2
```
