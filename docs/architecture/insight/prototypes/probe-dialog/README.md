# 探查确认弹窗原型（probe-dialog）

**对应功能**：触发"探查"后弹出的确认窗口——探查对象、数据量（全量/行数/百分比）、采样方式（顺序/随机）、探查列（全部/选择）、过滤条件、规模预估、底部三按钮（取消/全量探查/探查）。

**对应源码**：
- `crates/insight/src/workspace/probe_dialog.rs`（弹窗实体）
- `crates/insight/src/workspace/mod.rs`（`sync_probe_modal` 开关弹窗）
- `crates/insight/src/insight_view.rs`（`confirm_probe_dialog`/`cancel_probe_dialog`/`probe_quick_amount`）

**当前状态**：**新建（尚无 HTML 原型）**。当前直接改 GPUI 源码，未先出原型。

**本轮已落地调整**（2026-11-07）：
- 删内容区重复小标题（Dialog 标题已是"探查确认"）
- 表名加"探查对象："前缀
- 删数据量 RadioGroup，只留 chips 快捷预设 + toggled 高亮
- 过滤说明精简
- 规模预估加过滤判断
- 删提示灰底框
- 底部三按钮：取消（ghost）/ 全量探查（outline+Zap）/ 探查（primary，文案随列选择变）

**待验证**：
- [ ] 三按钮布局/间距/对齐
- [ ] chips toggled 高亮视觉
- [ ] Dialog 自带按钮条是否真的不渲染
