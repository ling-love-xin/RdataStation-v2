> **存档说明**：这是 v2 初期的**脚手架与迁移路线说明**，原先放在仓库根 `README.md`。
> 根 `README.md` 现已改为面向 GitHub 的项目介绍（中英双语：`README.md` / `README.en.md`），
> 本文保留于此仅作历史参考。部分内容已过期（例如 crate 数量当时写作 12，现为 16）。
> 当前的架构现状请读 `docs/architecture/core-design-current.md`。

---

# RdataStation v2（初期脚手架说明 · 存档）

基于 **Rust + GPUI-kit** 的本地优先数据库分析与查询工作站（重构版）。

> 目录结构按 [GPUI-kit 编码指南](https://gpui-kit.com/zh-CN/docs/coding-guides/) 组织：**大型应用按业务能力组织 crate**，App Shell 只组合 Feature，依赖只向下。

## 目录结构

```
RdataStation-v2/
├── Cargo.toml              # workspace（12 个 crate）
├── rust-toolchain.toml
├── docs/
│   ├── architecture/       # v2 架构说明
│   └── migration/          # v1 → v2 迁移映射与指南
├── crates/                 # v2 代码（按业务能力组织）
│   ├── app/                # App Shell：组合窗口与 Feature，不承载业务逻辑
│   ├── workbench/          # 工作台：Dock 布局、活动栏、命令面板、编辑器工作台
│   ├── project/            # M1 双层数据架构：项目、系统级/项目级、promote、版本快照（模块特点见该 crate 的 README.md）
│   ├── engine/             # M2 双引擎基础设施：SQLite 元数据 + DuckDB 分析 + 驱动/缓存/迁移/日志
│   ├── connection/         # M3 数据源连接：原生连接 + DuckDB Secret 加速通道
│   ├── database/           # M4 数据库导航：元数据浏览器、对象属性面板
│   ├── scratchpad/         # M5 草稿箱
│   ├── analytics_resource/ # M6 资源分析
│   ├── mock/               # M7 Mock：元数据驱动生成（只写分析引擎）
│   ├── insight/            # M8 洞察：库/表/列画像、规则引擎
│   ├── plugin/             # M9 插件宿主：WASM / Sidecar / gpui-shell
│   └── shared/             # 跨 Feature 稳定能力（≥2 个真实使用方）
└── v1/                     # v1 源码暂存区（原样复制，不参与编译，逐 Feature 迁移）
    ├── backend/            # ← src-tauri（Rust 后端：core/commands/adapters/api/mock/docs）
    ├── frontend/           # ← src（Vue3 前端，退役参考）
    ├── docs/  tests/  prototype/  public/  scripts/  .trae/
    └── 根配置文件（README/package.json/tsconfig 等）
```

## 规范要点（GPUI-kit 编码指南）

- **按业务能力组织 crate**：同一能力的 model / service / view / command / dialog / workflow 放一起；禁止全局 `views/`、`models/`、`modals/` 目录。
- **依赖只向下**：`app → feature → engine/shared → gpui-base/component`；Feature 不得反向依赖 App Shell，也不得进入另一 Feature 内部。
- **Feature 协作**：优先用明确的 command、event、共享 service，不让彼此的 view 互相依赖。
- **共享 crate 门槛**：只有一项能力有清晰名称且 ≥2 个真实使用方时，才放入 `shared/`。
- **UI 规范**：`RenderOnce`（值类型）与 `Entity<T>`（跨 frame 状态）按需选择；`Root` 为每窗口第一层；theme token 取色不写死 hex；稳定 `ElementId`；虚拟列表处理大数据集；`render` 中禁止无条件 notify。
- **测试分层**：pure test → GPUI context test → `VisualTestContext` 交互测试 → example/application smoke test。

## 快速开始

```bash
cargo check --workspace   # 已验证通过（12 个 crate）
cargo run -p rds-app      # 启动最小 App Shell 窗口（GPUI-kit 已接入）
```

`app/src/main.rs` 是已接通的 GPUI-kit 最小示例（`application().run` → `init` → `open_window` → `Root::new(workspace, window, cx)`）；`engine` 已迁入 v1 DuckDB 分析引擎全量代码；`shared` 已迁入 v1 基础层（error/models/arrow/stream/utils/crypto 等）。迁移进度见 `docs/migration/v1-to-v2-mapping.md`。

接入 GPUI-kit 后（`app/src/main.rs` 见占位注释）：
```rust
app.run(move |cx| {
    gpui_kit::init(cx);
    cx.spawn(async move |cx| {
        cx.open_window(WindowOptions::default(), |window, cx| {
            let workspace = cx.new(|cx| Workspace::new(window, cx));
            cx.new(|cx| Root::new(workspace, window, cx))
        }).expect("failed to open window");
    }).detach();
});
```

## 迁移路线（详见 docs/migration/v1-to-v2-mapping.md）

1. **暂存**：v1 源码已整体复制到 `v1/`（1071 个源文件，排除 .git / node_modules / target），不参与 v2 编译。
2. **逐 Feature 迁移**：按 `docs/migration/` 映射表，把 `v1/backend/src/core` 对应模块迁入目标 crate；Tauri 命令演化为服务方法或 GPUI Action；Vue 视图按 Feature 重写为 GPUI 视图。
3. **垂直切片先行**：connection → database → workbench（编辑/执行/结果），跑通后铺开其余 Feature。

## 资源目录（assets）

| 目录 | 来源 | 内容 |
| --- | --- | --- |
| `assets\icons` | v1 `src-tauri\icons`（53 文件 7.1MB） | 应用图标：128/32/64 px PNG、android mipmap 系列、app-icon-coral-clean.png |
| `assets\public` | v1 `public`（7 文件 5.7MB） | 品牌主视觉（brand 3D story）、rds-icon-dark/light、popout.html 等 |
