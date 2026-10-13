# crates/editor — SQL 编辑器（M-编辑器）

> **一句话**：一个内核 + 三档能力——文本模式（不与数据库通信）/ SQL 模式（连接 + 执行 + 结果）/ 分析模式（单元 + 会话 + 输出）。

**设计文档在 `docs/architecture/editor/`，本文件只作入口指针**（项目约定：crate 内不复制设计）。

| 想了解 | 去哪 |
| --- | --- |
| 从哪开始读 / 边界 / 代码地图 / 改前必守约束 | `docs/architecture/editor/README.md` |
| 长什么样、怎么交互（三模式解剖 / 交互 / 主题映射） | `docs/architecture/editor/editor-prototype-design.md` |
| 为什么这样设计（概念模型 / 数据流 / D1–D26 决策 / 现状与已知问题；D23 已于 2026-10-12 修订） | `docs/architecture/editor/editor-architecture.md` |
| 做到哪了、下一步（Phase 2c 排期的唯一权威） | `docs/architecture/editor/editor-impl-taskbook.md`（历史 Phase 0/1a/1b 流水见 `editor-dev-plan.md`） |
| 怎么用（入口 / 导览 / 典型流程 / 快捷键 / FAQ / **USIT 验收清单**） | `docs/architecture/editor/editor-user-guide.md` |
| 可交互示意稿 | `docs/architecture/editor/editor-prototype.html` |

## 依赖方向（硬约束）

```
workbench → editor → engine / database / shared
```

editor **不得**依赖 workbench（壳层）或任何 Feature 的视图。

## 当前状态（2026-10-13）

- ✅ **Phase 0 / 1a / 1b 已完成**：三模式内核与服务层、Dock 标签面板、执行闭环（执行族 / 中断超时 / 事务 / 历史）、三档执行通道（源库 / 本地加速 / 联邦，含联邦「源清单」）、补全、格式化 / 转译 / 执行计划、导出五档、DuckDB 分析、折叠。
- ✅ **Phase 2a 已完成**：结果集就地编辑（双击脏格 → 提交变更写回源库；`changes.rs` / `editability.rs` / `execution.rs`，六道防线、只做 UPDATE）。
- ✅ **Phase 2c 已完成 + 验收收尾**（T2c-1~6：Schema 面包屑 / 方言段 / 英文微标 / 重设菜单 / 工具栏分组自定义 / 面板布局持久化；**T2c-1「切 schema 真影响执行」2026-10-13 落地**——执行链逐执行携带文档 schema → 引擎会话上下文注入（SET search_path / USE），未限定表名真落到所选 schema；**516 tests passed**）；之后转 2b → 2d。
- ⬜ **未开工**：2b（通用模式）、1c/2d（分析模式笔记本视图，`notebook.rs` / `notebook_view.rs` 尚不存在）。

语句切分在 `engine::sql::split`（SQL 文本原语属 engine，本 crate 消费而不重复实现）。
排期与已知缺口的权威来源是 `docs/architecture/editor/editor-impl-taskbook.md`。

## 验证

```sh
cargo check -p rds-editor --all-targets -j 2
cargo test -p rds-editor --lib -j 2
```
