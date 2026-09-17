# docs/architecture 导航

> 架构设计决策统一存放于此层（**方案 A**：按主题细分目录），实现说明随代码（注释 + PR + 各文档尾部映射表），crate 目录不承担文档仓库职责。

## 目录

| 文档 | 主题 | 关键内容 |
| --- | --- | --- |
| `overview.md` | 架构总览 | 三层架构 / 双层数据 / 双引擎 / crate 依赖方向 |
| `crate-ownership-proposal.html` | crate 归属 | app / workbench / settings / Feature / shared / 资产 归属示意 |
| `layout/layout-proposal.html` | 布局示意（v5） | 五段布局最终共识的可视化示意 |
| `layout/layout-design.md` | 布局方案 | 五段结构 / 三模式交互 / Quick Open / 改动清单 / 实施步骤 |
| `theme/theme-preview.html` | 配色预览 | 明暗色卡对比 |
| `theme/theme-design.md` | 主题方案 | 明暗 token / 产品语义角色 / 落地方式 |
| `theme/ui-constraints.md` | UI 约束规范 | 字体阶梯 / 图标三档 / 间距阶梯 / 控件规格 / 区域固定尺寸 / 按钮与自绘控件 / 交互态 / 检查清单与迁移计划 |
| `settings/settings-prototype-design.md` | 设置页**原型设计** | 形态选型（两栏弹层 vs 单列 / 独立窗口 / 侧栏）/ 页面解剖 / 行规格（四形态 · 生效方式标注）/ 搜索行 / **第一版内容清单（只收有生产者的项）与不设节的理由** / 主题映射 / 5 个新增尺寸常量 / GPUI 落点 / 状态矩阵 / 与 V1 对照 / 落地项 8 条 |
| `settings/settings-architecture.md` | 设置**准入裁决书**（设计理念与架构） | 裁决摘要 D1–D12 / 定位与边界 / **三分法作用域（应用级·项目级·会话态）** / 概念模型（设置项解剖 · 登记≠上页）/ 分层与归属（其他 Feature 不直接依赖 settings）/ 单一权威与读写路径 / 六条数据流 / **设置项登记表（权威）** / **准入五条 + 退役清单（7 项无生产者字段）** / 持久化与迁移 / 降级矩阵 / **插件 beta3 预留** / 测试策略（含登记表一致性契约测试）/ 实现位置映射 / K1–K9 / Q1–Q6 |
| `settings/settings-crate-design.md` | settings crate（**沿革**） | 动机 / 结构 / model / 持久化 / 迁移清单；§2–§3 已由 `settings-architecture.md` 取代 |
| `settings/settings-prototype.html` | 设置页**交互稿** | RDS Light/Dark 双主题；5 场景可切（默认 / 搜索有结果 / 搜索无结果 / 已修改 / 深色）；分段·开关·搜索·恢复默认可交互（**示意稿，非权威**） |
| `settings/settings-dev-plan.md` | 设置**开发方案** | 进度记录（文档 + 代码侧登记表 + 僵尸项裁撤已落地）/ 现状盘点 / P0–P4 任务表 / T1–T12 测试场景 / R1–R6 风险 / 验证命令 / 实现位置映射 / 明确不做 |
| `project/README.md` | 项目管理**模块入口** | **先读这个**：一句话定位 / 文档索引 / 特点速览（一实例一项目 · 名册与元数据分离 · 两道护栏 · 视图归属）/ 代码落点 / 状态与范围外 |
| `project/project-prototype-design.md` | 项目管理原型 | 一实例一项目 / CRUD（增删改查）/ 选择器（最近·全部·已移除）/ 项目菜单 / 新建对话框 / 项目设置 / 未保存拦截与项目锁逃生口 / 固定排序持久化 |
| `project/project-prototype.html` | 项目管理原型（交互稿） | RDS Light/Dark 双主题：选择器、项目菜单、新建/设置/CRUD/拦截与锁弹层（可切换、可切主题） |
| `project/project-dev-plan.md` | 项目管理开发方案 | P0/A/B/C 任务（CRUD 主线 + 名册迁移 019）/ 文件落点 / 测试场景 / 风险 / 映射 |
| `project/project-view-architecture.md` | 项目视图架构（A3） | 宿主桥 `ProjectUiHost` 契约与状态所有权 / 对话框栈语义 / 窗口测试方案（含 `#[test]` 宏遮蔽坑）/ 实现映射 |
| `project/project-user-guide.md` | 项目管理**使用手册** | 入口 / 界面导览（选择器·卡片菜单·设置）/ 典型流程（新建含目录选择·空目录询问·拦截·重定位·删除找回）/ 状态速览 / 快捷键 / 数据与安全 / FAQ / 验收清单 |
| `connection/README.md` | 连接模块**入口** | **先读这个**：特点提炼（产品行为 / 数据与安全 / 架构约束 / 工程与文档）/ 边界 / 代码地图 / 改前必守约束 / 测试与验证命令 / 文档地图 / 下一步 |
| `connection/connection-prototype-design.md` | 连接模块原型 | 新增连接对话框布局（v5 对齐）/ 五 Tab / 交互 / 主题映射 |
| `connection/connection-dialog-prototype.html` | 连接模块原型（交互稿） | RDS Light/Dark 双主题新增连接对话框示意（可交互） |
| `connection/connection-dev-plan.md` | 连接模块开发方案 | Phase A/B/C 任务 / 文件落点 / 测试场景 / 风险 |
| `ui/ui-design-spec.md` | UI 设计规范 | 三层约束（主题 token / 尺寸常量 / 组件规格）、rem 基准常量表 |
| `connection/connection-dialog-architecture.md` | 连接模块架构与数据流 | 分层与依赖 / 概念模型（数据库类型 vs 驱动实现、作用域、三类引用、Secret、暂存草稿）/ 状态所有权与对话框层挂载 / 五条数据流（打开·保存·暂存状态机·运行时连接）/ 快捷键 / 决策取舍 / 测试策略 / 数据字典与迁移 / 降级矩阵 / 性能·可观测·安全 / 成熟度评估 / **已知问题与后续项（§14，权威待办清单）** / **数据来源审计（§15，零 UI 造数据）** |
| `connection/connection-user-guide.md` | 连接模块使用指南 | 入口 / 界面导览 / 典型流程（新建·编辑·连续编辑·标签分组）/ Tab 速览 / 快捷键 / 草稿与安全 / FAQ 排查 / USIT 验收清单 |
| `database/README.md` | 数据源管理 / 数据库导航**模块入口** | **先读这个**：特点提炼（产品行为 / 数据与缓存 / 架构约束 / 工程与文档）/ 边界 / 代码地图 / 改前必守约束 / 测试与验证命令 / 文档地图 / 下一步 |
| `database/database-navigator-architecture.md` | 数据源管理 / 数据库导航**架构与设计理念** | 定位与边界 / **设计理念（心智模型 + 三条边界规则 + 概念定位表 + 视觉通道预算）** / 分层与 render 零 I/O / 概念模型与数据 / 五条数据流 / 决策取舍 / 降级容错 / 性能可观测 / 测试策略 / 实现映射 / **已知问题与后续项** |
| `database/database-navigator-prototype-design.md` | 数据源管理 / 数据库导航原型 | 面板布局与**连接行解剖（v6/v7 降密）** / 双通道徽标 / 归属域列 / **facet 筛选（类型 / 驱动 / 标签）与搜索语法** / 分组头 / 树模型 / 三级元数据管线 / 交互 / 主题映射 / 落点 |
| `database/database-navigator-prototype.html` | 数据源管理 / 数据库导航原型（交互稿） | RDS Light/Dark 双主题面板示意（双通道徽标 / 加载 / 搜索 / **可点「筛选 ▾」弹层** / 右键菜单 / 空态可切；含 **v5 → v7 密度对比**） |
| `database/database-nav-dev-plan.md` | 数据库导航开发方案 | Phase A/B/C + **v6/v7 任务（V1–V10，逐项状态）** / 迁移与表 / 测试场景 / 风险 / 验证 / 映射 |
| `database/database-navigator-user-guide.md` | 数据源管理 / 数据库导航**使用手册** | 入口 / 界面导览（**连接行怎么读 · 状态色 · 类型形状**）/ 典型流程 / 分组与标签分工 / 快捷键 / 显示开关 / FAQ 排查 / 验收清单 |
| `scratchpad/README.md` | 草稿箱**模块入口** | **先读这个**：一句话定位（项目私有的临时探索工作区）/ 特点提炼（产品行为 · 语义与数据 · 架构约束 · 工程与文档）/ 边界 / 代码地图 / 改前必守 10 条 / 测试与验证命令 / 文档地图 / 下一步 |
| `scratchpad/scratchpad-prototype-design.md` | 草稿箱原型 | 根 = 项目目录 / 240px 左 Dock 面板 / 树与交互 / 主题映射 |
| `scratchpad/scratchpad-prototype.html` | 草稿箱原型（交互稿） | RDS Light/Dark 项目工作区示意（可交互） |
| `scratchpad/scratchpad-architecture.md` | 草稿箱**设计理念与架构** | 裁决摘要 9 条 / 定位与三段式（M4 看数据 · M5 干活 · M6 留证据）/ 概念模型与四条不变式 / 存储布局（内容与内部态分离 · 迁移）/ 项目级回收站 / 路径安全与只读 / 十条数据流（列表 · 虚拟化 · 新建落点 · 移动复制 · 删除撤销 · 导入 vs 引用 · 搜索 · 替换 · 缓存刷新 · 键盘）/ 分层与状态所有权 / D1–D13 决策表 / 降级矩阵 / 性能 / 测试策略 / 实现映射 / **§13 已知问题（权威）** |
| `scratchpad/scratchpad-dev-plan.md` | 草稿箱开发方案 | P0 项目会话 + Phase A/B/C/D / 文件落点 / 测试场景 / 风险 / 逐轮进度记录 |
| `scratchpad/scratchpad-user-guide.md` | 草稿箱**使用手册** | 能力与期次标注 / 入口 / 界面导览（**一行怎么读 · 分组分工**）/ 典型流程（新建模板 · 导入 · 引用与重定位 · 移动复制多选 · 删除与撤销 · 搜索 · 替换 · 键盘导航）/ 一次操作落到哪 / 快捷键 / 只读与数据安全 / FAQ 排查 / USIT 验收清单 |
| `editor/README.md` | SQL 编辑器**模块入口** | **先读这个**：一句话定位 / 特点速览（一内核三档能力 · 模式判定与切换代价 · 只读两维度 · 执行入口唯一 · 结果单权威 · 单元与会话）/ 边界 / 代码地图 / 改前必守约束 / 测试命令 / 文档地图 / 待拍板项 |
| `editor/editor-prototype-design.md` | SQL 编辑器原型 | 三模式对照与判定规则 / SQL 模式解剖（工具栏 · 编辑区 · 结果区 · 状态栏）/ 文本模式 / 分析模式（单元解剖 · 输出类型）/ 交互与快捷键 / 状态矩阵 / 主题映射与尺寸常量 / 与 V1 的逐项对照 |
| `editor/editor-prototype.html` | SQL 编辑器原型（交互稿） | RDS Light/Dark 双主题 × 三模式；可执行（含执行中/中断、结果集上限淘汰、快速过滤、分隔条拖拽、单元运行与 stale、单元增删排序） |
| `editor/editor-architecture.md` | SQL 编辑器**设计理念与架构** | 概念模型与不变式 / 分层与 crate 归属 / 状态所有权 / 八条数据流 / D1–D20 决策表 / **§7 现状与四个假底座** / 降级矩阵 / 测试策略 / 实现映射 / **§12 已知问题（权威）** / §13 待确认 |
| `editor/editor-dev-plan.md` | SQL 编辑器开发方案 | Phase 0（地基与技术验证）/ 1a（内核 + 文本/SQL 编辑体验 + 最小执行）/ 1b（执行闭环）/ 1c（分析模式骨架）/ 任务表与验收 / 测试场景 29 条 / 风险 / 验证命令 |
| `insight/README.md` | 洞察模块（M8）**入口** | **先读这个**：一句话定位（把数据变成结论）/ 特点提炼（一次分析 = 一个目标 + 一份结论 · 规则三层作用域 · 校验失败不连坐 · 采样必须明示 · 视图随 crate（D21 = 方案 A）· **分析中间表一律走 `duckdb::analysis`（D50）**）/ 边界 / 代码地图 / 改前必守约束 13 条 / 测试命令与 202 项基线 / 文档地图 / 下一步 |
| `insight/insight-prototype-design.md` | 洞察模块原型 | 核心语义（一次分析 = 一个目标 + 一份结论）与规则三层作用域 / 280px 右 Dock 面板布局（五 Tab 宽度核算）/ 目标视图分派（列画像 · 表探查 · 多列 · Schema · **快照历史与版本对比（含两批实现期修正）**）/ 状态与空态矩阵 / **规则管理对话框（可禁用内置规则 + 校验错误可见 + 全局层目录首次写入时创建）** / 主题映射与尺寸常量 / GPUI 落点 / **§10 与 V1 的逐项对照（照搬 / 重接 / 重设计 / 不迁）** |
| `insight/insight-prototype.html` | 洞察模块原型（交互稿） | RDS Light/Dark 双主题右 Dock 洞察面板示意（五 Tab 可点切换 / 折叠区 / 质量色阶 / 规则对话框含禁用与校验失败 / 空态·骨架·过期错误·类型未识别） |
| `insight/insight-architecture.md` | 洞察模块**设计理念与架构** | 定位与边界 / 概念模型与**八条不变式** / 分层与 crate 归属（含四处归属偏差）/ **状态所有权表（单一写入者）** / 六条数据流（装配 · 索引同步 · 列画像 · 快照版本链（含面板侧的保存 / 读取 / 对比 / 清理）· 表级评估 · 目录监听）/ **D1–D50 决策表** / 并发与资源 / 降级矩阵 / 测试策略与三条测试纪律 / 实现位置映射 / **§11 已知问题 K1–K16（权威：含无 SQL 沙箱 · 重复规则目录 · 残留规则 K3/K15 · 清理剪断版本链 · 临时表回收 K16（洞察侧已修，结果集侧未接））** / §12 待确认 Q1–Q7（Q6/Q7 已定） |
| `insight/insight-user-guide.md` | 洞察模块**使用手册** | 入口（四个）/ 界面导览与怎么看数字（类型徽标 · 质量色阶 · 采样口径 · 临时表口径）/ 典型流程六条（含**规则管理对话框与它的两处行为**、**多列分析的列序语义**、**快照历史 / 版本对比 / 清理旧快照**）/ **§4 规则编写指南（对外契约）**：三层作用域 · 字段全表 · `applies_to` 与 `value_type` 取值表 · 质量门控判定语义与两个已知坑 · 两份可照抄示例 · 参数替换与**安全边界（如实说明无沙箱）** · **§4.8 内置规则 16 条一览** / FAQ 排查 / USIT 验收清单 |
| `insight/insight-dev-plan.md` | 洞察模块开发方案 | 已确认决策 9 项 / **§0 进度记录（Phase 0 两批 + Phase 1 六批 + Phase 2 两批 + Phase 3 三批 + Phase 4 一批 + Phase 5 三批 + 规则校验补强 + 死副本与残留规则收口 + 临时表一致化（K16 ①+②））** / 现状盘点 / **§2 五项实证缺陷** / **§3 目标 crate 边界（4 类文件归位）** / **§4 规则三层作用域与索引表设计** / Phase 0–5 任务与落点 / 测试场景 T1–T14 / 风险 R1–R7 / 实现位置映射 / 验证命令 |
| `analytics_resource/README.md` | 资产库 / 分析存档**模块入口** | **先读这个**：一句话定位（我留下了什么，它当时长什么样）/ 特点提炼（归档凭证三件套 · 复现强度可见 · 只读常态 · 版本只追加 · 异常可处理）/ 边界 / 代码地图 / 改前必守 12 条 / 测试命令与 15 项基线 / 文档地图 / 下一步 |
| `analytics_resource/analytics-resource-architecture.md` | 资产库 / 分析存档**设计理念与架构**（本文是语义裁决书） | 裁决摘要 D1–D12 / 定位与三段式（M4 看数据 · M5 干活 · M6 留证据）/ 三种 kind 与复现强度 / 归档凭证三件套 / 存储布局与真相源规则 / 对 v1 `007` 的处置（增 9 列 + `scope` 派生 + `config` 降级）/ 版本语义（内容指纹 + 历史内容保留）/ 归档取回契约（顺序与回滚）/ 回收站统一与索引修复 / 分层与接线缺口 / 降级矩阵 / 已知问题 19 条 |
| `analytics_resource/analytics-resource-prototype-design.md` | 资产库 / 分析存档原型 | 面板解剖（头 / 工具栏 / **行解剖与字段优先级** / 单层分组 / 状态行）/ 详情属性面板（按 kind 分派）/ 五条核心交互（归档 · 取回 · 版本 · 回收站 · 索引修复）/ 状态与空态矩阵 / 主题映射（视觉通道预算）/ 尺寸常量与对话框宽度 / GPUI 落点 / **§10 与 V1 的逐项对照** |
| `analytics_resource/analytics-resource-prototype.html` | 资产库 / 分析存档原型（交互稿） | RDS Light/Dark 双主题；12 场景可切（默认 · 归档 · 取回 · 版本历史 · 回收站 · 索引修复 · 空库 · 无结果 · 异常态 · 筛选排序 · 右键菜单 · 只读被拒） |
| `analytics_resource/analytics-resource-dev-plan.md` | 资产库 / 分析存档开发方案 | 已确认决策 / 现状盘点（store 层 55% 可用 · `recycle.rs` 作废 · 四处占位 · 接线缺口 3 项）/ **Phase 0 地基（含 engine 连接池修复）** / Phase 1–5 任务与落点 / §8 明确不做 / T1–T16 测试场景 / R1–R9 风险 / 验证命令 / 实现位置映射 |
| `analytics_resource/analytics-resource-user-guide.md` | 资产库 / 分析存档**使用手册** | 能力与期次标注 / 入口 / 界面导览（**行怎么读 · 复现强度三档 · 状态行 · 详情面板**）/ 典型流程（归档 · 取回 · **归档→取回→再归档核心循环** · 重命名打标签归组 · 版本 · 回收站 · 索引修复）/ 一次操作落到哪 / 快捷键 / 只读与安全（三重守卫 · 指纹 · 手工改动三态 · 备份）/ FAQ 排查 14 条 / USIT 验收清单 / 相关文档 |
| `mock/README.md` | Mock 数据生成（M7）**入口** | **先读这个**：一句话定位（把**表结构**变成可用的测试数据：命名新表 + 组织列 → 生成到临时表 → 显式出口）/ 特点速览（生成≠写入 · 元数据驱动 · 只进分析引擎 · 确定性可复现 · 列名智能映射 · 四出口 · 方案①两处排版 · 视图随 crate）/ 边界与相邻模块关系 / 代码地图（含自持视图与宿主桥）/ 改前必守 10 条 / 测试命令与 99+26+10 项基线 / 文档地图 / 优化建议清单 |
| `mock/mock-prototype-design.md` | Mock 数据生成原型 | 落位与尺寸（右 Dock 280px + 中央「Mock 数据」tab）/ **为什么拆两处（方案①）** / 两处解剖图 / 导入结构与列编辑对话框 / 生成器目录（137 变体 · 15 分类 · 分类子菜单）/ 智能映射置信度呈现 / 状态与空态矩阵 / 主题映射与尺寸常量 / GPUI 落点与组件选型 / **§9 与 V1 的逐项对照（已迁 / 重设计 / 待办 / 不迁）** / 待拍板项 |
| `mock/mock-prototype.html` | Mock 数据生成原型（交互稿） | v2 原生（RDS Light/Dark 双主题 · 7 场景可切：空态 / 导入结构 / 已生成·出口就绪 / 落库反馈 / 只读 / 生成器分类子菜单 / 列编辑）；由 v1 原型（`v1/prototype/mock-data-generator.html`）**参考重画**，非搬运 |
| `mock/mock-architecture.md` | Mock 数据生成**设计理念与架构** | **六条不变式**（含生成不写库 · 出口不覆盖）/ 概念模型（配置 · 列 · 临时表 · 出口）/ 分层与 crate 归属（**视图随 crate，宿主经 `MockHost` 注入**）/ 状态所有权与副作用边界 / 数据流（含生成不写库与 cache-aside 取列）/ D1–D19 决策表 / 降级矩阵 / 性能与可观测 / **§9 已知问题（含 v1 迁移五项实证 + 锁重入死锁教训）** / 测试策略 / 实现位置映射 |
| `mock/mock-dev-plan.md` | Mock 数据生成开发方案 | 现状盘点（迁移完整度对照）/ **Phase A 完成项 A1–A17（含语义回归 · 方案①排版 · 四出口 · 导入结构）与验收证据** / Phase B–E 任务与落点（已标完成态）/ 测试场景 T1–T16 / 风险 R1–R8 / 验证命令 / 进度记录 |
| `dependencies/dependency-strategy.md` | 依赖治理 | 版本唯一入口 / 升级流程 / 编译时间手段 / 跨大版本待办 |
| `dependencies/duckdb-linking.md` | DuckDB 内核动态链接 | 为什么不再 `bundled`（编译时间 / 内存 / 体积）/ 库的落位（`third_party/duckdb/<版本>`，gitignore）/ 取库脚本 / 运行时 dll 拷贝 / 升级步骤与排错 / **§9 `target/` 体积治理（60 GB 提醒 + `--clean`）** |

## 关联目录

- `../migration/`：v1 → v2 迁移记录与命令退役
- 各 crate 内 `README.md`：该模块的入口与**特点提炼**（不复述设计），如 `../crates/project/README.md`、`../crates/scratchpad/README.md`
- `../architecture/` 之外：各 crate 内仅保留 README 级入口指针，不复制设计

## 阅读顺序建议

1. `overview.md` → 2. `crate-ownership-proposal.html` → 3. 按需进入 `layout/` / `theme/` / `settings/` / `connection/` / `database/` / `project/` / `scratchpad/` / `editor/` / `insight/` / `analytics_resource/` / `mock/`

## 模块文档集约定（每模块应具备的类型）

| 类型 | 作用 | 命名示例 |
| --- | --- | --- |
| 模块入口 | **从哪开始读**：特点提炼 / 边界 / 代码地图 / 硬约束 / 测试命令 / 文档地图 | `README.md` |
| 原型设计 | **长什么样**（视觉 / 布局 / 交互规格） | `*-prototype-design.md` |
| 可交互原型 | 可切换的视觉稿 | `*-prototype.html` |
| 架构与设计理念 | **为什么这样设计 / 怎么运转**（概念模型 / 数据流 / 决策 / 测试策略 / 实现映射） | `*-architecture.md` |
| 开发方案 | **做什么、做到哪**（阶段任务 / 逐轮记录 / 风险） | `*-dev-plan.md` |
| 使用手册 | **怎么用**（入口 / 导览 / 典型流程 / FAQ / 验收清单） | `*-user-guide.md` |

### 各模块文档类型缺口（功能模块）

| 模块 | 原型设计 | 交互稿 | 架构 / 设计理念 | 开发方案 | 使用手册 |
| --- | --- | --- | --- | --- | --- |
| `connection/` | ✅ | ✅ | ✅ `connection-dialog-architecture.md` | ✅ | ✅ |
| `database/` | ✅ | ✅ | ✅ `database-navigator-architecture.md`（本轮新增） | ✅ | ✅ `database-navigator-user-guide.md`（本轮新增） |
| `project/` | ✅ | ✅ | ✅ `project-view-architecture.md` | ✅ | ✅ `project-user-guide.md`（本轮新增） |
| `scratchpad/` | ✅ | ✅ | ✅ `scratchpad-architecture.md`（本轮新增） | ✅ | ✅ `scratchpad-user-guide.md`（本轮新增） |
| `editor/` | ✅ | ✅ | ✅ `editor-architecture.md` | ✅（方案待确认） | ⬜ 缺使用手册（实现后补） |
| `insight/` | ✅ | ✅ | ✅ `insight-architecture.md` | ✅ | ✅ `insight-user-guide.md`（规则格式属对外契约，已补） |
| `analytics_resource/` | ✅ | ✅ | ✅ `analytics-resource-architecture.md`（本轮新增，兼语义裁决书） | ✅ | ✅ `analytics-resource-user-guide.md`（本轮新增） |
| `mock/` | ✅（本轮新增） | ✅（自 v1 迁入） | ✅ `mock-architecture.md`（本轮新增） | ✅ `mock-dev-plan.md`（本轮新增） | ⬜ 缺使用手册（实现后补） |

> `layout/` / `theme/` / `ui/` / `dependencies/` 属**规格类**（单文档即可，不强制五件套）。

### `settings/` 的定位（跨行）

设置同时有**规格性**（尺寸与组件约束已归 `ui/`、配色已归 `theme/`）与**页面性**（有入口、有交互、有内容清单），因此按“页面”补文档：

| 文档 | 状态 |
| --- | --- |
| `settings-prototype-design.md`（原型设计） | ✅ 首版（待迭代） |
| `settings-architecture.md`（架构 / 准入裁决） | ✅ 首版（待迭代） |
| `settings-dev-plan.md`（开发方案） | ✅ 首版（P0 + P1a 已落地，见其 §0） |
| `settings-user-guide.md`（使用手册） | ⬜ 实现后补（与 `editor/` 同例） |
| `settings-prototype.html`（可交互原型） | ✅ 首版（示意稿，非权威；规格仍以 `ui/` + 原型设计文档为准） |

> 插件（M9）不在本表：v2 尚未为其建立文档集，等 beta3 立项时补（见 `settings-architecture.md` §10）。
