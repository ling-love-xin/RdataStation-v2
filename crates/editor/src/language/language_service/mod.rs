//! 编辑器的**语言服务面**：补全 / 诊断 / code action **三面同源**（纯函数层，不碰 GPUI）
//!
//! ## 为什么收成一个面（而不是三套）
//!
//! 三面要判的**是同一件事的不同投影**：都得先回答"这段文本在词法上是什么"——哪个词是关键字、
//! `where` 是写在字符串里还是真的子句、那个 `(` 是代码还是字面量的一部分。只是问答不同：
//!
//! | 面 | 问的问题 | 取用请求里的哪几段 |
//! | --- | --- | --- |
//! | [`complete`] | 光标处要补什么 | 文本 + 光标 + 目录 + 连接 |
//! | [`diagnostics`] | 这篇文本哪里写不下去 | 文本（**整篇**，与光标无关） |
//! | [`code_actions`] | 光标 / 选区那条语句能怎么安全改写 | 文本 + 光标 + 选区 + 通道 |
//!
//! 收在一处的三个理由：
//!
//! 1. **一致性**：同一段文本在三面得到同一个词法结论。各扫各的，迟早出现"补全认得 `where`
//!    是关键字、诊断却把字符串里的 `where` 当成子句"这种分歧（那种 bug 极难查）；
//! 2. **一处演进**：加一条诊断 / 加一条改写只动这个模块；词法或目录口径变了，三面一起跟上；
//! 3. **UI 零驱动分支**：界面只构造一次请求（[`Request`]）、拿一个 [`ServiceReport`]，
//!    不需要知道"这个库能补什么、能报什么"——dbflux 把三面收进一个 `LanguageService`
//!    交给驱动，同一个理由（界面不判驱动类型）。
//!
//! ## 词法只有一个来源
//!
//! 凡是要 token 的地方一律问 [`highlight_spans`]（sqlglot tokenizer；与高亮 / 折叠 /
//! 通道闸同一份词法）：字符串里的 `where` 不算子句、引号里的 `(` 不算括号、注释里的 `limit`
//! 不算已有上限。**只有它整篇扫不动的时候**（词法失败 → 空区间）本模块才退到字节层的
//! 保守检查，而那一步只会让结论**更弱**（找不到闭合符才报），不会凭空造出结论。
//!
//! ## 三面各自的边界（判不出就不报）
//!
//! - **补全**：上下文判定与挑排**不在这里重写**，只转发给 [`crate::completion`]（`request_at`
//!   + `candidates`）。**未绑定连接**时不采用目录里那部分——没有目标库就谈不上"按通道给
//!   限定名"，宁可这次只给关键字与函数（与 `CompletionPort` 的契约同源，见 [`complete`]）。
//! - **诊断**：**只有词法层判得动的错**——未闭合的引号字面量 / 引号标识符 / 块注释 /
//!   美元引用，以及括号不平衡。不解析语法树，不报"这里该有个 FROM"这种要文法才说得清的话：
//!   半写着的 SQL 天天在编辑器里，押注式报错会把注意力耗光。报不出位置就一条都不报。
//! - **code action**：**只有目标位置确定时才给**（改哪一段、换成什么都要说得出）。目标是
//!   "光标（选区优先）所在的**那一整条语句**"，判据与执行族同源（`split_statements`）；
//!   两条动作都是**追加在语句末尾**，所以末尾挂不稳（还挂着运算符 / `FROM` / `(`）、
//!   末尾是行注释（插进去会被注释吃掉）、或者这条语句在这条通道上根本跑不了（通道闸会拒）
//!   时一律**回绝**。
//!
//! ## 有意的边界（都有测试钉住）
//!
//! - **不认方言**：`LIMIT` / `#` 注释 / `TOP` 这些方言差异本模块不判（源方言要从连接来，
//!   见 [`crate::translate`] 的口径）；`#` 之后的东西按 `engine::sql::split` 的同一选择
//!   当成代码而不是注释。
//! - **未闭合的美元引用按 token 判**：词法层有时会把 `$$abc` 接住成一个字符串 token
//!   （不是失败），我们按 token 的形状判它没闭合；词法层自己没扫出来的怪字符，一条都不报。
//! - **括号按语句判**，一条语句只报一处（后面的错互相牵连，报多了是噪音）。
//! - 超过 [`SCAN_MAX_BYTES`] 不扫（与折叠 / 着色同一量级：大文件不每键全扫）。

use engine::sql::SqlDialect;

use crate::channel::ExecChannel;
use crate::completion::{Candidate, Catalog};

/// 补全面最多给几条（与 `view/completion.rs` 的 `MAX_ITEMS` 同量级：够选就行）
pub const MAX_CANDIDATES: usize = 60;

/// 超过这个字节数不做打字期扫描（诊断 / 动作都走这个门槛）
pub const SCAN_MAX_BYTES: usize = 1_000_000;

/// 定位未闭合构造时最多问几次词法（每次都要扫一遍前缀，预算要小）
const PROBE_BUDGET: usize = 12;

/// 一次语言服务请求
///
/// 三面共用这一个形状：界面从自己手上已有的东西（编辑内核的文本 / 光标 / 选区、宿主给的
/// 目录与绑定）一次摆好，**不必为每个面各造一份输入**——那正是"驱动分支"的入口。
#[derive(Debug)]
pub struct Request<'a> {
    /// 整篇文档文本（下面所有字节偏移都在这个坐标系里）
    pub text: &'a str,
    /// 光标（字节偏移；越界或落在多字节字符中间会就地钳到字符边界）
    pub cursor: usize,
    /// 选区（字节区间；`None` = 没有选区）。首尾的空白与分号不算判据
    pub selection: Option<(usize, usize)>,
    /// 候选目录快照（宿主按 (连接, 通道) 组装好；见 [`completion::CompletionPort`]）
    pub catalog: &'a Catalog,
    /// 文档绑定的连接 id（`None` = 跟随当前连接）
    pub connection: Option<&'a str>,
    /// 执行通道（限定名按它给 + 写语句在它上面可能被拒）
    pub channel: ExecChannel,
    /// 源方言（语义诊断 / 未来跳转按它解析；宿主按连接驱动注入，见 `diagnostics`）
    pub dialect: SqlDialect,
}

/// 一次请求的三类结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceReport {
    pub completion: Vec<Candidate>,
    pub diagnostics: Vec<Diagnostic>,
    pub code_actions: Vec<CodeAction>,
}

/// 一条打字期诊断（**字节区间**，行列为派生）
///
/// 没有 `severity` 字段：本模块只报有把握的错，拿不准的一律不报，所以没有第二档。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// 稳定标识（接线时直接给 `lsp_types::Diagnostic::code`；测试按它断言）
    pub code: &'static str,
    /// 说给人听的一句（说的是**词法层**看到的事，不假装是语法分析）
    pub message: String,
    pub start: usize,
    pub end: usize,
}

impl Diagnostic {
    fn new(code: &'static str, message: &str, start: usize, end: usize) -> Self {
        Self {
            code,
            message: message.to_string(),
            start,
            end,
        }
    }

    /// 落进内核诊断要的行列：`((行, 列), (行, 列))`，**0 基**、列按**字符**数
    ///
    /// 与 `crate::diagnostics` 的 1 基显示口径同一套换算（那里给人看，这里给 `Position`）。
    pub fn positions(&self, text: &str) -> ((usize, usize), (usize, usize)) {
        (line_column(text, self.start), line_column(text, self.end))
    }
}

/// 一段改写：把 `[start, end)` 换成 `replacement`（`start == end` = 纯插入）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

/// 一条改写型动作
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeAction {
    /// 稳定标识（菜单排序、接线、测试都用它；与标题分开，标题随时可以改文案）
    pub id: &'static str,
    /// 菜单上那一行
    pub title: String,
    pub edit: TextEdit,
}

/// 加 `WHERE 1 = 0`：先不匹配任何行，确认影响面之后再改掉
const ADD_WHERE: &str = "add-where-match-nothing";
/// 加 `LIMIT 100`：先看一眼前 100 行
const ADD_LIMIT: &str = "add-limit-100";

// ---------------------------------------------------------------------------
// 面一：补全
// ---------------------------------------------------------------------------

/// 光标处要补什么
///
/// **上下文判定与挑排全部转发给 [`crate::completion`]**，这里只做两件事：钳坐标、把
// ==== Step 9 拆分（2026-10-29）：三面按能力域迁入子模块 ====
mod face_complete;
mod face_diag;
mod face_actions;

pub use face_complete::*;
pub use face_diag::*;
pub use face_actions::*;

#[cfg(test)]
mod tests;
