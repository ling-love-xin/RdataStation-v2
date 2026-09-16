# 设置（应用级）· 开发方案

> 状态：**P0/P1a 已完成（2026-09-16），P1b 起待排**
> 关联：`settings-architecture.md`（准入与作用域裁决）、`settings-prototype-design.md`（页面形态）、`settings-crate-design.md`（crate 沿革）
> 基线：`cargo test -p rds-settings` → **10 项全绿、零告警**；`cargo check -p rds-workbench --all-targets` → 见 §5

## 0. 进度记录（最近在前）

### 2026-09-16（第三批）— 写盘失败可见 + 原子写 + 搜索高亮 + 窗口冒烟（P2 主体）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| **原子写** | `save_settings_to`：临时文件 + rename；失败时清掉临时文件并返回原因 | `crates/settings/src/lib.rs` |
| **失败可见（K2 关闭）** | 进程级错误槽 + `last_save_error()`；页面在底栏上方渲染危险色提示「未能写入 settings.json：…（本次改动只在本进程生效）」；下一次成功后自动清空 | 同上 + `settings_page.rs` |
| **可注入路径** | `load_settings_from` / `save_settings_to` + 测试用配置目录覆盖（`#[cfg(test)]`）——测试不再碰用户真实配置 | 同上 |
| **搜索命中高亮** | 命中片段上 `search.match.background` 底色（标签行分段渲染）；大小写折叠改变字节长度时整段不高亮（不切在非字符边界上） | `settings_page.rs` |
| **降级修复** | `product_tokens::get` 未安装时返回空集（原先 **panic**）：资产缺失 / 宿主未接线 / 测试都不应炸 | `product_tokens.rs` |
| **窗口测试（P1.7 冒烟）** | 真实 headless 窗口：渲染一帧 + 默认停在第一节 + 程序性改词→过滤条件同步 | `settings_page.rs` |
| 测试 | **22 项全绿、零告警**（较上批 +5：原子写往返 / 失败上报 / 错误槽 / 高亮切片 / 窗口冒烟） | §5 |

> 本批实测到两条**容易踩的事实**（已写进代码注释）：
> 1. `InputState::set_value` 的值**下一帧才可读**，且它自己**不发** `InputEvent::Change`——程序性改词必须同时改“生效条件”（本页 `set_query`，与 `resource_view.rs` 同结论）；
> 2. **headless 下合成的 `cx.emit` 不会投递给 `subscribe_in` 订阅者**——所以键盘输入路径没有自动化覆盖（本仓另两个同款订阅也只测程序性入口），登记为 K10。

### 2026-09-16（第二批）— 两栏页面实体 + 槽位分发表（P1b 主体）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| **两栏页面** | 标题行 / 搜索行 / 分节导航（`List` 组件，键盘与 hover 由组件给）/ 内容区（内部滚动）/ 底栏；分节与行**全部由登记表驱动**；`↺` 恢复默认 + 节级「重置本节」；搜索（节名·标签·说明·key）与无结果空态 | `crates/settings/src/settings_page.rs`（新增） |
| **页面尺寸常量** | `PAGE_WIDTH/HEIGHT` · `NAV_WIDTH` · `LABEL_WIDTH` · `ROW_MIN_HEIGHT` · `ROW_HEIGHT` · `SECTION_HEAD_HEIGHT` · `HEADER_HEIGHT` · `CARD_RADIUS/PADDING` · `HAIRLINE/ACTIVE_BAR` | `crates/settings/src/ui.rs`（新增；**不能**放 workbench，依赖方向所限，见原型 §7） |
| **槽位分发表** | `Slot` + `slot_for` / `slot_kind` / `slot_is_scalar`：把"键 → 类型化读写"收敛成一张表；`apply_by_key` / `value_by_key` 是唯一读写路径 | `crates/settings/src/registry.rs`、`lib.rs` |
| **数值预设档** | `SettingSpec::presets`（值 + 显示名）＋"上页的数值行必须有预设档"的契约约定（自由输入不做） | 同上 |
| 测试 | **17 项全绿、零告警**（registry 11 + 页面 3 + model 2 + product_tokens 1） | §5 |
| 宿主替换 | ⬜ **未做**：`workbench/src/view.rs` 正被并行改动，P1.6 等其落地后再接 | §2 P1.6 |

### 2026-09-16 — 文档 + 登记表 + 僵尸项裁撤（P0 + P1a）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 两份权威文档 | 原型设计（形态 / 行规格 / 清单）与架构裁决书（D1–D12 / 登记表 / 准入五条 / 退役清单 / 降级矩阵 / K1–K9 / Q1–Q6） | `docs/architecture/settings/settings-prototype-design.md`、`settings-architecture.md`；导航已登记（`docs/architecture/README.md`） |
| **代码侧登记表** | `SettingSpec`（key / 节 / 标签 / 说明 / 形态 / 默认值 / 生效方式 / 入口 / 消费方）+ `REGISTRY` 9 项 + `sections()` / `page_rows()` 供页面直接消费 | `crates/settings/src/registry.rs`（新增） |
| **契约测试 6 项**（准入护栏） | 登记项必须存在于模型 · **模型叶子必须登记** · 默认值一致 · 枚举默认值在候选内 · 两态文案完整 · 登记表卫生 + 节相邻 + page_rows 分类一致 | 同文件 `#[cfg(test)]` |
| **裁撤 7 个无生产者字段** | 删 `general`（language / restore_last_workspace）、`engine`（workspace_dir / cache_dir）两整节、`appearance.font_size`、`connection_defaults.{default_driver, query_timeout_ms}` | `crates/settings/src/model.rs` |
| 视图同步 | 删对应界面行（界面语言 / 工作区目录 / 默认数据源 / 查询超时）与死掉的 `value_text`；现有形态收敛为三节（外观 / 数据源导航 / 连接默认值） | `crates/settings/src/settings_view.rs` |
| 兼容性 | 旧 `settings.json` 带着被删节仍可解析（未开 `deny_unknown_fields`），新增测试 `legacy_config_with_removed_sections_still_loads` 锁住该保证 | 同上（model 测试） |
| 交互稿 HTML | 单文件、零外链、明暗两套、5 场景可切（默认 / 搜索有结果 / 搜索无结果 / 已修改 / 深色）；分段·开关·搜索·恢复默认可交互 | `docs/architecture/settings/settings-prototype.html`（*示意稿，非权威*） |

> 本轮**没有**改：持久化路径、写盘时机、消费方（workbench 侧一行未动）——裁撤字段的引用面全在 `settings` crate 内（已核对）。

## 1. 现状盘点

| 能力 | 现状 | 缺口 |
| --- | --- | --- |
| 模型与持久化 | `Settings` 4 节 + `settings.json`（`%APPDATA%/RdataStation`）；缺失 / 坏文件回退默认；**原子写 + 失败可见**（错误槽 → 页面提示） | 非 Windows 落临时目录（K5） |
| 服务与命令 | `SettingsService`（init / get / 9 个 `set_*` / 主题切换）、进程级连接默认值快照 | 构造期直读 2 处（K1）；`ToggleThemeMode` 未接线（K8） |
| 登记表（准入） | ✅ `registry.rs`：`REGISTRY` 9 项 + `Slot` 分发表 + `presets` + **11 项契约测试** | 待接线项（M6 `keep_versions`）还未入表（按设计如此） |
| 页面形态 | ✅ 两栏实体已落地（`settings_page.rs`：导航 / 内容区 / 搜索 / 恢复默认） | **宿主替换未做**（工作台仍挂旧视图，P1.6）；无窗口测试；`↺` 的 hover 卡未接 |
| 宿主接线 | workbench overlay 懒创建 + `on_close` / `on_open_cache` 回调 | 与 Quick Open 未互斥；`Esc` / `Ctrl+F` 未绑（P3） |
| 契约扫描 | 颜色扫描已含 `settings_view.rs` | **尺寸扫描未含**（K7，P3） |
| 主题设施 | `rds-theme.json` 明暗 + `product-tokens.json` 产品角色（暂住本 crate，K6） | 无需改动（页面用既有角色） |
| 待接线项 | — | M6 `resources.keep_versions`（P4） |

## 2. 阶段与任务

### P0 地基（已完成）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| P0.1 | 两份文档定稿（首版） | `docs/architecture/settings/` | 导航已登记；缺口表 / 阅读顺序同步 |
| P0.2 | 裁撤 7 个无生产者字段 + 视图同步 | `model.rs`、`settings_view.rs` | `cargo test -p rds-settings` 全绿；旧配置兼容测试通过 |
| P0.3 | 登记表 + 6 项契约测试 | `registry.rs` | 故意加一个未登记字段 → `every_model_leaf_is_registered` 变红（人工验证一次） |

### P1a 页面骨架 · 前置（已完成）

同 P0.2/P0.3。

### P1b 页面骨架 · 两栏（crate 内已完成；宿主替换待做）

| # | 任务 | 落点 | 验收 | 状态 |
| --- | --- | --- | --- | --- |
| P1.1 | 页面尺寸常量 | `crates/settings/src/ui.rs` | 数值与原型 §7 一致；契约扫描加入本文件（P3.3） | ✅ |
| P1.2 | `SettingsPage` 两栏壳（标题行 / 搜索行 / 分节导航 / 内容区 / 底栏） | `crates/settings/src/settings_page.rs`（新） | 切节不改弹层尺寸；内容区内部滚动 | ✅ |
| P1.3 | 分节由 `registry::sections()`、行由 `registry::page_rows()` 驱动 | 同上 | 页面行集 == 登记表 `entry != Module`（测试 `page_rows_match_the_registry`） | ✅ |
| P1.4 | 三种控件接组件库（`TabBar::segmented` / `Switch` / 预设档分段） | 同上 | 无手搓分段控件 | ✅ |
| P1.5 | 行规格：标签列 / 说明行 / 行分隔 / 恢复默认 / 节级重置 | 同上 | 原型 §4.2 逐条对照；说明行（登记表 `hint`）已渲染 | ✅ |
| P1.6 | 替换宿主渲染（`settings_view` → `settings_page`），旧文件退役 | `workbench/src/view.rs::render_settings_panel`、`settings/src/lib.rs` | 入口 / `Ctrl+,` / Quick Open 三入口行为不变 | ⬜ 等 `view.rs` 并行改动落地 |
| P1.7 | 窗口测试（切节 / 点击写入 / 恢复默认） | `settings_page.rs` 或 `tests/` | 需给 `settings` 加 `test-support` dev-dep（照 `project` 做法） | ⬜ |

### P2 搜索与状态（✅ 主体已完成）

| # | 任务 | 落点 | 验收 | 状态 |
| --- | --- | --- | --- | --- |
| P2.1 | 搜索行（`Input` + 命中高亮） | `settings_page.rs` | 搜节名 / 行标签 / 说明 / key 均可命中；无结果显示空态；命中片段上底色 | ✅ |
| P2.2 | 行级「恢复默认」（非默认值时出现）+ 节级「重置本节」 | 同上 | 恢复后与 `registry.default_json` 一致 | ✅（P1b 已做） |
| P2.3 | 写盘失败可见 | `settings/src/lib.rs` + 页面底栏上方提示 | `save_settings` 返回 `Result`；只读目录下改值 → 危险色提示；成功后提示自动收起 | ✅（含原子写） |
| P2.4 | 底栏提示文案（生效方式 / 路径） | `settings_page.rs` | 各行 `hint` 已写明生效方式；底栏仍为单句路径提示（按节汇总未做） | 🟡 部分 |

### P3 宿主与契约（待排）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| P3.1 | Quick Open 与设置页互斥 | `workbench/src/view.rs` | 两 overlay 不同时为真（打开其一并关闭另一个） |
| P3.2 | `Esc` 关闭 + `Ctrl+F` 聚焦搜索 | `app/src/main.rs`（`key_context("settings")`）+ 页面 | 键位生效且不影响编辑器 `Ctrl+F` |
| P3.3 | 尺寸契约扫描纳入设置页视图文件 | `workbench/tests/ui_contract.rs` | 契约测试通过；故意写裸 `px(...)` 能让它变红 |
| P3.4 | 宿主桥收口（`SettingsHost`：on_close / on_open_cache / 预留 on_restart） | `settings/src/settings_page.rs`、`workbench/src/view.rs` | 页面不再直接持有 `Rc<dyn Fn>` 散字段 |
| P3.5 | `ToggleThemeMode` 决断（接线或删除） | `settings/src/commands.rs`（+ app 绑键） | 二者之一，且文档同步（K8 关闭） |
| P3.6 | `K1` 两处构造期直读改走服务 | `workbench/src/{view.rs, panels/}` | 全仓无 `settings::load_settings()` 调用（除 settings 自身） |

### P4 接线延伸（待排，依赖上游模块）

| # | 任务 | 依赖 | 验收 |
| --- | --- | --- | --- |
| P4.1 | `resources.keep_versions` 落地（模型 + 登记 + 页面 `Input` 行 + 归档调用接线） | M6 dev-plan P2.4 | 改值后归档保留份数随之变化 |
| P4.2 | 项目列表排序行（页面） | P1b | 与选择器循环按钮同值（双入口一致性用例） |
| P4.3 | 界面缩放（若选"复活字号倍率"路线） | 架构 §14 Q4 拍板 | 按拍板结论另立任务 |

## 3. 测试场景

| # | 场景 | 层 | 现状 |
| --- | --- | --- | --- |
| T1 | 旧配置（缺节 / 带已裁撤节）解析回退默认 | 纯函数 | ✅ `model::tests` |
| T2 | facet 筛选序列化往返 | 纯函数 | ✅ |
| T3 | 未登记字段 → 契约测试变红 | 纯函数 | ✅ `registry::tests`（6 项） |
| T4 | 默认值表 == 模型默认值 | 纯函数 | ✅ |
| T5 | 每个 `set_*` 后 global 与磁盘一致（临时目录隔离） | 纯函数 + 文件 | ✅ 原子写往返 + 错误槽 |
| T6 | 写盘失败（只读目录）→ 返回值 + 页面提示 | 纯函数 + 视图 | ✅ 返回原因 + 页面横幅（只读目录的 UI 断言待 P1.7 扩） |
| T7 | 切节 / 搜索过滤 / 无结果空态 | 窗口测试 | 🟡 渲染 + 程序性改词已覆盖；键盘输入路径未覆盖（K10） |
| T8 | 恢复默认（行级 / 节级）后取值回落 | 窗口测试 | ⬜ P2.2 |
| T9 | 三入口（⚙ / `Ctrl+,` / Quick Open）开合并互斥 | 窗口测试 | ⬜ P3.1 |
| T10 | 双入口一致性（显示标签：`⋯` 菜单与页面） | 窗口测试 | ⬜ P4.2 |
| T11 | 尺寸 / 颜色契约扫描含设置页 | 契约测试 | ⬜ P3.3 |
| T12 | `Esc` / `Ctrl+F` 键位只在本页生效 | 窗口测试 | ⬜ P3.2 |

> 窗口测试按 `crates/project/src/ui/tests.rs` 骨架写；注意 `#[gpui_kit::test]` 与"测试模块不通配导入"的坑（见 gpui-kit-dev skill）。

## 4. 风险

| # | 风险 | 影响 | 对策 |
| --- | --- | --- | --- |
| R1 | 工作区在途改动与页面文件重叠（`workbench/src/view.rs` / `panels/`） | 合并冲突、误覆盖 | P0 收尾先提交；页面改动尽量集中在 `settings` crate，宿主只留一行替换 |
| R2 | 页面按登记表渲染后，行文案与原型文档漂移 | 文档失真 | 原型 §5 与登记表同轮修改（写进 §7.1 补充纪律） |
| R3 | `TabBar::segmented` 的交互相对于现有 `Button::toggled` 有差异（键盘 / 焦点） | 体感回退 | P1.4 先在单行验证，再铺开 |
| R4 | 写盘改 `Result` 触及所有 `set_*` 签名 | 大面积改动 | 保留旧签名（内部记状态）+ 新增 `try_set_*`，页面用后者（P2.3） |
| R5 | 搜索要求"只搜登记项"，而模块内入口的项不在页面 | 用户找不到（如属性面板宽度） | 无结果文案里说明"这里只列已登记的设置项"；入口在模块内的项由模块文档负责 |
| R6 | 插件（M9）在 beta3 落地时要求新增"插件"节 | 页面结构变化 | 架构 §10 已写规则：先过准入五条，且优先判定是否项目级 |

## 5. 验证命令

```bash
# 5.1 本 crate（契约测试 + 模型兼容）
cargo test -p rds-settings            # ✅ 22 项全绿（含 11 项登记表/槽位契约 + 1 项窗口冒烟）

# 5.2 宿主与视图契约（尺寸 / 颜色扫描 + 边栏状态机）
cargo test -p rds-workbench --test ui_contract

# 5.3 全目标编译（含测试目标；plugin 不在默认图上，不必为它付编译成本）
cargo check -p rds-workbench --lib    # ✅ 通过（43s，零告警）
cargo check -p rds-workbench --all-targets
cargo check -p rds-app

# 5.4 真机验收（改值后重启）
cargo run -p rds-app     # 设置页改主题 / 显示标签 / 建连超时 → 重启后仍生效
```

> 验证记录（2026-09-16）：`cargo test -p rds-settings` → **10 项全绿、零告警**；`cargo test -p rds-workbench --test ui_contract` → **5 项通过**；`cargo check -p rds-workbench --all-targets` → **通过、零告警**。
> 同轮 `cargo check -p rds-app` 曾失败于**在途改动**（`app/src/main.rs` 的 `NavReorderUp` / `NavReorderDown` 键位漏导入，属数据库导航条目重排那条线），已由后续在途改动补齐导入；设置模块的字段裁撤引用面只在 `settings` crate 内，与该失败无关。

## 6. 实现位置映射

| 设计决策 | 落点 |
| --- | --- |
| 准入五条 / 登记表（代码侧权威） | `crates/settings/src/registry.rs`（+ 架构 §6/§7） |
| 设置项字段与默认值 | `crates/settings/src/model.rs` |
| 服务与持久化（唯一写入者） | `crates/settings/src/lib.rs` |
| 页面（目标形态） | `crates/settings/src/settings_page.rs`（P1b 新增；`settings_view.rs` 退役） |
| 尺寸常量 | `crates/workbench/src/ui.rs`（P1.1） |
| 宿主 overlay / 互斥 / 桥 | `crates/workbench/src/view.rs::render_settings_panel`（P1.6 / P3.1 / P3.4） |
| 命令与键位 | `crates/settings/src/commands.rs`、`crates/app/src/main.rs`（P3.2 / P3.5） |
| 契约扫描 | `crates/workbench/tests/ui_contract.rs`（P3.3） |
| 文档同步 | `settings-prototype-design.md` §5、`settings-architecture.md` §6/§7 |

## 7. 明确不做

| 项 | 理由 |
| --- | --- |
| 设置页搜索语法（`@modified` 等） | 触发条件：设置项 > 40 条（原型 §4.3） |
| 「导出 / 导入设置」 | 与连接模块 C4 同源能力，等排期（原型 §12 Q3） |
| 插件节 / 插件设置项 | beta3 立项后再过准入（架构 §10） |
| 设置变更审计 | 无多用户场景（架构 §14 Q6） |
