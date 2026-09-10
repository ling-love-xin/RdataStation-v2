# crates/settings 设计文档（新增 crate）

> 状态：方案已确认 · 关联：`docs/architecture/crate-ownership-proposal.html`（归属总览）
> 依据：gpui-kit 编码指南示例结构（settings feature：`lib.rs / model.rs / settings_view.rs / commands.rs`）

## 1. 动机与边界

按编码指南的 crate 拆分标准——**独立状态与生命周期、稳定公开边界、多使用方**，设置满足全部三项：

| 判定项 | 结论 |
| --- | --- |
| 独立状态 | 设置项 model（通用 / 外观 / 引擎 / 连接默认值）是独立状态 |
| 生命周期 | 配置文件持久化，独立于窗口/工作台生命周期 |
| 稳定边界 | 对外只暴露 `SettingsService` + 命令（`OpenSettings` 等） |
| 使用方（≥2） | workbench（引擎路径、连接默认值）、app（主题切换）、各 Feature（读取设置） |
| 反例排除 | 不是"每页一个 crate"（无此问题）；不依赖 workbench（依赖只向下） |

**不建 crate 的对象**：主题（无独立状态，状态在 gpui-kit `Theme` global，见 `theme/theme-design.md`）。

## 2. crate 结构（对齐 coding-guides 示例）

```
crates/settings/
├── Cargo.toml
└── src/
    ├── lib.rs             # 模块声明 + SettingsService 对外 API + 命令注册入口
    ├── model.rs           # Settings model（分节设置项，serde 序列化）
    ├── settings_view.rs   # 设置面板视图（Dialog/panel，按节渲染）
    └── commands.rs        # GPUI Action：OpenSettings / theme.toggle.mode / 引擎路径变更等
```

## 3. 设置项 model（分节）

```rust
pub struct Settings {
    pub general: General,      // 通用：语言、启动行为
    pub appearance: Appearance,// 外观：主题模式（Light/Dark）、字号
    pub engine: Engine,        // 引擎：SQLite/DuckDB 路径、缓存目录
    pub connection_defaults: ConnectionDefaults, // 连接默认值：默认数据源、超时
}
```

- 每节独立结构体，`serde::Serialize/Deserialize`，缺失字段用默认值（向前兼容）
- 主题模式枚举：`ThemeMode { Light, Dark }`（与 gpui-kit 0.6 对齐）

## 4. 持久化

- 配置文件：用户配置目录下 `settings.json`（Windows：`%APPDATA%/RdataStation/`；实施时经 `dirs`/`std::env` 解析，按现有项目约定）
- 时机：启动加载 → 修改即写（防抖可后置）；主题模式即时生效（`Theme::change(mode, window, cx)`）
- 迁移：现 `crates/workbench/src/services/persistence_service.rs`（连接持久化）迁移为 settings 的持久化基础，或保留 connection 内、由 settings 统一配置入口（实施时定，见 §8）

## 5. 命令（commands.rs）

| 命令 | 动作 |
| --- | --- |
| `OpenSettings`（⚙ / 快捷键） | 打开设置面板 |
| `theme.toggle.mode` | `Theme::change(mode, window, cx)` 切换明暗 |
| `settings.edit.engine_path` 等 | 引擎路径修改 → 持久化 → 通知 engine |

## 6. 与 workbench / app 的接口

- **依赖方向**：`settings` 不依赖 workbench；`workbench` / `app` / 各 Feature 依赖 `settings`（依赖只向下）
- workbench 活动栏底部 ⚙ 按钮 → 触发 `OpenSettings` 命令（workbench 只保留触发入口，不承载设置逻辑）
- app 启动：加载 `SettingsService` → 应用主题模式（配合 `watch_dir` 主题资产）
- 设置面板形态：`settings_view.rs` 以 Dialog 呈现（与 Quick Open 同级弹层），左右侧边栏不新增 panel

## 7. 迁移清单

| 现状 | 迁移后 |
| --- | --- |
| `crates/workbench` 内 `Tool::Settings` 占位 | 删除，settings 视图由 `crates/settings::settings_view` 提供 |
| `crates/workbench/src/services/persistence_service.rs` | 配置读写能力移入 `crates/settings`（model + 持久化）；连接专用持久化按归属评估 |
| `crates/workbench/src/commands.rs` 的"打开设置" | 委托 `crates/settings::commands::OpenSettings` |

## 8. 实施步骤

1. `crates/settings` 骨架：`Cargo.toml`（依赖 `gpui-kit.workspace = true`）+ `lib.rs/model.rs/settings_view.rs/commands.rs`
2. 注册进 workspace（根 `Cargo.toml` members）
3. `SettingsService`（global）+ 持久化读写 + 默认值
4. `settings_view.rs` 分节渲染（外观节含明暗切换控件）
5. workbench ⚙ 接入 `OpenSettings`；`app` 启动加载并应用主题模式
6. 迁移 `persistence_service`，删除 workbench 内 Settings 占位
7. 验证：`cargo check --workspace` + 运行内打开设置、改主题、重启后设置保留

## 9. 未决项

- 连接持久化的最终归属（settings 统一配置 vs connection 自持，实施时按依赖方向定）
- 配置文件路径约定（随项目现有 `dirs` 用法确认）
