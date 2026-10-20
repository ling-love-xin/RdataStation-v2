# 标签系统统一规格（正式规格）

> 状态：**已定稿**（2026-10-20 对齐全部待定口径）
> 替代：《标签系统统一方案（讨论纪要 · 交接文档）》`tag-system-plan.md`（2026-10-20 讨论产出）
> 适用范围：导航连接（M4）/ 分析资源（M6）/ 草稿箱（M5）三模块的标签能力统一
> 新会话入口：先读本文（含两张现状/目标图），即可按 §11 执行路径动手。

## 1. 目标

1. 消除两套分叉的标签实现（导航字符串集合 vs 分析资源实体），补齐草稿箱缺失的标签能力。
2. 标签作为**产品级统一能力**（独立 crate），三模块共同消费；导航保持轻量、分析资源做重、草稿箱轻接入。
3. 打通「草稿 → 归档 → 资产库」标签继承链路。
4. UI 统一用 gpui-kit 组件与主题 token，禁止手搓与裸色值。

## 2. 现状实证（2026-10-20 代码核实）

### 2.1 三方现状对比

| 消费方 | 代码位置 | 组织方式 | 标签现状 | 接入深度 |
| --- | --- | --- | --- | --- |
| 导航连接 | `crates/engine/src/persistence/connection_org_store.rs`（`connection_tags` 表 :99、`list_tags` :189）；`crates/database/src/nav_store.rs:117`（包装） | 分组树 | 纯字符串集合 `connection_tags(connection_id, tag, created_at)` 联合主键；**随连接所在库**（全局库 / 项目库各一张）；`DataSourceService` 同步为权威检索表 | 现状轻量；UI 自绘 chip + 逗号分隔输入 |
| 分析资源 | `crates/analytics_resource/src/tag.rs`（全文件）；实体定义 `models.rs:66` | 资源列表 | **实体标签** `analytics_tags`（id/name/color/icon/scope/created_at/deleted_at）+ 关联表 `analytics_resource_tags`；索引层在 **`project.db`**（`lib.rs:10`） | 做重：列表筛选 / 详情打标 / 批量打标 |
| 草稿箱 | `crates/scratchpad/`（`models.rs` 无标签字段；`store.rs` / `jobs.rs` 文件操作） | 文件树 | **无标签概念** | 轻接入：列表标签筛选 + 归档继承 |

### 2.2 现状图

```mermaid
flowchart TB
    subgraph 现状：三套分裂的标签能力
        NAV["导航连接<br/>connection_tags(connection_id, tag)<br/>纯字符串 · 联合主键 · 随连接所在库<br/>全局库 + 项目库各一张"]
        AN["分析资源<br/>analytics_tags 实体<br/>id / name / color / icon / scope / deleted_at<br/>project.db · 软删 · (name,scope) 部分唯一"]
        SP["草稿箱<br/>无标签概念<br/>ScratchpadEntry / FileMeta 均无标签字段"]
    end
    NAV -. 模型 / 存储 / 语义 分叉 .-> CORE["统一 tag crate<br/>（目标）"]
    AN -. 模型 / 存储 / 语义 分叉 .-> CORE
    SP -. 缺失能力，轻接入 .-> CORE
```

### 2.3 关键实证发现（含对讨论纪要的修正）

1. **导航标签存储本身是两级库**：`connection_tags` 在全局库（`global.db`）与项目库（`.RSmeta/project.db`）各建一张，随连接所在库（`connection_org_store.rs:5-8`）。统一后关联表继续保留这一特性。
2. **分析资源标签定义当前在 `project.db`**（`lib.rs:10`「索引层（project.db）」）；按已定口径上移全局表。
3. **部分唯一索引已存在**：`analytics_tags` 的 `(name, scope) WHERE deleted_at IS NULL`（`tag.rs:33-34` 注释确认），软删后同名可重建——统一模型直接沿用。
4. **删除语义现状**：`delete_tag` = 软删行 + 清本库全部关联，同一事务（`tag.rs:309-348`）。统一为 id 引用后**不再需要清关联**（见 §5 决策 D2）。
5. **gpui-kit 0.7.1 `Tag` 组件无内置关闭按钮**（`vendor/gpui-component/src/tag.rs` 实测：仅 `new/primary/secondary/danger/success/warning/info/custom/color/with_variant/outline/rounded/rounded_full`，`RenderOnce`，无 close/remove/on_click）。讨论纪要 §3.3「自带关闭 ×」**不成立**，删除入口需组合实现（见 §9.2）。
6. **归档请求无标签字段**：`ArchiveRequest`（`model.rs:214`）只有 `source_path/rel_path/name/existing_resource_id`，需新增 `tags`。

## 3. 统一模型规格

### 3.1 标签实体

```rust
pub struct Tag {
    pub id: String,          // 前缀 tag_{uuid}，跨域唯一
    pub name: String,        // trim 后非空；key:value 命名约定见 §6
    pub color: Option<String>, // 语义色名（映射主题 token），全模块共用
    pub icon: Option<String>,  // 图标名（IconName），全模块共用
    pub scope: String,       // 派生：global | project（由存储域决定，不可编辑）
    pub created_at: String,
    pub deleted_at: Option<String>, // 软删
}
```

- 字段 `color` / `icon` / `scope` **全模块共用**（已定口径）。
- `color` / `icon` 可空：未指定时 UI 用默认档位（`Tag::secondary` 语义 / 无图标），存量迁移数据落默认值。
- **同名约束**：`(name, scope) WHERE deleted_at IS NULL` 部分唯一索引（沿用 analytics 现状）。
- **scope 派生原则**（对齐 `tag.rs:274-276` 注释）：scope 由标签定义所在存储域决定，不是可编辑项。

### 3.2 存储布局（已定：两级）

| 存储域 | 库 | 表 | 定义 scope | 消费方 |
| --- | --- | --- | --- | --- |
| 全局 | 系统 `global.db`（经 `paths::*` 定位） | `tags` | `global` | 导航连接标签定义 + 分析资源标签定义 |
| 项目 | `{项目}/.RSmeta/project.db` | `tags` | `project` | 草稿箱标签定义 |

- 一张 `tags` 表结构相同（§3.1 字段），按库区分 scope。
- **关联表各模块自建**（§3.3）。
- 运行时数据路径一律走 `paths::*`，不得自拼目录（rds-architecture 硬约束）。

### 3.3 关联表

| 模块 | 关联表 | 所在库 | 语义 |
| --- | --- | --- | --- |
| 导航 | `connection_tags(connection_id, tag_id, created_at)`（改造现有表，`tag` 列 → `tag_id`） | 随连接所在库（全局库 / 项目库） | 连接 ↔ 全局标签 |
| 分析资源 | `analytics_resource_tags(resource_id, tag_id)`（保留） | `project.db` | 资源 ↔ 全局标签 |
| 草稿箱 | `scratchpad_tags(entry_id, tag_id)`（新增） | `project.db` | 草稿条目 ↔ 项目标签 |

### 3.4 跨库解析策略（决策 D1）

项目库的关联表（项目连接的 `connection_tags`、`analytics_resource_tags`、`scratchpad_tags`）引用**全局库** `tags` 表时，SQLite 无法跨库 join，统一走**应用层解析**：

- 读：关联表取 `tag_id` 集合 → 批量 `IN` 查询目标库 `tags` 表 → 内存合并（仿 `nav_store.rs::list_all_tags` 的合并模式）。
- 写：定义写目标库 `tags` 表，关联写关联表所在库。
- 两个库之间无事务保证；一致性靠「定义先写、关联后写」的顺序 + 读取侧过滤 `deleted_at IS NULL` 兜底。

## 4. TagStore 契约

`tag` crate 暴露统一 trait，三模块各自 store 实现：

```rust
pub trait TagStore {
    // 定义
    fn create_tag(&self, name: &str, color: Option<&str>, icon: Option<&str>) -> Result<Tag, CoreError>;
    fn rename_tag(&self, id: &str, name: &str) -> Result<Tag, CoreError>; // 全局重命名，关联自动跟随
    fn delete_tag(&self, id: &str) -> Result<(), CoreError>;              // 软删定义行（§5 D2）
    fn get_tag(&self, id: &str) -> Result<Tag, CoreError>;
    fn list_tags(&self) -> Result<Vec<Tag>, CoreError>;                   // 过滤软删
    // 关联
    fn attach(&self, owner_id: &str, tag_id: &str) -> Result<(), CoreError>;
    fn detach(&self, owner_id: &str, tag_id: &str) -> Result<(), CoreError>;
    fn tags_of(&self, owner_id: &str) -> Result<Vec<Tag>, CoreError>;     // 关联 + 定义，过滤软删
    fn owners_of(&self, tag_id: &str) -> Result<Vec<String>, CoreError>;
    fn usage_counts(&self) -> Result<Vec<(String, i64)>, CoreError>;      // 引用数（facet 排序用）
    // 治理
    fn cleanup_empty_tags(&self) -> Result<usize, CoreError>;             // 无引用的活跃标签 → 软删
}
```

- 同名约束、部分唯一索引、软删过滤在 crate 层统一实现（各模块 store 只接关联表差异）。
- `scope` 由 store 所属存储域隐式决定，不入参数。

**骨架实现说明（2026-10-20，已落地）**：契约在 `crates/tag` 中拆两层实现——

- `TagDefinitions`：定义 CRUD（`create/rename/delete/restore/get/get_by_name/list/list_deleted`），由 [`TagTable`] 实现（同步 rusqlite 连接，仿 `connection_org_store` 模式，面向 UI 线程轻量调用）；
- `TagStore: TagDefinitions`：在定义层之上扩展关联与治理（`attach/detach/tags_of/owners_of/usage_counts/cleanup_empty_tags`），由三模块各自的 store 实现（关联表在模块侧）。

## 5. 软删口径（已定）

- **软删**（`deleted_at`），**不引入回收站**（已定口径；回收站价值与软删恢复窗口重叠，且标签是轻量元数据）。
- **部分唯一索引** `(name, scope) WHERE deleted_at IS NULL`：软删后可同名重建（沿用 analytics 现状）。
- **决策 D2（删除语义简化）**：统一为 id 引用后，删除 = **仅软删定义行**，关联保留但读取侧一律过滤 `deleted_at IS NULL`。重建同名标签产生**新 id**，旧关联指向旧 id 自然不可见，不会「带回旧归属」——因此**无需跨库级联清关联**（与现状 `delete_tag` 的「软删 + 清关联同事务」语义不同，是本次统一的结构性收益）。
- **清理策略**：软删记录保留、不再展示；「空标签清理」= 软删无引用的活跃标签；管理覆盖层提供「查看已删标签 + 恢复」入口，不引入自动时间窗口。

## 6. key:value 规范（已定：标签名约定，不进模型）

- 标签名采用 `key:value` 命名约定（如 `env:prod`、`driver:pg`），**存储仍是字符串**，不新增字段。
- 解析层识别：facet 弹层把 `key:` 前缀拆为显式维度做精确匹配（`tag:` facet 的增强），与现有 `type:` / `driver:` / `scope:` 隐式 facet（原型 §1.1 规则 2）互补。
- 约束：`key` 与 `value` 均非空、不含 `:`；解析失败按普通标签处理。

## 7. 三模块接入规格

### 7.1 导航连接（升格，拆两步）

- **存储实体化**：`connection_tags` 的 `tag` 列改为 `tag_id`，引用全局 `tags` 表；存量字符串迁移为实体（§10.1）。
- **UI**：行内 chip 用 `tag::Tag`；标签编辑器从逗号分隔文本升级为「已加 chip 集合 + 追加输入框」；facet 弹层标签列表按引用数降序 → 名称升序。
- 维持现状的轻量定位：连接级组织，不做批量打标。

### 7.2 分析资源（做重）

- **定义上移全局表**：`analytics_tags`（project.db）迁移到全局库 `tags`（§10.2），`tag.rs` 瘦身为「关联 + 查询」；`analytics_resource_tags` 保留在 project.db。
- **做重能力**：列表标签筛选 / 详情打标 / 批量打标，UI 统一换 `tag::Tag`。

### 7.3 草稿箱（轻接入）

- project 库 `tags`（scope=project）+ 新增 `scratchpad_tags(entry_id, tag_id)`。
- 接入点：列表标签筛选 + 归档继承；文件本体不动（标签是索引，不写文件元数据）。

### 7.4 归档继承链路

```
草稿条目打标（project tags）
  → 归档：ArchiveRequest 新增 tags 字段（携带标签名列表，M6 不依赖 scratchpad 类型）
  → M6 创建资源：标签名逐个「同名复用全局定义，无同名则新建全局标签」
  → analytics_resource_tags 关联
```

- 依赖方向不变：`scratchpad → analytics_resource`（`model.rs:211` 注释），tags 用字符串名传递。
- **决策 D3（跨域复用）**：归档时同名（name 精确匹配）复用全局定义，避免资产库出现同名异色标签；颜色/图标以全局定义为准。

## 8. 排序与展示规则（已定）

- 行内 chip：**添加顺序稳定**（增删不重排）。
- facet 弹层标签列表：**引用数降序 → 名称升序**。
- 不提供标签手动排序（扁平集合，价值低）。

## 9. UI 规格

### 9.1 组件选型

- 行内展示与 chip：`gpui_kit::component::tag::Tag`（0.7.1 vendor 实测 API：`Tag::new/primary/secondary/danger/success/warning/info/custom(color, fg, border)/color(ColorName)/with_variant/outline/rounded/rounded_full`；`TagVariant` 六档，内部取色走 `cx.theme()`）。
- 颜色映射：标签 `color` 字段（语义色名）→ `TagVariant` 或 `Tag::custom`，映射表在 tag crate 提供；禁止裸 hex/rgb。
- 尺寸：结构尺寸用 `workbench_shell::ui` 常量（如 `CONTROL_HEIGHT_SM`），局部间距用 Tailwind 尺度（rds-ui-spec）。

### 9.2 删除入口（修正纪要 §3.3）

0.7.1 `Tag` **无内置关闭 ×**。删除入口组合实现：chip 尾部嵌 `button::Button::ghost().icon(IconName::…)` 作为关闭按钮（自带 hover/键盘/a11y），或 chip 旁独立删除按钮。禁用 `div().on_click()` 手搓。

### 9.3 编辑器

「已加 chip 集合 + 追加输入框」：输入框用 `Input`（复用 chrome.rs 现有 `InputState` 模式，Enter 提交、追加为 chip）；程序化回填不发事件（gpui-kit-dev：`set_value` 不触发 `InputEvent::Change`，依赖此语义做预填）。

## 10. 存量数据迁移

### 10.1 导航（connection_tags → 实体）

- 遍历全局库 + 各项目库的 `connection_tags` 现有行：`tag` 字符串去重 → 插入全局 `tags`（`INSERT OR IGNORE`，默认 `color=None / icon=None`，scope=global）→ 关联改写为 `tag_id`。
- 口径仿现有回填迁移（`connection_org_store.rs::backfill_tags_inner`）：**宁可少迁，不误伤**；表结构改造走迁移脚本，幂等。
- 同名标签跨库合并：全局库与项目库的同名连接标签归并为一个全局标签。

### 10.2 分析资源（analytics_tags → 全局 tags）

- `analytics_tags` 行迁移到全局 `tags`，保留 `color/icon/created_at/deleted_at` 原值；**scope 归一为 global**（原 `scope` 列语义并入存储域，不再作为标签可编辑属性；存量非 `global` 的 scope 值在迁移报告中列出，人工确认）。
- `analytics_resource_tags` 的 `tag_id` 不变（id 全局唯一，无需重映射）。

## 11. 执行路径

1. ~~对齐模型统一规格~~ → **本文即规格**（2026-10-20 完成）。
2. **tag crate 骨架**：实体模型 + `TagStore` trait + 全局/project 两域存储实现 + 部分唯一索引 + 软删 + 空标签清理 + 重命名（含单测，仿 `metadata_cache_index.rs::isolated()` 线程局部隔离范式；测试数据根隔离走 `paths` test-support）。~~→ ✅ 已完成并验证~~（11 测全绿；含 `get_tags_by_ids` 跨库批量读取、`insert_migrated` 迁移插入）。
3. **分析资源接入**：定义上移全局表（迁移）+ 做重（列表筛选 / 详情打标 / 批量打标，UI 换 `tag::Tag`）。**→ ✅ 已完成并验证**（2026-10-20：162+10+25 测全绿、workbench lib 84 测 + `ui_contract` 9 测全绿；含迁移 024 去关联表 tag 外键、存量迁移幂等 + 同名重映射、chip 换 `Tag::secondary().with_size(Size::Small)` + 内嵌 `Button::ghost` 关闭）。
4. **草稿箱轻接入**：project `tags` + `scratchpad_tags` + 列表标签筛选 + 归档继承（`ArchiveRequest.tags` + 跨域复用）。**→ ✅ 已完成并验证**（2026-10-20：scratchpad 46 测全绿（含标签存储读写、标签筛选纯函数）；analytics 163 测全绿（含 `initial_tags` 预填单测）；迁移 025；归档单选预填 / 多选直接携带）。**workbench 全链路验证已闭环**（2026-10-20 晚：其他轨修复 `group_manager_dialog.rs` 后阻塞解除——workbench lib 84 测 + `ui_contract` 9 测 + 全部集成测试文件全绿；仅 `tests/mock_panel_probe.rs` 4 测失败，该文件是其他轨 M7 的临时排查测试，其 mock 组件（`mock_host` / `mock_generator` / `mock_jobs` / `crates/mock`）零引用本方案改动的存储层（`connection_org_store` / `nav_store`），与标签接入无因果）。
5. **导航 UI 调整**：`connection_tags` 实体化 + 存量迁移；行内 chip / 标签编辑器换 `tag::Tag`；facet 标签排序；「管理分组与标签覆盖层」（批量重命名 / 合并分组 / 清理空标签）并入。**→ ✅ 已完成并验证**（2026-10-20：engine 583 测全绿（含实体化迁移测试 `tag_entity_backfill_and_soft_delete`）；database 154 测全绿（含 nav_view 实体标签构造适配）；tag 11 测全绿）。
   - 存储层：`connection_tags` 增加 `tag_id` 列（新库建表直含，老库 `PRAGMA` 检测后幂等 `ALTER TABLE ADD COLUMN`，不落迁移文件——与 `is_primary` 补列同模式）；`ensure_tables` 链追加 `ensure_tags_table`（幂等建 `tags` 表，DDL 与 tag crate 逐列一致 + `tags_name_scope_active` 部分唯一索引）+ `backfill_connection_tag_ids`（存量归并：`SELECT DISTINCT tag` 生成 `nav_<hex>` 定义行 → `UPDATE` 回填 `tag_id`；scope 按库派生 global/project）。
   - 读写实体化：`list_tags` / `list_connections_by_tag` / `list_all_tags` / `list_tag_pairs` 全改 `JOIN tags` + 过滤 `deleted_at`（D2）；`list_tag_pairs` 签名不变（返回名字对），新增 `list_tag_rows()` 返回 `(conn_id, NavTagRow{id,name,color,icon})` 供上层组装实体；`set_tags` 定义化（同名复用 D3，颜色/图标留空，chip 视觉由 TagVariant 映射兜底）。SQLite 注意：排序修饰符须写 `COLLATE NOCASE`（裸 `NOCASE` 是语法错误，曾致读取静默返回空）。
   - UI 层：`NavViewState.tags` 与 `NavConnFilter::passes` / `nav_conn_passes_facets` / 搜索词标签匹配全部换 `tag::Tag`（facet 约束仍是名字字符串）；行内 chip 改「色点 + 名称」，色点 = `nav_tag_color`（名称哈希 → 主题语义色分档，不直渲 hex，对齐 §14 color→TagVariant）；编辑器提交通道保持名字数组（`set_tags` 在 engine 层定义化）；facet 菜单标签集合由 `BTreeSet<String>` 从实体映射提取名字。
   - 测试：新增 engine 迁移测试覆盖「老库补列+归并→实体读取」「同名复用」「软删后同名重建」；nav_view 测试改实体构造。
   - 「管理分组与标签覆盖层」（批量重命名 / 合并分组 / 清理空标签）**未做**，见 §12 待办。
6. **标签覆盖层（管理标签对话框）**：批量重命名 + 清理空标签。**→ ✅ 已完成并验证**（2026-10-21：tag 12 测全绿（含 `reference_counts_aggregate_link_tables`）；database 154 测全绿；workbench lib 84 测 + `ui_contract` 9 测全绿）。入口 = 连接菜单「管理标签…」；对话框见 `crates/workbench/src/components/tag_manager_dialog.rs`（引用数跨库聚合口径见 D8，空标签软删见 D2，重命名关联 id 跟随见 D9）。
7. 每步独立验证提交：`cargo check -p <crate>` + 相关单测 + `ui_contract`。

## 12. 关联待办

| 原型待办 | 状态 | 归口 |
| --- | --- | --- |
| #3 管理分组与标签覆盖层（批量重命名 / 合并分组 / 清理空标签） | ✅ 已完成（2026-10-21：分组部分由其他轨先行完成；标签部分本文 §11 步骤 6——批量重命名 + 清理空标签） | 已实施 |
| #8 标签命名规范 `key:value` | ✅ 已定（本文 §6：标签名约定，不进模型） | — |
| #5 孤儿缓存回收 | 与本方案无关（主线导航会话任务） | — |

## 13. 风险与决策记录

| 编号 | 决策 / 风险 | 说明 |
| --- | --- | --- |
| D1 | 跨库引用走应用层解析 | SQLite 无跨库 join；读侧 `IN` 批量 + 内存合并；两个库间无事务，靠顺序写入 + 读取过滤兜底 |
| D2 | 删除 = 仅软删定义行，不清关联 | id 引用 + 查询过滤使「重建同名带旧归属」不成立；省去跨库级联；与现状 `delete_tag` 语义不同，属结构性收益 |
| D3 | 归档跨域复用全局同名标签 | 避免同名异色；颜色/图标以全局定义为准 |
| D4 | scope 派生原则 | scope 由存储域决定，不可编辑；存量 analytics scope 归一 global 需迁移报告确认 |
| D5 | 0.7.1 Tag 无内置关闭按钮 | 修正讨论纪要 §3.3；删除入口组合 `Button::ghost` 实现 |
| D6 | 导航 chip 色点 = 名称哈希 → 主题语义色分档 | 不直渲 `tag.color` hex（视图零裸色值，rds-ui-spec 硬约束）；同一标签跨连接行同色（哈希稳定）；无定义色也可用（导航旧标签 color 为空）。见 `crates/database/src/nav_view/primitives.rs::nav_tag_color` |
| D7 | 导航 `tags` 定义与关联同库、无迁移文件 | `ensure_tables` 幂等建表 + 补列 + 归并（与 `is_primary` 补列同模式）；tags 表 DDL 与 tag crate 逐列一致，任一方先建都成立；避免版本迁移与幂等演进两套口径漂移 |
| D8 | 覆盖层引用数 = 单库关联表聚合 + 跨库补数 | `TagTable::reference_counts()` 统计**本库**全部已知关联表（`connection_tags` / `scratchpad_tags` / `analytics_resource_tags`，`sqlite_master` 查存在性、动态拼固定表名，无注入面）；global 定义补**项目库**计数（`analytics_resource_tags` 跨库引用 global 定义，D1 应用层解析）；project 定义只数项目库（connection + scratchpad） |
| D9 | 批量重命名 = 改定义名、关联 id 跟随 | 三模块 chip / facet / 归档携带均按 `tag_id` 关联（D2），`rename_tag` 只动 `tags.name` 一行即可全模块一致，无需触碰关联表 |
| 风险 | 存量迁移数据面 | connection_tags 跨库同名合并、analytics scope 归一，均需迁移报告 + 人工确认，宁可少迁 |
| 风险 | 跨库一致性 | 标签定义（全局）与关联（项目）分库，删除/重建路径需单测覆盖过滤语义 |

## 14. 实现位置映射表

| 设计决策 | 代码位置 |
| --- | --- |
| tag crate（模型 + TagStore + 存储实现 + 单测） | `crates/tag/`（新建；依赖 `engine`/`shared`/`paths`，被 database / scratchpad / analytics_resource / workbench 消费） |
| 导航关联改造 + 迁移 | `crates/engine/src/persistence/connection_org_store.rs`（`ensure_tables` :108-171（建 `tags` 表 / 补 `tag_id` 列 / 存量归并）、标签读写与实体化方法 `list_tags` :293 起 / `set_tags` :311 起 / `list_tag_rows`、`NavTagRow` :63-72）；`crates/database/src/nav_store.rs`（`list_all_tags` 实体组装 + scope 派生 D4） |
| 导航 UI（chip / 编辑器 / facet） | `crates/database/src/nav_view.rs`（`NavViewState.tags` :293、`NavConnFilter::passes` :488）；`rows.rs`（chip 色点 + 名称 :1249-1274、facet/tag 匹配）；`primitives.rs`（`nav_tag_color` / `nav_tag_palette_index` :791 起）；`actions.rs`（facet 标签集合 :164）；`editors.rs`（提交仍走名字数组 :251-270）；`tests.rs`（实体构造 :712） |
| 分析资源定义上移 + 瘦身 | `crates/analytics_resource/src/tag.rs`（全文件）、`models.rs:66`（实体）；关联表迁移 SQL 在 `crates/engine/src/migration/` |
| 关联表去 tag 外键（定义上移后跨库无法 FK） | `crates/engine/migrations/project_meta/024_analytics_resource_tags_drop_tag_fk.sql`（重建 `analytics_resource_tags`，保留资源侧 FK） |
| 存量标签迁移（幂等 + 同名重映射） | `crates/analytics_resource/src/tag.rs::ensure_tags_migrated` / `migrate_analytics_tags` / `remap_tag_links`；tag crate `TagDefinitions::insert_migrated` |
| 跨库批量解析（IN 查全局定义） | `crates/tag/src/store.rs::get_tags_by_ids`（TagDefinitions）；`tag.rs::get_tags_for_resource` / `tags_by_resource` 消费 |
| 测试全局库隔离注入 | `crates/analytics_resource/src/lib.rs::new_for_test`（每测试独立 `global.db`，规避共享进程隔离根撞名） |
| 归档请求加 tags | `crates/analytics_resource/src/model.rs`（`ArchiveRequest` :214、`ArchiveBinding` :103） |
| 草稿箱接入（project tags + 关联 + 筛选 + 归档携带） | `crates/scratchpad/src/store.rs`（标签方法）、`models.rs`、`jobs.rs`；归档入口 `crates/analytics_resource/src/commands.rs:19` |
| 草稿箱关联表（无 FK，同库引用） | `crates/engine/migrations/project_meta/025_scratchpad_tags.sql`（entry_id = 模块内相对路径） |
| 草稿箱标签存储（打标/读取/字典） | `crates/scratchpad/src/store.rs`（`list_project_tags` / `create_project_tag` / `tags_of_entry` / `tags_by_entry` / `attach_tag` / `detach_tag`；经 `TagTable::open_project` + `conn()` 同库 JOIN） |
| 草稿箱标签 UI（筛选下拉 / 打标弹窗 / 右键入口） | `crates/scratchpad/src/scratchpad_view/{chrome.rs,actions.rs,rows.rs}`（`tag_filter` / `entry_tags` / `project_tags` 状态；`flatten_scratchpad` 标签平铺分支） |
| 归档继承（草稿标签 → ArchiveRequest.tags） | `crates/analytics_resource/src/dialogs/archive.rs`（`ArchiveDialogSeed.initial_tags` 预填）；`crates/workbench/src/components/resource_host.rs`（单选预填 / 多选直接带） |
| 隔离测试范式（tag crate 单测参考） | `crates/engine/src/persistence/metadata_cache_index.rs`（`isolated()` :366、`thread_local` 路径覆盖 :28） |
| gpui-kit Tag 组件 API 查证 | `vendor/gpui-component/src/tag.rs`（0.7.1 实测，禁止凭记忆） |
| 尺寸 / 颜色常量 | `crates/workbench_shell/src/ui.rs`、`assets/themes/product-tokens.json` |
| 管理标签覆盖层（批量重命名 / 清理空标签） | `crates/workbench/src/components/tag_manager_dialog.rs`（新建：`open_tag_manager_dialog` / `collect_rows`（跨库聚合 D8）/ `open_tag_rename_dialog` / `confirm_cleanup`；按 `TagRow.project` 选库 open_global/open_project）；`crates/workbench/src/components/nav_host.rs`（`open_tag_manager` 实现）；`crates/database/src/nav_host.rs`（`NavHost::open_tag_manager` trait :215 起）+ `nav_view/tests.rs`（mock 补空实现）；`crates/database/src/nav_view/chrome.rs`（菜单「管理标签…」:521 起） |
| 引用数聚合（覆盖层数据源） | `crates/tag/src/store.rs::TagTable::reference_counts()`（:142 起，动态聚合已知关联表；单测 `reference_counts_aggregate_link_tables`） |
| 原型待办表 | `docs/architecture/database/database-navigator-prototype-design.md`（§11 :696） |
