# 数据源连接模块 · 原型设计（新增连接对话框）

> 状态：**待二次确认** · 关联文件：`connection-dialog-prototype.html`（可交互原型）、`connection-dev-plan.md`（开发方案）
> 参考基准：布局对齐 v1 原型 `v1/prototype/add-datasource-v5.html`（+ `v5-analysis.md`）；配色服从 `docs/architecture/theme/theme-design.md`（RDS Light/Dark，rds-theme.json）；机制参考 DataGrip「数据源和驱动程序」方案
> 技术栈：GPUI（gpui-kit 0.6），组件消费 `cx.theme()` 语义 token，代码零裸 hex

## 1. 设计基准

| 维度 | 基准 | 说明 |
| --- | --- | --- |
| 布局 | v1 v5 原型 | 模态对话框：标题栏 + 左侧栏（搜索/暂存/类型树）+ 右侧主面板（Header 三行 / 五 Tab / 内容区）+ 底部操作栏 |
| 配色 | theme-design.md | 对话框=popover token；侧栏=sidebar token；选中项=coral 左边条；主按钮=primary coral |
| 机制 | DataGrip + v1 后端设计 | 数据源=连接配置（driver/auth/network/env 引用），测试→保存→内省闭环 |
| 后端契约 | v1 DATA-SOURCE-MODULE.md / DATASOURCE-DRIVER-ARCHITECTURE.md | drivers.config_schema 驱动表单；auth_configs AES-256-GCM；network_configs 协议链；environments+policies |

## 2. 布局结构（对齐 v5 原型）

```
┌─ dialog（模态，popover 底 + border 边框，圆角 8）────────────────────┐
│ 标题栏（title_bar）：『新建连接』                     ─ □ ✕ 窗口点      │
├───────────────────────────────────────────────────────────────┤
│ sidebar（200px，sidebar 底）      │ main-panel（flex:1）            │
│  ├ 搜索框（input）                │  ├ Header：                     │
│  ├ 暂存列表（saved-section）      │  │  ① 名称 + 驱动类型 + 作用域    │
│  │   ├ A · MySQL 演示（未保存）   │  │     （全局/项目，可同选）      │
│  │   └ ＋ 新建配置                │  │  ② 备注（宽输入）              │
│  ├ 分割线                         │  │  ③ URI 实时预览 + ✎ 编辑      │
│  └ 数据库类型（db-section）        │  │  ④ 作用域语义提示行            │
│      ▾ 关系型(4) 文件型(2) …      │  ├ tabs-bar（tab_bar 底）        │
│      ▸ NoSQL(2) 分析型(2)         │  │  常规 | 网络 | 能力 | 驱动属性 | 高级
└───────────────────────────────────────────────────────────────┘
   底部操作栏： [🔌 测试连接]  ✓ 成功·版本·延迟          [取消] [保存]
```

- 文件型驱动（SQLite/DuckDB）：连接区切换为文件选择，网络 Tab 隐藏
- 测试连接结果内联展示在底部（success 绿 / danger 红 + 版本 + 延迟 ms）

### 2.1 作用域（全局/项目，可同选 —— 对齐后端双轨设计）

| 选择 | 落库语义 |
| --- | --- |
| 仅 全局 | 保存至 `global.db` 的 `global_connections`（`G_xxx`） |
| 仅 项目 | 保存至当前项目 `project.db` 的 `connections`（`P_xxx`） |
| 全局 + 项目 | 保存为全局（`G_xxx`）**并共享至当前项目**（`GP_xxx` 快照引用）——连接同属两作用域 |

- 依据：v1 双轨制（`convert_to_global_connection` / `convert_to_project_connection`，v2 `workbench/services/connection_service.rs:1426/1516`）+ ID 前缀 `G_/P_/GP_`（engine `id_prefix.rs`）；规则：项目可引用全局（快照 GP_），全局不可引用项目私有配置
- Header 第 ④ 行实时提示当前组合的保存目标

## 3. 五 Tab 内容（对照 v5 原型）

### 3.1 常规（tab-general）—— 卡片式布局（本轮优化）
- 顶部 driver 信息条（info banner）：当前驱动名 + 类型徽标
- **三张 section 卡片并排**（`sec-card`，flex-wrap 自适应），每卡内部为对齐网格（label 固定列宽 + 输入框）：
  - **连接设置**：主机 / 端口 / 数据库（网络库），文件库则切换为文件选择
  - **数据库认证**（含**管理**入口，打开 AuthConfigManager 覆盖层）：
    - 认证方法 select（按驱动 `supported_auth_types` 过滤）+ 动态字段（用户名/密码/证书/Keytab…）
    - **引用已保存认证配置**下拉（选中后字段只读 disabled + 降透明）
  - **连接安全（SSL/TLS）**：模式（disable/require/verify-ca/verify-full）、CA 证书、客户端证书/私钥（按驱动显示）
- 驱动特有字段（form-driver-specific）追加在卡片下方

### 3.2 网络（tab-network）
- 提示条：协议链（SSH / HTTP(S) 代理）、最大 4 跳、拖拽排序；**SSL/TLS 见「常规 → 连接安全」**
- **引用网络配置**：选择已保存网络配置（network_store，内联编辑或整体引用）；**管理网络配置**入口（NetworkConfigManager 覆盖层）
- 链列表：列头（拖拽 · # · 协议 · 配置 · 启用 · 操作），行：SSH / Proxy 节点
- 添加跳：类型菜单（SSH / Proxy）+ 新建表单（各协议字段，见 v5 §3.3）
- **拓扑预览**（topo-preview）：数据路径图（Client → SSH → Proxy → DB，DB 节点带 TLS 徽标）
- 约束校验：SSH/Proxy 合计 ≤4 跳、hop 命名合法；违反时 chain-warning 提示

### 3.3 能力（tab-capabilities）
- 从 `driver.capabilities` JSON 解析，矩阵/chip 展示（只读）

### 3.4 驱动属性（tab-properties）
- `driver_properties` key-value 行，支持动态增删（高级用户）

### 3.5 高级（tab-advanced）
- **环境选择**（紧凑下拉）：环境 + 策略摘要行（只读/导航过滤/超时/行数/DDL·DML 摘要）；**管理环境**入口（EnvironmentManager 覆盖层，v1 内联于 AdvancedTab）
- **DuckDB 本地加速卡片**（warning 色标题，提高权重）：开关 + 展开 2×2 参数网格（缓存路径/大小/自动刷新等）+ 存储说明；仅网络数据库可见
- **安全策略**（可折叠）：策略明细勾选，可覆盖环境默认（标记"已覆盖"）

### 3.6 凭证 / 网络 / 环境的管理与引用（复用机制）
| 类型 | 引用（连接内） | 管理（覆盖层，v1 弹出窗） | 复用语义 |
| --- | --- | --- | --- |
| 认证配置 | 常规→数据库认证、网络→SSH/Proxy 认证：下拉引用已保存配置，选中后字段只读 | AuthConfigManager：分类列表（password/ssh_key/proxy_pwd…）+ 新建/编辑/删除；列表脱敏；显示**被引用计数** | 数据库 A 直接引用数据库 B 的认证配置（共用同一份凭据），修改一处全量生效；被引用的配置不可删除 |
| 网络配置 | 网络 Tab：下拉引用已保存协议链（内联编辑或整体引用） | NetworkConfigManager：配置档案（SSH 跳板/代理…）+ CRUD；显示**被复用计数** | 建一次网络档案，多连接复用 |
| 环境 | 高级 Tab：下拉选择环境 + 策略摘要 | EnvironmentManager：环境 CRUD + 策略编辑；显示**被使用计数** | 环境（5 类策略）共享；单连接可覆盖策略并标记"已覆盖" |

## 4. 关键交互

| 交互 | 行为 |
| --- | --- |
| 类型树 | 分类折叠/展开；搜索过滤；选中类型 → 顶部驱动下拉按 type_id 过滤 |
| 作用域 | 全局/项目**可同选**（双轨设计）：仅全局→`G_xxx`；仅项目→`P_xxx`；全局+项目→保存全局并共享至当前项目（`GP_xxx` 快照）；Header 提示行实时显示保存目标（2026-09-11：项目根由当前项目会话自动预填，可在对话框内修改） |
| URI 预览 | 随字段实时拼装（url_template）；点击 ✎ 进入手动编辑模式，完成/恢复预览 |
| 测试连接 | 独立会话（复用 ConnectionFactory，不污染正式连接），返回 `{success, message, latency_ms, version}`；成功 → 绿色 ✓ + 版本 + 延迟；失败 → 红色错误信息 |
| 保存 | 校验 → 落库（ID 前缀 G_/P_）→ `use_duckdb_fed=1` 时注册 DuckDB Secret → 触发 introspection → 关闭并刷新列表 |
| 暂存列表 | 未保存配置自动暂存、可切换续编；关闭对话框不丢失 |
| 认证配置选择 | 按 auth_type 过滤已保存配置；选中后动态字段只读；**管理**按钮打开 AuthConfigManager |
| 网络/环境引用 | 网络 Tab「引用网络配置」+「管理网络配置」；高级 Tab「管理环境」——均打开对应覆盖层 |

## 5. 主题映射（token → 视觉，两套一致结构）

| 元素 | RDS Light | RDS Dark | Token |
| --- | --- | --- | --- |
| 对话框/输入框底 | `#FFFFFF` | `#252526` | background / popover |
| 边框/输入边 | `#D4D4D4` | `#3C3C3C` | border / input.border |
| 标题栏 | `#DDDDDD` | `#323233` | title_bar |
| 左侧栏 | `#F3F3F3` | `#252526` | sidebar |
| 选中项底 | `#E4E4E4` | `#37373D` | list.active / sidebar.accent |
| 选中项左边条 | `#C25B46` | `#E8846F` | list.active.border（品牌 coral） |
| 悬停 | `#F0F0F0` | `#2A2D2E` | list.hover |
| 正文/弱文字 | `#333333` / `#8E8E8E` | `#CCCCCC` / `#8A8A8A` | foreground / muted |
| 主按钮（保存） | `#C25B46` 白字 | `#E8846F` 黑字 | primary |
| Tab 栏/激活 | `#F3F3F3` / 白底+coral 顶条 | `#252526` / `#1E1E1E` | tab_bar / tab.active |
| 成功/警告/信息 | `#16A34A` / `#B45309` / `#2563EB` | `#89D185` / `#CCA700` / `#3794FF` | semantic |
| 状态栏（如用） | `#C25B46` 白字 | `#A84A38` 白字 | primary 派生 |

> 实现时色值只存在于 `assets/themes/rds-theme.json`，GPUI 组件经 `cx.theme()` 读取，禁止写裸 hex（theme-design.md §5 约束）。

## 6. GPUI 落点映射

| 原型元素 | GPUI 落点 |
| --- | --- |
| 对话框 | `crates/connection/src/connection_dialog.rs`（`ModalLayer`/`Dialog`） |
| 标题栏/窗口点 | gpui-kit `TitleBar` 或自绘；窗口点由宿主窗口控制 |
| 类型树/暂存列表 | `List` + 自绘行；分类折叠用 `Disclosure` |
| 动态表单 | 按 config_schema 渲染 `Form`/`Input`/`Select`（driver_store 数据） |
| 认证/网络/环境配置 | 对接 engine `auth_store` / `network_store` / `env_store` |
| 测试连接 | `DataSourceService::test_connection`（独立会话） |
| 保存链路 | `DataSourceService::save` → global_db + Secret 联动 + introspection |
| 五 Tab | `Tabs` 组件（gpui-kit） |

## 7. 待确认项

1. **作用域同属**：全局/项目可同时选中（后端双轨设计：G_/P_/GP_ 快照），原型与文档已按此更新 —— 确认保存语义无误？
2. **SSL 移常规**：SSL/TLS 从网络 Tab 移入「常规 → 连接安全」，协议链仅 SSH/Proxy（≤4 跳），已按此改原型与文档 —— 确认？
3. **管理与引用**：认证/网络/环境三类均补"引用 + 管理覆盖层 + 复用计数"，与 v1 v5 原型一致 —— 确认？
4. 入口形态：活动栏/导航树/空态按钮 → 弹模态对话框；中央 EditorPanel 旧 5 字段表单移除 —— 确认？
5. 其余设计决策以 `connection-dev-plan.md` 为准，确认后开始 Phase A。
