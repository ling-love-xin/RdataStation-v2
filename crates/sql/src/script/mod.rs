//! 脚本语言词法高亮（自研轻量词法器，纯函数、零依赖）
//!
//! ## 定位（与 SQL 词法的关系）
//!
//! `crate::sql::highlight` 用 sqlglot tokenizer 扫 SQL；这里为**通用模式**的脚本文件
//! （Python / R / Rust / Markdown / JSON / Log）各自提供一个轻量词法器，产出与 SQL
//! **同构**的 `Vec<HighlightSpan>`（字节区间 + 类别），视图层复用同一套语义 token 通道
//! 与主题词汇上色——补上「脚本只有文件类型门控、没有上色本体」的缺口（编辑器遗留 #14）。
//!
//! ## 为什么自研而不是 tree-sitter
//!
//! 1. **零新依赖**：不引入 grammar crate / 构建产物 / highlights 查询，符合「依赖只向下」
//!    与 vendor 化约束；
//! 2. **高亮只需要词法层**：着色不依赖语法树；编辑中的脚本常常写不完，语法树会解析失败，
//!    词法扫描仍能给出可用着色（与 SQL 词法的取舍一致，见 `sql/highlight.rs` 模块头）；
//! 3. **只有 3/7 语言有官方 grammar**：CSV / Log / Markdown 无 tree-sitter grammar 可用，
//!    引入 tree-sitter 也只能覆盖一半语言，另一半仍要自研（VS Code 对 CSV / Log 也默认
//!    不上色，靠第三方扩展做装饰性着色——见调研记录）。
//!
//! ## 词法器约定
//!
//! - 每个语言一个纯函数（`xxx_spans(&str) -> Vec<HighlightSpan>`），**整篇扫描**：
//!   provider 接口无跨行状态，跨行结构（Python 三引号 / Rust 块注释 / Markdown 围栏代码块）
//!   在一次扫描内用局部状态处理，不依赖行间缓存；
//! - 区间取**原文**（含引号与注释标记本身），与 SQL 词法一致；
//! - **词法失败不 panic**：未闭合字符串 / 引号等按已识别部分产出，其余正文色；
//! - 类别只用 `crate::sql::TokenClass`（扩展后的通用集合），视图层映射到主题词汇；
//! - 覆盖**有限、可后续迭代**：不做上下文语义（类型 / 变量作用域），只做词法层着色。
//!
//! ## 各语言覆盖范围
//!
//! | 语言 | 上色内容 | 明确不上色 |
//! | --- | --- | --- |
//! | Python | 关键字、字符串（含前缀与三引号）、`#` 注释、数字、函数启发式、运算符/标点 | 标识符、内置名（正文色） |
//! | R | 关键字、常量（TRUE/FALSE/NA…）、字符串、`#` 注释、数字、`%…%`/赋值运算符 | 标识符 |
//! | Rust | 关键字、字符串/raw 串/字节串/char、`//` 与**嵌套** `/* */` 注释、数字、生命周期、宏/函数启发式 | 类型上下文、普通标识符 |
//! | Markdown | ATX 标题、行内代码、围栏代码块标记、链接 URL、列表/引用标记 | 强调（正文色）、围栏内代码 |
//! | JSON | 字符串、**键**、数字、true/false、null、结构标点 | 裸词（非法 JSON） |
//! | Log | 级别词（分级）、时间戳、括号、key=value | 正文消息 |
//! | CSV | **无文本词法**（走表格展示，见 `crate::duckdb` / 编辑器后续接线） | 全部 |

pub mod json;
pub mod log;
pub mod markdown;
pub mod python;
pub mod rlang;
pub mod rust;

pub use json::json_spans;
pub use log::log_spans;
pub use markdown::markdown_spans;
pub use python::python_spans;
pub use rlang::r_spans;
pub use rust::rust_spans;
