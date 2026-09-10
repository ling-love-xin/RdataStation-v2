import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\docs\migration\v1-to-v2-mapping.md')
t = p.read_text(encoding='utf-8')

marker = '''### ⏳ 下一轮候选

- GPUI 视图层：workbench 主界面（数据库导航/连接管理/资源分析）首个可交互视图；
- mock 命令装配（mock_commands → GPUI 或 CLI 入口）；
- v1 源文件清理决策（v1/ 暂存区保留 vs 按 Feature 删除）。
'''

round20 = '''### ✅ Round 20（已完成，DockArea 可拖拽布局系统接入，`cargo check --workspace` 零告警）

**目标**：把 Round 19 的单体骨架升级为 GPUI-kit DockArea 布局系统——左侧栏与中央内容区成为可拖拽、可收起、可持久化（DockAreaState）的面板，为后续 Feature 面板（数据库导航 / 资源分析 / 草稿箱 / 洞察）提供挂载容器。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/panels.rs`（12KB，新增） | `Shared`（面板间共享状态：active_tool / selected / connections / notice）、`SidebarPanel`（连接列表可选中 / 导航树 / 资源 / 设置）、`EditorPanel`（连接详情 + 新建连接 + 通知文案）、`SidebarEvent`（SelectConnection）；两个面板完整实现 base `Panel` + component `Panel`（panel_name / tab_name / title / Focusable / EventEmitter） |
| `crates/workbench/src/view.rs` | `WorkbenchView` 重构：持有 `Entity<DockArea>`，render 首次懒初始化（`init_workspace`：创建面板实体 → `cx.subscribe` 订阅选中事件 → `DockSkin::dock_area` → `set_center(h_split: sidebar 240px + editor)`）；标题栏加侧边栏收起/展开按钮（`area.toggle_dock(Left)`）；活动栏点击更新共享状态并通知面板 |
| `crates/workbench/src/lib.rs` | `pub mod panels;` |
| `tools/fix_round20.py` | 本轮修复脚本归档 |

**gpui-kit 0.6 Dock 真实 API 修正记录（8 类编译错误 → 0）**：

| 初版用法 | 0.6 真实 API | 说明 |
| --- | --- | --- |
| `Entity<T>.emit(app, ev)` | `entity.update(app, \\|_, cx\\| cx.emit(ev))` | `emit` 是 `Context::emit`（要求 T: EventEmitter<Evt>），Entity 上无此方法 |
| `cx.subscribe(&e, \\|this, event, cx\\| ...)` 3 参 | `cx.subscribe(&e, \\|this, _entity, event, cx\\| ...)` 4 参 | `Context::subscribe` 闭包签名 `FnMut(&mut T, Entity<T2>, &Evt, &mut Context<T>)`——第二参是被订阅实体（值），第三参才是事件 |
| `DockLayout::tabs().panel(entity)` | `DockLayout::tabs().panel_view(panel_handle(entity), cx)` | builder 无 `panel()`，只有 `panel_view(Arc<dyn PanelView>, cx)`；`panel_handle` 在 `component::dock` |
| `Rc<Cell<Option<String>>>` | `Rc<RefCell<Option<String>>>` | `Cell::get` 要求 T: Copy，Option<String> 不满足 |
| `self.connections.borrow().get(i).cloned()`（闭包内） | 先 `let conns = self.connections.borrow();` 再取 | 临时 borrow 悬垂（E0515） |
| subscribe 返回值未用 | 存入 `_subscription: Option<Subscription>` 字段 | 防 drop 即取消订阅；编译器强制 must_use |
| 面板 `EventEmitter<PanelEvent>` | `EventEmitter<gpui_kit::component::dock::PanelEvent>` | base Panel 的关联事件类型经 component re-export |

**验证**：`cargo check --workspace -j 2` 零告警；`cargo build -p rds-app` 后运行验证：进程稳定存活、无 panic（DockArea + 双面板渲染正常）。布局持久化（DockAreaState dump/load）与命令面板（Action/Keybinding）留待下一轮。

'''

assert marker in t, 'marker not found'
t = t.replace(marker, marker + round20)
p.write_text(t, encoding='utf-8')
print('Round 20 appended, total lines:', len(t.splitlines()))
