# 项目管理模块（M1）· 模块入口

> **一句话**：「一个实例一个项目」的本地优先项目工作区——项目注册与生命周期、`.RSmeta` 元数据、实例锁与未保存草稿保护，加选择器 / 标题栏菜单 / 项目设置 / 语义对话框全套视图。
>
> 本文只做**文档索引 + 特点速览**：特点的完整提炼与代码落点见 crate 入口 [`crates/project/README.md`](../../../crates/project/README.md)，设计细节见下列文档，本目录不重复设计。

## 文档索引

| 文档 | 内容 | 什么时候看 |
| --- | --- | --- |
| `project-prototype-design.md` | 交互语义与原型：9 条决策、CRUD 场景、状态机、主题映射、范围外清单 | 改交互、评估范围 |
| `project-prototype.html` | 可交互原型（RDS 明暗双主题） | 视觉 / 文案对齐 |
| `project-dev-plan.md` | 任务划分（P0/A/B/C）、进度记录、测试场景清单、风险与对策、实现映射表 | 查历史决策与待办 |
| `project-view-architecture.md` | 宿主桥 `ProjectUiHost` 契约、状态所有权、对话框栈语义、窗口测试方案与坑 | 改 `ui.rs`、写视图测试 |

## 特点速览

- **一实例一项目**：会话唯一（`OpenProject`），不支持多窗口打开同一项目；切换 = 关闭当前 + 回选择器。
- **名册与元数据分离**：全局库 `project_info`（固定 / 软删 / 最后打开）+ 项目内 `.RSmeta/`（`project.db` / `analytics.duckdb` / `project.json`）；删除只动 `.RSmeta`，用户文件保留。
- **两道护栏**：OS 文件锁（退出自动释放，带「只读打开 / 仍要打开」逃生口）+ 未保存草稿拦截（确认后直达目标对话框）。
- **视图与 model / service 同 crate**：`ui.rs` 经 `ProjectUiHost` 注入宿主能力，workbench 只剩 `components/project_host.rs` 桥接。
- **状态可恢复、偏好可持久**：软删找回、失效路径重定位、固定置顶、排序方式跨会话保留；内置示例项目。

## 代码落点

`crates/project/src/`：`models.rs`（域模型）、`store.rs`（`.RSmeta`）、`lock.rs`（实例锁）、`service.rs`（编排）、`ui.rs` + `ui/tests.rs`（视图与 12 项窗口测试）。职责表见 crate README。

## 状态

- 主线完成（2026-09-11）：CRUD / 生命周期 / 实例锁 / 选择器 / 项目设置 / 系统目录选择器；12 项窗口测试 + 3 项集成测试。
- 范围外（原型 §12）：提升 / 引用（promote / snapshot）、移动或另存项目目录、DuckLake 远程项目（`ProjectPath::Remote` 仅模型层预留）。
