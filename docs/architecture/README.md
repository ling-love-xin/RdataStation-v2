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
| `settings/settings-crate-design.md` | settings crate | 动机 / 结构 / model / 持久化 / 迁移清单 |
| `project/project-prototype-design.md` | 项目管理原型 | 一实例一项目 / CRUD（增删改查）/ 选择器（最近·全部·已移除）/ 项目菜单 / 新建对话框 / 项目设置 / 未保存拦截与项目锁逃生口 / 固定排序持久化 |
| `project/project-prototype.html` | 项目管理原型（交互稿） | RDS Light/Dark 双主题：选择器、项目菜单、新建/设置/CRUD/拦截与锁弹层（可切换、可切主题） |
| `project/project-dev-plan.md` | 项目管理开发方案 | P0/A/B/C 任务（CRUD 主线 + 名册迁移 019）/ 文件落点 / 测试场景 / 风险 / 映射 |
| `project/project-view-architecture.md` | 项目视图架构（A3） | 宿主桥 `ProjectUiHost` 契约与状态所有权 / 对话框栈语义 / 窗口测试方案（含 `#[test]` 宏遮蔽坑）/ 实现映射 |
| `connection/connection-prototype-design.md` | 连接模块原型 | 新增连接对话框布局（v5 对齐）/ 五 Tab / 交互 / 主题映射 |
| `connection/connection-dialog-prototype.html` | 连接模块原型（交互稿） | RDS Light/Dark 双主题新增连接对话框示意（可交互） |
| `connection/connection-dev-plan.md` | 连接模块开发方案 | Phase A/B/C 任务 / 文件落点 / 测试场景 / 风险 |
| `ui/ui-design-spec.md` | UI 设计规范 | 三层约束（主题 token / 尺寸常量 / 组件规格）、rem 基准常量表 |
| `connection/connection-dialog-architecture.md` | 连接模块架构与数据流 | 分层与依赖 / 概念模型（数据库类型 vs 驱动实现、作用域、三类引用、Secret、暂存草稿）/ 状态所有权与对话框层挂载 / 五条数据流（打开·保存·暂存状态机·运行时连接）/ 快捷键 / 决策取舍 / 测试策略 / 数据字典与迁移 / 降级矩阵 / 性能·可观测·安全 / 成熟度评估 / **已知问题与后续项（§14，权威待办清单）** / **数据来源审计（§15，零 UI 造数据）** |
| `connection/connection-user-guide.md` | 连接模块使用指南 | 入口 / 界面导览 / 典型流程（新建·编辑·连续编辑·标签分组）/ Tab 速览 / 快捷键 / 草稿与安全 / FAQ 排查 / USIT 验收清单 |
| `database/database-navigator-prototype-design.md` | 数据源管理 / 数据库导航原型 | 合并面板布局 / 树模型与作用域 / 三级元数据管线 / 交互 / 主题映射 / 落点 |
| `database/database-navigator-prototype.html` | 数据源管理 / 数据库导航原型（交互稿） | RDS Light/Dark 双主题面板示意（树 / 加载 / 搜索 / 右键菜单 / 空态可切） |
| `database/database-nav-dev-plan.md` | 数据库导航开发方案 | Phase A/B/C 任务 / 迁移与表 / 测试场景 / 风险 / 验证 / 映射 |
| `scratchpad/scratchpad-prototype-design.md` | 草稿箱原型 | 根 = 项目目录 / 240px 左 Dock 面板 / 树与交互 / 主题映射 |
| `scratchpad/scratchpad-prototype.html` | 草稿箱原型（交互稿） | RDS Light/Dark 项目工作区示意（可交互） |
| `scratchpad/scratchpad-dev-plan.md` | 草稿箱开发方案 | P0 项目会话 + Phase A/B/C / 文件落点 / 测试场景 / 风险 |
| `dependencies/dependency-strategy.md` | 依赖治理 | 版本唯一入口 / 升级流程 / 编译时间手段 / 跨大版本待办 |

## 关联目录

- `../migration/`：v1 → v2 迁移记录与命令退役
- `../architecture/` 之外：各 crate 内仅保留 README 级入口指针，不复制设计

## 阅读顺序建议

1. `overview.md` → 2. `crate-ownership-proposal.html` → 3. 按需进入 `layout/` / `theme/` / `settings/` / `connection/` / `project/` / `scratchpad/`
