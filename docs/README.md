# RdataStation v2 文档总览

> 本目录是项目的所有文档：架构设计、迁移指南、测试报告、竞品分析等。

## 目录结构

```
docs/
├── README.md                    # 本文件（总览）
├── project-progress.md          # 项目进度与计划
├── architecture/                # 架构设计（核心文档）
├── migration/                   # v1 → v2 迁移记录
├── reports/                     # 测试报告
├── analysis/                    # 竞品分析、调研（不入库）
└── tmp/                         # 临时产物（不入库）
```

## 快速导航

### 1. 架构设计（`architecture/`）
**先读这个：**
- [`overview.md`](architecture/overview.md) - 架构总览
- [`core-design-current.md`](architecture/core-design-current.md) - 核心设计现状版
- [`module-status.md`](architecture/module-status.md) - 模块状态与测试基线

**按模块：**
| 模块 | 说明 | 文档入口 |
|------|------|----------|
| 连接 | 数据源连接管理 | [`connection/README.md`](architecture/connection/README.md) |
| 数据库导航 | 数据源管理与元数据浏览 | [`database/README.md`](architecture/database/README.md) |
| 编辑器 | SQL 编辑器 | [`editor/README.md`](architecture/editor/README.md) |
| 草稿箱 | 项目工作区 | [`scratchpad/README.md`](architecture/scratchpad/README.md) |
| 洞察 | 数据分析与洞察 | [`insight/README.md`](architecture/insight/README.md) |
| Mock | 测试数据生成 | [`mock/README.md`](architecture/mock/README.md) |
| 资产库 | 分析存档与复现 | [`analytics_resource/README.md`](architecture/analytics_resource/README.md) |
| 项目管理 | 项目与实例 | [`project/README.md`](architecture/project/README.md) |
| 设置 | 设置系统 | [`settings/settings-architecture.md`](architecture/settings/settings-architecture.md) |
| 主题 | 主题与配色 | [`theme/theme-design.md`](architecture/theme/theme-design.md) |
| 布局 | 工作台布局 | [`layout/layout-design.md`](architecture/layout/layout-design.md) |
| 插件 | 插件系统 | [`plugin/README.md`](architecture/plugin/README.md) |
| 通知 | 通知系统（新） | [`notification-crate-design.md`](architecture/notification-crate-design.md) |
| 联邦查询 | 多源跨源查询 | [`federation/README.md`](architecture/federation/README.md) |
| 发布 | 发布流水线 | [`release/release-pipeline.md`](architecture/release/release-pipeline.md) |

### 2. 迁移指南（`migration/`）
- v1 → v2 迁移记录与命令退役

### 3. 测试报告（`reports/`）
- 各模块的测试报告与性能基线

### 4. 竞品分析（`analysis/`）
- 竞品深度调研（不入库）
- 参考项目：navop / zqlz / dbflux / dbui / sqlab / DBeaver / DataGrip

## 文档约定

### 模块文档集（每个功能模块应具备）
| 类型 | 作用 | 命名示例 |
|------|------|----------|
| 模块入口 | 从哪开始读 | `README.md` |
| 原型设计 | 长什么样 | `*-prototype-design.md` |
| 可交互原型 | 可切换的视觉稿 | `*-prototype.html` |
| 架构与设计理念 | 为什么这样设计 | `*-architecture.md` |
| 开发方案 | 做什么、做到哪 | `*-dev-plan.md` |
| 使用手册 | 怎么用 | `*-user-guide.md` |
| 一页看懂 | 可贴版总结 | `*-showcase.md` |

### 不入库的目录
- `docs/tmp/` - 临时分析产物、日志、截图
- `docs/analysis/` - 竞品研究、过程性调研
- `docs/**/_backup/` - 版本备份
- `docs/**/_shots/` - 截图

## 阅读顺序建议

1. **新人入门**：`architecture/overview.md` → `architecture/core-design-current.md` → 按需进入具体模块
2. **改某个模块**：先读该模块的 `README.md` → 再读 `architecture.md` → 最后看 `dev-plan.md`
3. **了解项目进度**：`project-progress.md` → `architecture/module-status.md`

## 相关链接

- 仓库根目录：`../README.md`（项目介绍）
- 各 crate 内的 `README.md`：该模块的入口与特点提炼
