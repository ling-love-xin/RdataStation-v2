---
name: rds-architecture
description: RdataStation v2 架构硬约束：crate 依赖方向、模块归属判定标准、三层架构、设计文档位置约定。新增/拆分 crate、调整依赖、判断代码归属或做架构级改动时使用。
---

# RdataStation v2 架构约束

修改 crate 结构或判断代码归属时，遵守以下硬约束。

## 三层架构

1. 表现层：GPUI-kit（gpui-component / gpui-base）——视图、布局、主题
2. 服务层：Feature crates（`crates/*`）——业务能力，按 M1~M9 模块划分
3. 数据层：双层 × 双引擎（系统级 `global.sqlite + shared.duckdb`；项目级 `project.sqlite + analysis.duckdb`）

## crate 依赖方向（硬约束）

```
app ───────────────► 所有 Feature crates
Feature crates ────► engine, shared, gpui-kit（UI 基础设施）
engine ────────────► shared
shared ────────────► (gpui-kit / 第三方)
```

- Feature 不得依赖 `app`
- Feature 间不得直接依赖对方的 view；协作走 command / event / shared service
- Feature **可以**直接依赖 `gpui-kit`（UI 基础设施），以便按 GPUI-kit 编码指南把同一业务能力的 model / service / view / command / dialog / workflow 放在同一 feature crate（官方示例：`workspace/src/{workspace_view.rs, rename_dialog.rs, commands.rs}`）
- 只有 ≥2 个真实使用方的稳定能力才进 `shared/`
- 依赖必须无环，始终指向更小、更稳定的 crate

## crate 归属判定标准

新建 crate 前对照判定（依据 `docs/architecture/settings/settings-crate-design.md` §1）：

- 独立状态与生命周期（如 settings 持久化独立于窗口/工作台生命周期）
- 稳定公开边界（对外只暴露 Service + 命令）
- 使用方 ≥2
- **不建 crate 的对象**：无独立状态的能力（如主题——状态在 gpui-kit `Theme` global）

## 模块 → crate 映射

M1 `project` / M2 `engine` / M3 `connection` / M4 `database` / M5 `scratchpad` / M6 `analytics_resource` / M7 `mock` / M8 `insight` / M9 `plugin`；工作台 `workbench`；设置 `settings`。

## 设计文档位置约定

- 设计决策统一放 `docs/architecture/<主题>/`（layout / theme / settings / connection 等目录），crate 内不复制设计文档（只留 README 级入口指针）
- 每份设计文档尾部带「实现位置映射表」（设计决策 → 代码文件），代码改动需同步更新映射表
- 实现说明随代码注释（简体中文）+ 各文档尾部映射表

## 关键文档

- `docs/architecture/overview.md`：三层架构 / 双层数据 / 双引擎 / 依赖方向
- `docs/architecture/crate-ownership-proposal.html`：crate 归属示意
- `docs/architecture/settings/settings-crate-design.md`：crate 拆分判定标准实例
