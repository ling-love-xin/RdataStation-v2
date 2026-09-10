# 草稿箱模块 · 开发方案（P0 + Phase A/B/C）

> 状态：**模块根语义 + 项目级回收站 + 文件元数据/引用 + 面板首切片已落地**（2026-09-11，`cargo check --workspace --all-targets` 零告警；`rds-scratchpad` 7 项测试全绿） · Phase B 其余、Phase D（提升/存档/取回）与 Phase C 待续
> 关联文件：`scratchpad-prototype-design.md`（原型与已确认决策）、`scratchpad-prototype.html`（可交互原型）
> 前置：v1 后端/前端为行为蓝本（`v1/backend/src/core/scratchpad`、`v1/frontend/extensions/builtin/scratchpad`）；v2 后端已迁移（`crates/scratchpad`，`models`/`state`/`store` 47 个方法）
> 本方案核心变更：草稿箱根 = **模块目录 `{project}/scratchpad/`**（可见），内部元数据 `.RSmeta/scratchpad/`，回收站为**项目级** `.RSmeta/trash/`（草稿 + 资源共用）
> 复用 `connection-dev-plan.md` 的推进方式：Phase 划分 → 文件落点 → 验收 → 测试场景 → 风险

## 0. 进度记录（最近在前）

### 2026-09-11（二次）— 模块根 + 项目级回收站 + 元数据/引用

> 取代上一条中的 “根 = 项目目录” 与 “回收站落 meta” 两项（已按最终模型重写）。

**已完成**

| 项 | 内容 | 落点 | 验证 |
| --- | --- | --- | --- |
| 根语义定稿 | `ScratchpadStore::new` 改为 `root = {project}/scratchpad`（可见模块目录），`meta = {project}/.RSmeta/scratchpad`；`ensure_dir` 建模块根/meta/回收站 | `crates/scratchpad/src/store.rs` | 单测 `module_root_and_meta_isolated` |
| 项目级回收站 | 新增 `ProjectTrash`：`.RSmeta/trash/<id>/{payload,manifest.json}`，条目携带 `origin` + `original_rel_path` + `kind/size/deleted_at`；restore 按原相对路径重建、同名自动改名；跨模块条目拒绝在草稿箱还原 | `crates/scratchpad/src/trash.rs` | 单测 `trash_is_project_level_with_origin`、`restore_refuses_other_module_entries` |
| 旧数据迁移 | `.scratchpad/` 内容 → `scratchpad/`；旧 `.scratchpad.json` → `config.json`；旧 `.trash/` 与上一版 `{meta}/.trash` 均并入项目级回收站；空壳清理；幂等 | `store.rs`（`migrate_legacy_layout`） | 单测 `legacy_layout_is_migrated` |
| 数据源引用 | `FileMeta` 新增 `bound_connections`；新增 `ScratchpadStore::bind_connections`（只存连接 ID） | `models.rs` / `store.rs` | 单测 `bound_connections_roundtrip` |
| 外部引用可用性 | 新增 `ExternalReferenceStatus` + `external_reference_status()`（加载时探测路径是否存在） | `models.rs` / `store.rs` | 单测 `external_reference_persists_and_reports_status` |
| 面板 | 外部引用显示“丢失”态（置灰 + （丢失）后缀） | `crates/workbench/src/panels.rs` | `cargo check` 零告警 |

**下一步**：A5 文件监控；B4–B9（内联新建/模板、导入/引用对话框、搜索、右键菜单/重命名/删除/移动、回收站管理 + 撤销栏、多选、虚拟列表/排序、懒加载）；新增 **Phase D 提升/存档/取回**（§1.2）。

### 2026-09-11 — P0 + Phase A + Phase B 首切片

**已完成**

| 项 | 内容 | 落点 | 验证 |
| --- | --- | --- | --- |
| P0.1/P0.2 | 当前项目会话：环境变量 `RDS_PROJECT_PATH` → 全局库「最近打开项目」（取路径仍存在者）→ 空态；会话写入 `Shared::project` | `crates/workbench/src/services/project_session.rs`、`panels.rs`（`Shared`）、`view.rs`（`WorkbenchView::new` + 标题栏项目名） | `cargo check` 零告警 |
| A1/A2 | `ScratchpadStore` 根 = 项目目录；内部元数据 `{project}/.RSmeta/scratchpad/`（`config.json` + `.trash/`）；`ensure_dir` 只建 meta 目录 | `crates/scratchpad/src/store.rs` | 单测 `root_is_project_dir_and_meta_isolated` |
| A3 | 隐藏与防护：树跳过点前缀条目（`.RSmeta` 天然不可见）；`resolve_path_impl` 拒绝首段为点的路径 | 同上 | 单测 `internal_paths_are_blocked` |
| A4 | 旧布局迁移：`.scratchpad/.scratchpad.json` → 新配置；用户文件搬到项目根（**同名保留不覆盖**）；旧 `.trash` 并入新回收站；空目录清理；幂等 | 同上（`migrate_legacy_layout`） | 单测 `legacy_layout_is_migrated` |
| A6 | 清理语义：`CONFIG_FILE` 改为 `config.json`；回收站统一走 `trash_dir()` | 同上 | 编译零告警 |
| A7 | 测试：根语义/元数据隔离/路径防护/回收站落位/引用持久化/旧布局迁移 5 项 | `store.rs` `mod tests` | `cargo test -p rds-scratchpad` 5 passed |
| B1 | 依赖接线：workspace `scratchpad` 别名 + workbench 依赖 | `Cargo.toml`、`crates/workbench/Cargo.toml` | `cargo check` |
| B2/B3（部分） | 草稿箱面板首个切片：工具栏（＋文件 / 🗀文件夹 / ↻刷新）、只读树（展开/折叠/选中）、外部引用分组、回收站计数、空态、底部统计；`LeftPanel::Draft` 由占位接管 | `crates/workbench/src/panels.rs`（`ScratchpadView` / `render_scratchpad`） | `cargo check` 零告警 |
| A5 | 文件监控：**未做**（`notify` 已是构建图内传递依赖，接入时仍按 workspace.dependencies 统一声明） | `crates/scratchpad/src/state.rs` | — |

**Phase B 未完成（下一步）**：B4 内联新建输入与模板、导入/引用对话框；B5 搜索（文件名过滤 + 内容搜索结果落中央区）；B6 右键菜单/重命名/删除/移动/提升 + 键盘；B7 回收站管理 + 撤销栏；B8 多选/批量；B9 虚拟列表（>50）与排序；懒加载（当前 depth=4 全量，大项目需改按需加载）。

**Phase C 未开始**：编辑器草稿文件模式、`file_meta` 连接恢复、拖放、冲突 Diff、搜索替换。

**已知取舍**：
- 面板加载用 `Runtime::new().block_on(...)`（与 workbench 现有服务调用模式一致）；后续可改 `cx.spawn`。
- 新建文件默认 `未命名.sql`（冲突自动 `_1` 递补），未做模板选择。
- 树为 depth=4 全量加载，未做懒加载；项目根很大时有 IO 开销。

**关键文件**：`crates/scratchpad/src/store.rs`、`crates/scratchpad/src/state.rs`、`crates/workbench/src/panels.rs`、`crates/workbench/src/services/project_session.rs`、`crates/workbench/src/view.rs`、`Cargo.toml`、`crates/workbench/Cargo.toml`。

## 1. 现状结论（盘点摘要）

| 层 | 状态 |
| --- | --- |
| 域模型（`crates/scratchpad/src/models.rs`：`ScratchpadEntry`/`SearchMatch`/`ExternalReference`/`AnalyzableFile`/`FileMeta`/`DiffResult`/`ReplaceResult`） | ✅ 已迁移 |
| 存储（`crates/scratchpad/src/store.rs`：列表/CRUD/回收站/搜索/替换/Diff/引用/可分析文件/路径防护） | ✅ 已迁移（根目录仍为 `{project}/.scratchpad/`，需改造） |
| 状态（`crates/scratchpad/src/state.rs`：`ScratchpadState` + `watcher_active` 标志） | ⚠️ 有骨架，**未接入任何调用方**；文件监控未真正启动 |
| 占位文件（`model.rs` / `commands.rs` / `scratchpad_view.rs`） | ⚠️ 空占位，需清理或填充 |
| 视图（GPUI） | ❌ 未实现（workbench `LeftPanel::Draft` 为两行占位） |
| workbench 依赖 | ❌ 未依赖 `rds-scratchpad`（需补 workspace 依赖） |
| 当前项目会话（项目根路径来源） | ❌ 缺失（连接模块同样缺口：对话框手填项目路径） |

**关键缺口**：① 根目录语义切换（A）；② 面板视图与交互（B）；③ 编辑器联动与生态（C）；前置依赖 P0 项目会话。

## 2. 阶段划分

### P0 — 当前项目会话（前置，与连接模块共用）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| P0.1 | 启动确定「当前项目根」：优先启动参数，其次 `GlobalDatabaseManager::get_recent_projects` 首项，最后默认工作区目录 ✅ | `crates/app/src/main.rs` + `crates/workbench/src/services/`（新增 `project_session.rs`） | 启动后可拿到 `project_root: Option<PathBuf>` |
| P0.2 | 会话状态入 `Shared`（`Rc<RefCell<Option<ProjectSession>>>`），标题栏项目名 / 连接对话框 / 草稿箱共用 ✅ | `crates/workbench/src/panels.rs`（`Shared`）、`view.rs` | 三处读到同一项目根 |
| P0.3 | 无项目时降级：草稿箱空态、连接项目作用域禁用（消除「手填项目路径」）⬜ | workbench | UI 有明确空态 |

> P0 是草稿箱能显示内容的硬前提；若暂缓，草稿箱只能停留在空态。

### Phase A — 后端根语义切换（目标：根 = 项目目录，元数据隔离）✅ 已实现（A5 文件监控除外）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| A1 | `ScratchpadStore::new` 改为：`root = project_path`，`meta_dir = project_path/.RSmeta/scratchpad`，`config = meta_dir/config.json`，`trash = meta_dir/trash/` | `crates/scratchpad/src/store.rs` | 列出的是项目文件，不再有 `.scratchpad/` |
| A2 | `ensure_dir` 只创建 meta 目录（config 父目录 + trash），**不创建**项目根 | 同上 | 空项目首次调用后出现 `.RSmeta/scratchpad/` |
| A3 | 隐藏与防护：`scan_dir_tree` 跳过所有点开头条目（已有）+ 显式跳过 `.RSmeta`；`resolve_path_impl` 拒绝首段为 `.RSmeta` 或点开头的相对路径（防越权读写内部目录） | 同上 | `.RSmeta` 不在列表、不可被 API 访问 |
| A4 | 旧数据迁移：若 `{project}/.scratchpad/` 存在 → 迁移 `config.json`（原 `.scratchpad.json`）与用户文件到新语义（文件本就在根下则不移动），迁移后清理空目录（策略见原型 §8.2 待确认） | 同上（`migrate_legacy_layout`） | 迁移幂等；重复启动不报错 |
| A5 | 文件监控接入：用 `notify` 监听项目根（忽略 `.RSmeta`），变更经事件推送刷新树；`ScratchpadState::set_watching` 落地 ⬜ 未做 | `crates/scratchpad/src/state.rs`（+ 依赖 `notify`） | 外部新建/修改文件，面板自动刷新 |
| A6 | 清理占位死文件（`model.rs` / `commands.rs` / `scratchpad_view.rs` 按需合并进 `models/state/view`） | `crates/scratchpad/src/` | `cargo check -p rds-scratchpad` 零告警 |
| A7 | 单元/集成测试：根列表隐藏内部目录、路径穿越防护、回收站落位、引用/`file_meta` 读写、`get_analyzable_files` 相对路径 | `crates/scratchpad/tests/` | 测试全绿 |

### Phase B — 面板视图接入（目标：替换 `LeftPanel::Draft` 占位）△ 首切片已落地（只读树）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| B1 | 依赖接线：`Cargo.toml` workspace 增 `scratchpad` 别名；workbench 依赖 `scratchpad` ✅ | `Cargo.toml`、`crates/workbench/Cargo.toml` | 编译通过，依赖方向向下 |
| B2 | `ScratchpadPanel` 实体：面板头 / 工具栏 / 搜索 / 分组树 / 底部状态；`Shared` 增加草稿箱状态（选中、展开集合、排序、脏点集合）✅ 首切片（工具栏 + 只读树 + 分组 + 底部统计，状态存于 `ScratchpadView`） | `crates/workbench/src/components/scratchpad_panel.rs`、`panels.rs` | 面板渲染，切换活动栏可见 |
| B3 | 树渲染：递归行、类型图标、选中/悬停/脏点、相对时间、懒加载（`depth=0` → 展开加载）✅ 部分（递归行/类型色点/选中/悬停/展开折叠已做；相对时间、脏点、懒加载待补） | 同上 | 深目录展开正确 |
| B4 | 工具栏与空态：新建文件/文件夹（内联输入 + 模板）、导入、引用、排序、刷新；空态引导 | 同上 | 各按钮闭环 |
| B5 | 搜索：文件名实时过滤；内容模式调 `search_file_content`（正则/大小写），结果落中央编辑区 | 同上 + `panels.rs` `EditorPanel` | 结果带上下文、可跳行 |
| B6 | 右键菜单 + 键盘：重命名/删除/剪切/复制/粘贴/打开位置/提升；F2/Delete/Ctrl+A/Ctrl+N | `scratchpad_panel.rs`（绑定 Action/快捷键） | 全操作可用 |
| B7 | 回收站与撤销栏：折叠区列表/恢复/清空；删除后 5s 撤销 | 同上 | 误删可恢复 |
| B8 | 多选（Ctrl/Shift）与批量删除 | 同上 | 菜单按单/多选自适应 |
| B9 | 虚拟列表（>50 条）与排序（名称/大小/时间） | 同上 | 大目录流畅 |

### Phase C — 编辑器联动与生态

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| C1 | 中央编辑区「草稿箱文件模式」：`.sql` 打开 → 执行引擎 + 连接选择 + `Ctrl+S` 回存；`.py`/`.json`/`.md` 代码编辑器；防重复 Tab | `crates/workbench/src/panels.rs` `EditorPanel` | 双击打开、编辑回存正确 |
| C2 | `file_meta` 联动：执行后写 `last_connection_id`/`last_executed_at`；再次打开自动选连接 | `scratchpad` store + 编辑器 | 连接自动恢复 |
| C3 | 拖拽文件到编辑区插入内容；拖放文件进树导入 | workbench | 拖放生效 |
| C4 | 冲突处理：外部修改 → 冲突对话框 → `diff_with_content` Diff 弹窗 → 接受右侧 | `scratchpad` store + 弹窗 | 冲突可消解 |
| C5 | 搜索替换：预览计数 → `replace_in_file`（正则）→ 原子写回 → 刷新 | 同上 | 替换历史可回看 |
| C6 | 提升为分析资源：经 command/event 调 `analytics_resource`，**移动 + 归档锁定**（详见 Phase D） | `scratchpad` 命令 + 分析资源服务 | 提升后事件刷新 |
| C7 | 迁移文档验收：`cargo check --workspace` 零告警；无 `unwrap/expect` 新增；架构红线复核 | 全仓 | 全绿 |

### Phase D — 提升 / 存档 / 取回（分析资源）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| D1 | `resources/` 模块根约定（`{project}/resources/`）与资源登记模型（`resource_id / version / hash / promoted_from / readonly`） | `crates/analytics_resource` + `project.db` | 登记可查询、可回读 |
| D2 | 提升：`scratchpad/ → resources/` **move + 归档**，随文件归档数据源绑定/来源；完成后发事件刷新两侧 | `rds-scratchpad` 命令 + 资源服务 | 提升后草稿消失、资源只读 |
| D3 | 锁定：文件系统只读属性 + 应用守卫（禁重命名/移动；编辑器只读打开） | 存储 + 编辑器 | 资源不可写 |
| D4 | 取回（检出）：资源**复制**回 `scratchpad/`（新名 + 派生关系），资源本体不动 | 资源服务 + 草稿箱 | 可取回并编辑 |
| D5 | 版本：再次提升生成**新版本**（稳定 `resource_id`），旧版保留 | 资源服务 + `project.db` | 版本递增、引用不断 |
| D6 | 资源删除 → 项目级回收站（`origin = "resources"`；在资源模块还原） | `ProjectTrash` + 资源服务 | 条目来源正确、不在草稿箱误还原 |

## 3. 测试场景清单（参照 v1 §四，Phase A 覆盖 1–6）

1. **根语义**：项目根有 `a.sql` / `data/report.sql`，面板只列用户文件，`.RSmeta` 不出现
2. **新建/重命名**：根与子目录内联创建、重命名（重名/空值被拒）
3. **软删除/恢复**：删除 → 回收站计数 +1 → 撤销栏恢复 → 文件回到原位
4. **移动/复制**：剪切粘贴跨目录；复制生成 `_copy`
5. **外部引用**：添加别名引用 → 列表显示；非法路径置灰；移除生效
6. **路径防护**：传入 `../`、`.RSmeta/x` 被拒
7. **可分析文件**：`.csv`/`.parquet` 被 `get_analyzable_files` 识别，相对路径正确（Phase B）
8. **搜索**：文件名过滤；内容模式正则/大小写；结果上下文与跳行；>500 条截断（Phase B）
9. **编辑器**：`.sql` 草稿模式打开、执行、`Ctrl+S` 回存；连接自动恢复（Phase C）
10. **冲突/Diff**：外部修改触发冲突 → Diff 红绿 → 接受（Phase C）
11. **监控**：外部新建文件面板自动刷新；`.RSmeta` 变更不触发（Phase A）
12. **大目录**：>50 条启用虚拟滚动，滚动/排序无卡顿（Phase B）
13. **主题**：明暗切换核对面板底/选中条/搜索框/菜单/撤销栏（`theme-preview.html` 为基准）
14. **旧数据迁移**：存在 `.scratchpad/` 的旧项目启动后迁移且幂等（Phase A）

## 4. 风险与对策

| 风险 | 对策 |
| --- | --- |
| 缺少项目会话（P0）导致草稿箱无内容 | P0 优先；未完成时明确空态，不阻塞后端 A 阶段 |
| 模块根内若误放内部态，泄露到 UI 或被 API 访问 | 内部态统一 `.RSmeta/<模块>/`（天然在模块根之外）+ `resolve_path` 拒绝点前缀路径；测试覆盖 |
| 回收站被跨模块误还原 | 条目带 `origin`；`ScratchpadStore::restore_from_trash` 只接受 `scratchpad` 来源 |
| 提升后仍能改一份，与存档分叉 | 提升 = move + 只读锁定；修改只能取回（检出）后提升新版本（Phase D） |
| 文件监控把 `.RSmeta`（SQLite/DuckDB）变更当作刷新信号，造成抖动 | watcher 过滤 `.RSmeta` 与临时文件（`-wal`/`-shm`/`~`） |
| 240px 面板塞入搜索/替换/Diff 过挤 | 导航留侧栏，重结果/弹窗落中央编辑区（原型 §4.3/§4.5） |
| 根目录可能很大（用户整个项目） | 懒加载 + 虚拟列表 + 内容搜索流式/超时/截断 |
| 脏状态与外部冲突误判 | 脏点集合与 watcher 事件联动；冲突弹窗人工裁决 |
| 模板/图标颜色对比度不足 | 先用标准 token；不足再补产品语义 token（原型 §6.4） |
| `.RSmeta` 与 `.scratchpad` 命名/大小写不一致 | 统一走 `project` crate 的 `RS_META_DIR_NAME`（`.RSmeta`），不各写各的 |

## 5. 实现位置映射（设计决策 → 代码文件）

| 设计决策 | 代码文件 |
| --- | --- |
| 模块根 = `{project}/scratchpad/`；内部元数据 `.RSmeta/scratchpad/` | `crates/scratchpad/src/store.rs`（`ScratchpadStore::new` / `ensure_dir` / `scan_dir_tree` / `resolve_path_impl`） |
| 项目级回收站（含来源/原路径） | `crates/scratchpad/src/trash.rs`（`ProjectTrash` / `TrashEntry` / `TrashManifest`） |
| 文件元数据 / 数据源绑定 / 外部引用可用性 | `crates/scratchpad/src/models.rs`（`FileMeta::bound_connections` / `ExternalReferenceStatus`）、`store.rs`（`bind_connections` / `external_reference_status`） |
| 旧 `.scratchpad/` 迁移（→ 模块根/元数据/项目回收站） | `crates/scratchpad/src/store.rs`（`migrate_legacy_layout` / `move_dir_contents` / `ingest_trash_dir`） |
| 文件监控 | `crates/scratchpad/src/state.rs`（`notify`）+ 事件推送 |
| 域模型 / 存储 API | `crates/scratchpad/src/{models,state,store}.rs` |
| 面板视图（头/工具栏/树/分组/空态/撤销栏） | `crates/workbench/src/components/scratchpad_panel.rs` |
| 左 Dock 装配（`LeftPanel::Draft`） | `crates/workbench/src/panels.rs`（`SidebarPanel`） |
| 当前项目会话 | `crates/workbench/src/services/project_session.rs`、`crates/app/src/main.rs` |
| 中央编辑区草稿文件模式 | `crates/workbench/src/panels.rs`（`EditorPanel`） |
| 依赖接线 | 根 `Cargo.toml`（`scratchpad` 别名）、`crates/workbench/Cargo.toml` |
| 主题 token（含可选 `scratchpad.search.match.background`） | `assets/themes/rds-theme.json`（+ 必要时 `product-tokens.json`） |

## 6. 验证方式

- 每阶段：`cargo check -p rds-scratchpad -p rds-workbench -p rds-app --all-targets` 零告警 + 对应测试（`crates/scratchpad/tests/`）
- UI：`cargo run -p rds-app` 手动走通 §3 场景清单
- 主题：明暗切换核对 token（`docs/architecture/theme/theme-preview.html` 为基准）
- 阶段完成后回填本文件「状态」与原型文档同步
