//! L4 动作注册表（方案稿 §4.5 / D6-A，B4）：对象菜单动作的纯数据元模型。
//!
//! ## 职责边界
//!
//! 所有「某类对象在某族下能出现哪些菜单动作、归哪个分组、顺序、门控依据、快捷键同源元数据」
//! 从 `nav_view/rows.rs` 的散落 `if` / `data_like` / 条件 push 收拢到本层；**新增动作 = 在
//! 静态登记表里追加一条描述符**，视图层不再写类型 `match` 与门控 `if`。
//!
//! - 本模块只承载**静态元数据与纯函数判定**：无 I/O、不持连接状态、不挂回调（决策 A：
//!   注册表纯数据，点击后视图层按 [`NavActionId`] 枚举 match 到现有处理器闭包）；
//! - 类型适用性（`applies`）回答「该类型上该不该有这项动作」；运行时靶可用性
//!   （`property` / `qualified` / 路径靶是否 `Some`）属视图层靶构造判定，不进注册表；
//! - 连接根菜单、分组头菜单属连接生命周期 / 组织模型动作，不在本注册表范围（后续批次扩
//!   [`TargetKind`] 与连接动作描述符时再纳入）。
//!
//! ## 分派链
//!
//! 视图层取节点 [`engine::ObjectKind`] + 连接态 + L2 能力集（`crate::nav::capability`）→
//! [`NavActionRegistry::entries_for`] 得到分组分段、排序后的 [`ResolvedEntry`] 序列 →
//! 按分组渲染 `PopupMenuItem`，`on_click` 内 match [`ResolvedEntry::descriptor`].id 分派。

mod registry;

pub use registry::{
    ActionGroup, NavActionDescriptor, NavActionId, NavActionRegistry, ResolvedEntry, TargetKind,
    registry,
};
