# 图标（微标）：现成到什么程度、品牌标怎么落

> 缘起：审计「数据库类型 / 驱动 / 文件」这类**微标**是否有现成资产可用，以及为什么数据库类型
> 下拉里那些品牌图标（DataGrip 一抓一大把）本仓一个都没有。结论先行：**通用语义图标齐全；
> 品牌标一个没有，且那是授权问题不是工程缺口。**

## 1. 现成资产：gpui-kit 自带 Lucide 全量集

- `gpui-kit-assets 0.6.1` 内嵌 **1830 个 SVG**（`assets/icons/`），由 `gpui_kit::assets::AllAssets`
  提供；应用侧注册一次即可（`crates/app/src/main.rs` 的 `.with_assets(gpui_kit::assets::AllAssets)`）。
- 命名空间是**平铺**的 `icons/<kebab-name>.svg`（`IconName::path()` 的返回值）——自备图标必须
  另开命名空间，否则将来 Lucide 收录同名图标就串了。
- **未注册资产源时图标静默渲染为空**（元素在、看不见）：排查"图标丢了"先看这一行。

### 1.1 数据库对象 → 现成图标（实测存在，可直接用）

| 对象 | 图标 | 对象 | 图标 |
| --- | --- | --- | --- |
| 连接 / 实例 | `server` · `network` | 主键 | `key` · `key-round` |
| 库 / 目录 | `database` | 外键 | `link-2` |
| schema | `folder` · `folder-tree` | 索引 | `hash` |
| 表 | `table` · `table-properties` | 序列 | `list-ordered` |
| 视图 | `eye` | 触发器 | `zap` |
| 列 | `columns-2/3/4` · `table-columns-split` | 例程 / 函数 | `square-function` · `sigma` |
| JSON 列 | `braces` · `file-braces` | BLOB | `binary` |
| 时间列 | `clock` · `calendar-clock` | 备份 / 导出 | `database-backup` · `hard-drive-download` |
| 联邦 / 加速 | `network` · `database-zap` | 文件类型 | `file-spreadsheet`(CSV) · `file-code`(SQL) · `file-braces`(JSON) · `file-chart-*` · `file-diff` |

**结论：通用对象图标不需要自备。** 唯一"照直觉找了但没有"的是 `function-square`——实际名字是
`square-function`。

## 2. 品牌图标：一个都没有（也不该随手加）

实查资产：`mysql` / `postgres` / `sqlite` / `duckdb` / `oracle` / `mongo` / `redis` / `mariadb` /
`sqlserver` **精确匹配 0 个**，名字里带 `sql` 的也是 0。

原因是**授权**而不是遗漏：海豚（MySQL）、大象（PostgreSQL）、鸭子（DuckDB）、红色甲骨文都是
**注册商标**，开源图标集（Lucide 这类）通常不收。关键区分是**版权与商标是两件事**：图标文件的
许可（即便是 CC0 / MIT）只覆盖版权，**不构成商标使用许可**——simple-icons 的 README 就写明
"这些图标是各自所有者的商标，我们不授予商标使用权"。所以"能下载"不等于"能发布"。

### 2.1 那 DataGrip 为什么"每次都能拿到那么多"？

它**不是从某个公共图标包里取的**，是 JetBrains 自己逐个收录 + 法务背书：

1. **资产自维护**：JetBrains 把品牌图标当产品资产逐张维护（产品包里每个品牌一张 SVG），
   每次新增数据源支持就配一张——不是"扫描图标包"的结果。
2. **法律基础是"指明性使用"（nominative use）**：用厂商图标标注"连接到该厂商的产品"属于指代
   对方；多数厂商的 brand / press kit 也明确允许在"集成 / 兼容"场景使用（不得改色变形、
   不得暗示官方背书）。
3. **商业关系**：JetBrains 与多家数据库厂商有合作/联合营销，部分 logo 是协商后使用的。
   **风险是他们承担并管理的**（有法务流程），不是因为图标公开可用。

**对本仓的意思**：这是**产品 / 法务拍板项**（与文档里其它"需产品拍板"同一口径）。三条路：

| 路线 | 成本 | 风险 | 备注 |
| --- | --- | --- | --- |
| ① 自绘中性几何标 | 中（设计工作） | 零授权风险 | 与既有「形状 + 2 字母」口径一致，**推荐** |
| ② 引开源品牌集（simple-icons / devicon） | 低 | 商标条款需逐条核 | 许可只覆盖版权，商标另说 |
| ③ 用厂商 brand kit 官方资源 | 低 | 受各厂商商标政策约束 | 通常要求不得改动 + 注明来源 |

> 本文不是法律意见；②③ 落地前需产品 / 法务确认。

## 3. 落地配方：一张品牌图标 = 三步

工程侧管线已铺好（`crates/workbench_shell/src/db_icons.rs`），拿到授权 SVG 后：

1. **放文件**：`assets/icons/db/<type_id>.svg`（独立命名空间，避开 Lucide 的平铺名）。
2. **登记资产源**：把 `crates/app/src/main.rs` 的 `.with_assets(...)` 换成**组合 AssetSource**——
   先查自备目录，未命中再委托 `gpui_kit::assets::AllAssets::get(path)`。
   注意 `AllAssets::load()` 对未知路径返回 **Err（不是 `Ok(None)`）**，组合器要自己吞掉未命中；
   自备图标只有几张，用 `include_bytes!` + 一张 `match` 表即可，不必给 `app` 加 `rust_embed` 依赖。
3. **改一行映射**：`db_icons::db_icon_of` 里把该类型的 `Catalog(IconName::…)` 换成
   `DbIcon::Brand { type_id: "mysql" }`——路径由 `DbIcon::brand_path` 推导，**调用点不动**。

## 4. 现状与边界

- **尚未接线**：导航 / 对话框的类型徽标今天仍走库里 `data_source_types.icon` 列的 **emoji**
  （MySQL=🐬…）与「形状 + 2 字母」（能力矩阵 §7 #10 记为有意设计）。`db_icons` 当前**只入表不接**，
  接线属各自视图轮次（当时 `crates/database/src/nav_view.rs` 正被并发会话修改）。
- **emoji 仍是数据**：它是类型目录的一列，也是"没有 SVG 时的兜底"，本模块不碰它。
  渲染优先级（品牌标 > 通用标 > emoji > 形状 + 字母）由调用点决定。
- **漂移有守卫**：种子里加/删类型而图标表没跟上 → `every_seed_type_has_an_explicit_icon_row` 红；
  映射指向的图标不在资产里 → `mapped_catalog_icons_resolve_in_the_asset_source` 红
  （渲染成空白是最难查的一类"没报错"）。

## 5. 实现位置映射

| 内容 | 落点 |
| --- | --- |
| 通用图标资产（Lucide 全量 1830 个） | 依赖 `gpui-kit-assets`；注册点 `crates/app/src/main.rs`（`.with_assets(gpui_kit::assets::AllAssets)`） |
| 类型 → 图标映射、品牌标落点、守卫测试 | `crates/workbench_shell/src/db_icons.rs` |
| 类型目录（`id` / `name` / `category` / `icon`） | `data_source_types` 表；种子 `crates/engine/migrations/global/008_add_data_source_module.sql`；读侧 `engine::persistence::{driver_store, driver_catalog}` |
| 徽标渲染现状（emoji） | `crates/workbench/src/components/connection_dialog/helpers.rs::type_badge`；导航侧「形状 + 2 字母」在 `crates/database/src/nav_view.rs` |
| 图标尺寸 / 取色 | `crates/workbench_shell/src/ui.rs`（`ICON_SIZE_SM` / `ICON_SIZE_MD`）+ 主题 token |
