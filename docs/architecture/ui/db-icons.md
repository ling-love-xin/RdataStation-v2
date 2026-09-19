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

## 2.2 DBeaver 为什么也能拿到那么多（与 JetBrains 同构，但多一层「开源」）

DBeaver Community 是 **Apache-2.0** 开源，品牌图标就在各 driver 扩展自己的 `icons/` 目录里，
所以你能看到、也能考据来源。但**开源不等于可自由使用**：

1. **版权 ≠ 商标（关键）**：Apache-2.0 给你的是**版权**许可（可再分发，需保留归属与 NOTICE），
   而 Apache-2.0 **§6 明确写了不授予商号 / 商标 / 服务标记 / 产品名的许可**。所以「从
   Apache-2.0 仓库拷 logo」只解决了版权那一半，商标问题原封不动。
2. **法律基础同 DataGrip：指明性使用**：用厂商图标标注「连到该厂商的产品」属于指代对方
   （不是你的品牌、不暗示背书）；多数厂商的 brand / press kit 明文允许「集成 / 兼容性」场景。
3. **厂商乐于配合**：数据库客户端是厂商最重要的生态入口之一，不少品牌指南直接欢迎你做集成
   标识（代价通常是「不得改色变形、不得单独用于宣传你的产品」）。
4. **真正的护城河是「可审计、可逐张替换」**：DBeaver 那套图标能长期存在，靠的是每张都能追到
   来源与提交记录 —— 出问题能定位、能换掉。**这才是「像 DBeaver 那样」的实质。**
5. 另有一层现实：DBeaver 的驱动图标**可由用户自己配置 / 替换**（driver manager 里能选图标），
   它并未把所有库都硬编成品牌标。

> 本仓现状：只有 MIT `LICENSE`，**仓库里没有任何第三方许可 / 商标声明**（实查），也没有「关于」页。

## 2.3 我们要「像那样」用品牌标：四步

| # | 步骤 | 具体做什么 | 谁做 / 风险 |
| --- | --- | --- | --- |
| 1 | **建来源台账** | 下表逐行填：素材来源 URL、厂商商标政策链接、收录日期、收录人 | 工程（零风险，**先做**） |
| 2 | **逐厂商核 brand kit** | 政策允许指代使用的 → 收；不明确的 → 不收（**不要凭「别人也这么干」收**） | 产品 / 法务（中风险，逐条核） |
| 3 | **加第三方商标声明** | 仓库 `NOTICE` 或设置页「关于」一段：「各数据库名称与图标为其各自所有者的商标；本产品使用它们仅为指代所连接的产品，不代表任何厂商赞助或背书」 | 产品 / 法务（零风险） |
| 4 | **（可选，最优雅）品牌图标走运行时资源包** | 默认发中性标；用户 / 企业把品牌 SVG 放进资源目录（`RDS_HOME/icons/db/<type_id>.svg`），AssetSource 先查磁盘再委派内置 | 工程（一次小改动，见 §3） |

**为什么第 4 步值一提**：它把「是否用品牌标」变成**配置**而不是发布内容——仓库与安装包
不含第三方商标素材，风险下沉到使用者，体验上却与内置无异（丢文件即生效，不用重编译）。

### 来源台账（模板，逐行核完再填）

| type_id | 厂商 / 产品 | 素材来源 | 商标 / 品牌政策 | 结论 | 收录日期 |
| --- | --- | --- | --- | --- | --- |
| `mysql` | Oracle | 待填（官方 brand/resource 页） | Oracle 商标政策（历史上最严的一档，通常要求用官方素材且不得改动） | ⏳ 待核 | |
| `mariadb` | MariaDB Foundation | 待填 | MariaDB 商标政策 | ⏳ 待核 | |
| `postgresql` | PostgreSQL（PGEU/PGCA） | 待填 | PostgreSQL 商标政策 | ⏳ 待核 | |
| `oracle` | Oracle | 待填 | 同 MySQL 一栏 | ⏳ 待核 | |
| `mssql` | Microsoft | 待填 | Microsoft 商标 / 品牌指南（对「指代」场景有专门条款） | ⏳ 待核 | |
| `sqlite` | SQLite（Hwaci） | 待填 | SQLite 官网对 logo 的声明 | ⏳ 待核 | |
| `duckdb` | DuckDB Foundation | 待填 | DuckDB 品牌素材页 | ⏳ 待核 | |
| `clickhouse` | ClickHouse Inc. | 待填 | ClickHouse 品牌指南 | ⏳ 待核 | |
| `mongodb` | MongoDB Inc. | 待填 | MongoDB 品牌指南 | ⏳ 待核 | |
| `redis` | Redis Ltd. | 待填 | Redis 商标指南 | ⏳ 待核 | |

> 上表「厂商 / 政策」是**已知存在、需逐个点开核**的入口名，不是核实过的结论；
> 填表时请把政策原文链接与关键条款摘一句进来。
>
> **不建议整包拷贝参考产品的图标目录**：版权上可行（保留归属），但它并不能解决商标问题，
> 还会引入一堆你不支持的数据库图标 —— 维护与合规双重负担。

### 又一个能带品牌标的项目：`t8y2/dbx`（实查）

[Rust + Tauri 的跨平台客户端](https://github.com/t8y2/dbx)，同样带 90+ 家数据库 logo。实查结论：

- 图标是**前端静态资源**：`apps/desktop/public/icons/database/*.svg|png|webp`（含 `dm.svg` 达梦、
  `apache_kylin.svg`），另有 `icons/ai/`；
- 仓库里**没有**针对这些 logo 的归属 / 许可 / 声明文件 —— 那两个目录只有图片，全仓的
  `LICENSE` / `NOTICE` 只覆盖代码、vendored Rust crate 与一个字体（Geist）；
- 所以他们「能」用：不是因为拿到了授权（也没有披露），而是这类「指代所连接产品」的用法
  通常落在容忍区，且他们选择不披露。**风险的真实形态是「被要求停用 / 换标」**（厂商先发函，
  应用商店收到投诉会下架），不是跑不起来。
- 旁证（同一次实查）：simple-icons 里 **`oracle` / `sqlserver` / `microsoftsqlserver` 已不存在
  （HTTP 404）**，而 `mysql` / `postgresql` / `mariadb` / `sqlite` / `duckdb` / `clickhouse` /
  `mongodb` / `redis` / `databricks` / `snowflake` 都在 —— 连 CC0 图标集也会被要求撤下，
  说明商标确实有人管，而不是「大家都放着所以没事」。

## 3. 落地配方：一张品牌图标 = 三步

工程侧管线已铺好（映射 `crates/workbench_shell/src/db_icons.rs` + 资产源
`crates/app/src/assets.rs`），拿到 SVG 后：

1. **放文件**：品牌图标放**运行时目录** `<RDS_HOME>/icons/db/<type_id>.svg`
   （默认可执行文件所在目录，开发期是 `<repo>/.rds/icons/db/`）。
   —— 这是 §2.3 第 4 步的实现：**仓库与安装包不含第三方素材**，不重编译，重启应用即生效。
2. **资产源（已实现）**：`crates/app/src/main.rs` 注册的是 `assets::AppAssets`（`crates/app/src/assets.rs`）——
   先查运行时品牌包、未命中再委派 `gpui_kit::assets::AllAssets`。三条行为约定：
   - **未命中不静默**：品牌包缺这张图时交给内置源，内置也没有就 **Err**（名字写错能在日志里看见）；
   - **不在渲染路径读盘**：按路径**只读一次**并缓存（含“不存在”的结论）——代价是换图要重启应用；
   - **颜色跟主题**：加载时给「根标签既无 `fill` 也无 `stroke`」的单色 SVG 补 `fill="currentColor"`
     （gpui 用元素的 `text_color` 解析 `currentColor`，见 `elements/svg.rs`）。
     simple-icons 这类 fill 路径图**直接能用**；Lucide 风格（`stroke="currentColor"`）与
     自带配色的彩色品牌标**一律不动**。
3. **改一行映射**：`db_icons::db_icon_of` 里把该类型的 `Catalog(IconName::…)` 换成
   `DbIcon::Brand { type_id: "mysql" }`——路径由 `DbIcon::brand_path` 推导，**调用点不动**。

**来源建议**：优先用 [simple-icons](https://github.com/simple-icons/simple-icons)（CC0，单色路径，
写明了版权许可；但注意 CC0 **不授予商标许可**）。实测可用的 slug：`mysql` / `postgresql` /
`mariadb` / `sqlite` / `duckdb` / `clickhouse` / `mongodb` / `redis` / `databricks` / `snowflake`；
**`oracle` / `sqlserver` / `microsoftsqlserver` 不在里面（已 404）**——那两家要另外找（厂商 brand kit
或自绘）。彩色品牌标（DataGrip / dbx 那种）要走各厂商 brand kit，并记得先把 `fill` 做进根标签或
保住原色（本层的补 `fill` 规则只作用于“无 fill / 无 stroke”的单色图）。

## 4. 现状与边界

- **尚未接线**：导航 / 对话框的类型徽标今天仍走库里 `data_source_types.icon` 列的 **emoji**
  （MySQL=🐬…）与「形状 + 2 字母」（能力矩阵 §7 #10 记为有意设计）。`db_icons` 当前**只入表不接**
  （所有类型都还是 `Catalog(..)`），接线属各自视图轮次。
- **emoji 仍是数据**：它是类型目录的一列，也是"没有 SVG 时的兜底"，本模块不碰它。
  渲染优先级（品牌标 > 通用标 > emoji > 形状 + 字母）由调用点决定。
- **漂移有守卫**：种子里加/删类型而图标表没跟上 → `every_seed_type_has_an_explicit_icon_row` 红；
  映射指向的图标不在资产里 → `mapped_catalog_icons_resolve_in_the_asset_source` 红
  （渲染成空白是最难查的一类"没报错"）。

## 5. 实现位置映射

| 内容 | 落点 |
| --- | --- |
| 通用图标资产（Lucide 全量 1830 个） | 依赖 `gpui-kit-assets`；由 `assets::AppAssets` 委派（不在 `main.rs` 直接注册 `AllAssets`） |
| **资产源（内置 + 运行时品牌包）** | `crates/app/src/assets.rs`（`AppAssets`：品牌包 → 内置；路径解析、缓存、`currentColor` 归一化与 6 条单测）；注册点 `crates/app/src/main.rs`（`.with_assets(assets::AppAssets)`） |
| 品牌包目录 | `<RDS_HOME>/icons/db/<type_id>.svg`（`paths::home()` 解析，见 `docs/architecture/runtime/data-paths.md`） |
| 类型 → 图标映射、品牌标落点、守卫测试 | `crates/workbench_shell/src/db_icons.rs` |
| 类型目录（`id` / `name` / `category` / `icon`） | `data_source_types` 表；种子 `crates/engine/migrations/global/008_add_data_source_module.sql`；读侧 `engine::persistence::{driver_store, driver_catalog}` |
| 徽标渲染现状（emoji） | `crates/workbench/src/components/connection_dialog/helpers.rs::type_badge`；导航侧「形状 + 2 字母」在 `crates/database/src/nav_view.rs` |
| 图标尺寸 / 取色 | `crates/workbench_shell/src/ui.rs`（`ICON_SIZE_SM` / `ICON_SIZE_MD`）+ 主题 token |
