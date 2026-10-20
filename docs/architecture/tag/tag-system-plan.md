# 标签系统统一方案（讨论纪要 · 交接文档）

> 2026-10-20 于「数据库导航栏模块」会话中讨论产出；本会话主线是导航栏，标签系统跨三个模块，**另开会话继续**。
> 本文是讨论结论的完整交接：动机 / 现状实证 / 已定方向 / 待定口径 / 执行路径 / 关联代码位置。新会话先读本文，再补两张现状图（下文待定项）即可动手。

## 1. 背景与动机

- 项目内已有**两套分叉的标签实现**：导航连接标签（字符串集合）与分析资源标签（实体，带 color/icon/scope），模型、存储、语义各不相同，长期会继续分叉。
- 讨论中发现**第三个消费方**：草稿箱（文件树组织，无标签概念），且存在「草稿 → 归档 → 资产库」的天然链路，标签应随归档继承。
- 结论：标签应是**产品级统一能力**（独立 crate），三个模块共同消费；导航侧保持轻量，分析资源做重，草稿箱轻接入。

## 2. 消费方与现状（全部实证，2026-10-20 核实）

| 消费方 | 代码位置 | 组织方式 | 标签现状 | 接入深度 |
| --- | --- | --- | --- | --- |
| 导航连接 | `crates/database/src/nav_store.rs:117 list_tags`；`crates/engine/src/persistence/connection_org_store.rs`（`connection_tags` 表，权威检索表，DataSourceService 同步） | 分组树 | 纯字符串集合（`connection_tags(connection_id, tag)` 联合主键） | 现状轻量；UI 自绘 chip + 逗号分隔输入（`nav_view/chrome.rs:99-116` 标签编辑器、`D3 tag_editor_tags` 预读） |
| 分析资源 | `crates/analytics_resource/src/tag.rs`（`AnalyticsTag { name, color, icon, scope, deleted_at }`，同名拒绝、`(name,scope)` 部分唯一、软删） | 资源列表（`project.db`） | **实体标签**（比导航丰富） | 做重：列表筛选 / 详情打标 / 批量打标 |
| 草稿箱 | `crates/scratchpad/`（`rds-scratchpad`，`ScratchpadStore` 文件系统驱动，`jobs.rs` 文件操作；归档链路 `ArchiveRequest` → analytics_resource，`promoted_from: "scratchpad/…"`） | 文件树 | **无标签概念** | 轻接入：列表标签筛选 + 归档继承标签 |

> 导航标签 UI 现状：`nav_view/chrome.rs` 标签编辑器是自绘 `InputState`（逗号分隔、Enter 提交），标签未采用 gpui-kit 组件。gpui-kit 0.7.1 有 `tag` / `badge` 组件（vendor `gpui-component/src/lib.rs` 实测 `pub mod tag; pub mod badge;`）。

## 3. 已定方向（讨论共识）

1. **独立 tag crate**（不放进 engine）：项目已有抽取先例（`rds-property`、`rds-connection`、`workbench_shell`）；依赖方向 `tag → engine/shared`，被 database / scratchpad / analytics_resource / workbench 消费，无环，符合「依赖只向下」。
2. **共享程度 = B 方案为主，存储争取统一**：
   - A（只共享 UI 层）：不需要 crate，已否决。
   - B（共享模型 + CRUD 契约，存储各自实现）：`TagStore` trait（create/rename/delete/list + attach/detach + 同名约束 + 空标签清理），各模块 store 实现。**保底方案**。
   - C（共享同一张表 + 多态引用）：侵入最大，现在不做。
   - **倾向**：既然做共享层，存储也统一（一张通用标签表，三方共用），重命名 / 同名约束 / 空标签清理一次写对。具体见待定项 3。
3. **标签 UI 换 gpui-kit 组件**：行内展示用 `tag::Tag`（自带关闭 ×、色彩档位、a11y）；编辑器从「逗号分隔文本输入」升级为「已加 chip 集合 + 追加输入框」。动手前先读 vendor `tag.rs` 确认 API。
4. **增删改查建议**：
   - 增：行内 `+`（已有）弹 chip 编辑器；facet 弹层加「新建标签」入口。
   - 删：chip 的 `×`（gpui-kit Tag 原生）；删除 = 从所有关联对象移除（级联）。
   - 改：**全局重命名**（一个标签改名，所有带它的对象同步）——归入「管理分组与标签覆盖层」待办。
   - 查：facet 筛选（导航已有）；补「空标签清理」入口。
5. **顺序建议**：行内 chip 按**添加顺序稳定**（增删不重排）；facet 弹层标签列表按**引用数降序 → 名称**；不提供标签手动排序（扁平集合，价值低）。
6. **`key:value` 规范（§11 #8）**：建议做（`env:prod` 显式维度在 facet 里可精确匹配），与现有 `type:/driver:/scope:` 隐式 facet（规则 2）互补。
7. **三方定位**：分析资源做重（实体元数据）、草稿箱轻接入（筛选 + 归档带标）、导航维持现状（连接级组织）。
8. **UI 视觉统一**：统一用 gpui-kit Tag 组件 + 主题 token 取色（`cx.theme().colors.*`，禁止裸色值）。

## 4. 待定口径（新会话需先对齐，否则 crate 会变成空壳）

1. **实体字段规格**：color / icon / scope 是全模块共用还是仅分析资源？导航的字符串标签是否升格为实体标签（涉及**存量数据迁移**：`connection_tags` 现有数据如何映射 color/icon/scope）。
2. **`key:value` 是否纳入模型**（解析与存储形态：独立字段 vs 标签名约定）。
3. **草稿箱标签存储载体**：
   - ① project.db 加一张通用标签表（tag crate 统一管，三方共用存储）——推荐；
   - ② scratchpad 目录下隐藏元数据文件（跟文件走，跨项目不通用）。
4. **归档继承链路**：`ArchiveRequest`（model.rs:209，目前只收路径 + 元数据）需加 tags 字段；草稿打标 → 归档 → 资产库标签继承。
5. **软删口径**：对齐 analytics 现有软删（`deleted_at`）还是统一硬删 + 回收站。

## 5. 执行路径（建议顺序）

1. 对齐「模型统一规格」（本节 4 项口径）→ 产出正式规格文档（替代本文）。
2. 搭 tag crate 骨架：实体模型 + `TagStore` trait + 存储实现 + 同名约束 / 空标签清理 / 重命名（含单测，仿 `metadata_cache_index.rs` 的隔离测试模式）。
3. 分析资源接入做重：列表标签筛选 / 详情打标 / 批量打标（UI 换 gpui-kit Tag）。
4. 草稿箱轻接入：标签筛选 + 归档继承。
5. 导航 UI 调整：行内 chip / 标签编辑器换 gpui-kit Tag；facet 弹层标签列表排序（引用数降序）；「管理分组与标签覆盖层」（批量重命名 / 合并分组 / 清理空标签）并入。
6. 每步独立验证提交（`cargo check -p <crate>` + 相关单测 + `ui_contract`）。

## 6. 关联的原型待办（database-navigator-prototype-design.md §11）

- #3 管理分组与标签覆盖层（批量重命名 / 合并分组 / 清理空标签）——⬜ 待做（分组多时刚需）→ 本方案步骤 5
- #8 标签命名规范 `key:value` ——⬜ 待定（影响 `tag:` 解析）→ 本方案待定项 2
- #5 孤儿缓存回收 —— 与本方案无关，主线导航会话任务 #8（方案已定，待实现）

## 7. 关键代码位置索引

- 导航标签：`crates/engine/src/persistence/connection_org_store.rs`（表 :99、list_tags :189）；`crates/database/src/nav_store.rs:117`；UI `crates/database/src/nav_view/chrome.rs:99-116`（标签编辑器）、`:494`（chips 渲染）、`actions.rs`（facet 持久化）
- 分析资源标签：`crates/analytics_resource/src/tag.rs`（全文件）；归档请求模型 `crates/analytics_resource/src/model.rs:209`
- 草稿箱：`crates/scratchpad/src/store.rs`、`jobs.rs`、`models.rs`；归档链路入口 `crates/analytics_resource/src/commands.rs:19`
- 索引层隔离测试范式（tag crate 单测参考）：`crates/engine/src/persistence/metadata_cache_index.rs`（`isolated()` 线程局部路径覆盖）
- gpui-kit 组件查证：`vendor/gpui-component/src/lib.rs`（`pub mod tag; pub mod badge;`）——写组件前必须读对应模块源码，禁止凭记忆
- 原型文档：`docs/architecture/database/database-navigator-prototype-design.md`（§11 待办表 :696、§1.1 规则 2、§6.3）
