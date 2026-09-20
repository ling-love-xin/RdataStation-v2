//! Mock 面板的动作（M7）。
//!
//! **只在事件路径上**：动作由视图 `on_action` 处理（改状态 → 提交后台任务），渲染路径不认识它们。
//!
//! 命名空间是 `mock`，键位在 `crates/app` 统一注册（**注册了才宣传**：没实现的动作不绑键，
//! 也不写进文档的快捷键表）。
//!
//! 键位的 key context 是 `mock-detail`（中央「Mock · {表}」tab 根元素的 `key_context`）——
//! 只有焦点在那张 tab 里才生效，不抢编辑器的同名键（`Ctrl+Enter` 在编辑器是「执行 SQL」，
//! 两个 context 互不重叠）。

use gpui_kit::*;

actions!(
    mock,
    [
        /// 生成当前草稿（等价于点中央表头的「生成 / 重新生成」；结果表 tab 上无效）
        GenerateMock
    ]
);
