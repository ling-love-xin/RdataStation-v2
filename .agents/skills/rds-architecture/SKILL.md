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
任何 crate ────────► paths（运行时数据路径唯一解析点，无内部依赖）
```

- Feature 不得依赖 `app`
- Feature 间不得直接依赖对方的 view；协作走 command / event / shared service
- Feature **可以**直接依赖 `gpui-kit`（UI 基础设施），以便按 GPUI-kit 编码指南把同一业务能力的 model / service / view / command / dialog / workflow 放在同一 feature crate（官方示例：`workspace/src/{workspace_view.rs, rename_dialog.rs, commands.rs}`）
- 只有 ≥2 个真实使用方的稳定能力才进 `shared/`
- 依赖必须无环，始终指向更小、更稳定的 crate
- **运行时数据路径只能走 `paths::*`**：不得在 crate 里自己拼 `APPDATA` / `LOCALAPPDATA` / `env::temp_dir()` / `"RdataStation"` 目录名（口径与目录布局见 `docs/architecture/runtime/data-paths.md`）
- 开发期数据与临时目录由 `.cargo/config.toml` 的 `[env]` 钉在仓库内（`RDS_HOME=.rds`、`TEMP/TMP/TMPDIR=.rds/tmp`）；系统盘上的历史测试临时目录用 `tools/clean-temp.sh` 清
- **测试不得写产品数据根**：`cargo test` 已由 `paths` 的 `test-support` feature 自动隔离数据根（各成员在 `[dev-dependencies]` 打开，有静态契约兜住）；新增成员若依赖 `paths`/`engine`/`shared`，必须同步打开它

## crate 归属判定标准

新建 crate 前对照判定（依据 `docs/architecture/settings/settings-crate-design.md` §1）：

- 独立状态与生命周期（如 settings 持久化独立于窗口/工作台生命周期）
- 稳定公开边界（对外只暴露 Service + 命令）
- 使用方 ≥2
- **不建 crate 的对象**：无独立状态的能力（如主题——状态在 gpui-kit `Theme` global）

## 模块 → crate 映射

M1 `project` / M2 `engine` / M3 `connection` / M4 `database` / M5 `scratchpad` / M6 `analytics_resource` / M7 `mock` / M8 `insight` / M9 `plugin`；工作台 `workbench`；设置 `settings`；运行时数据路径 `paths`。

## 书写约定

- 注释、文档、**提交信息**一律**简体中文**（提交信息按个人 `AGENTS.md` 的格式：祈使句、首行 ≤ 50 字、不加句号、正文 72 列换行）
- 代码标识符 / 日志 / 错误串仍用英文；面向用户可见的文案用中文（英语仅作标识符与外部契约的键）

## 设计文档位置约定

- 设计决策统一放 `docs/architecture/<主题>/`（layout / theme / settings / connection 等目录），crate 内不复制设计文档（只留 README 级入口指针）
- 每份设计文档尾部带「实现位置映射表」（设计决策 → 代码文件），代码改动需同步更新映射表
- 实现说明随代码注释（简体中文）+ 各文档尾部映射表

## 关键文档

- `docs/architecture/overview.md`：三层架构 / 双层数据 / 双引擎 / 依赖方向
- `docs/architecture/runtime/data-paths.md`：运行时数据路径（单一根 `RDS_HOME`、旧布局迁移、git 卫生）
- `docs/architecture/crate-ownership-proposal.html`：crate 归属示意
- `docs/architecture/settings/settings-crate-design.md`：crate 拆分判定标准实例
