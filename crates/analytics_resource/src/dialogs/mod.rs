//! M6 对话框（归档 / 取回）。
//!
//! 分工与面板一致：**表单状态、校验与渲染在 crate 内，执行与窗口能力归宿主**——
//! `on_submit` 由宿主注入（照 `group_form_dialog` 的形状），crate 不认识
//! `ArchiveService` / `ProjectDatabaseManager`，也不碰项目库。
//!
//! 两条实现纪律（都踩过）：
//!
//! 1. **输入实体在开窗前建好**：`open_dialog` 的 builder 是 `Fn`（每帧重建），
//!    在里面 `cx.new` 就会每帧漏一个 `InputState`；
//! 2. **校验是"从输入值推导"，不是标志位**：每帧重算，用户改回去提示自然消失——
//!    标志位会留下"提示还在但已经合法"的假象。

pub mod archive;
pub mod checkout;
