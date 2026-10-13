//! 通用模式的执行环境（外部解释器）模型与端口
//!
//! 通用模式 = 无连接档：文件编辑 + **本地脚本执行**。脚本要跑，得先有一个"环境"——
//! 一个外部解释器（Python / R / Rustc / 自定义命令）。本模块定义环境的数据形状与
//! 宿主注入端口（[`EnvPort`]），不碰进程本身——**怎么把进程跑起来**是执行器
//! （`QueryRunner::run_script`）的事，这里只回答"有哪些环境、装没装"。
//!
//! 端口口径与全仓一致：**未注入 = 空环境列表**（如实，不假装探测过）；缺解释器时
//! 运行不亮并说明原因（原型通用模式交互）。

use std::rc::Rc;

/// 解释器类型（决定默认命令 / 展示标签；自定义 = 用户自己填命令）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvKind {
    Python,
    RScript,
    Rustc,
    Custom,
}

impl EnvKind {
    /// 界面展示名
    pub fn label(self) -> &'static str {
        match self {
            EnvKind::Python => "Python",
            EnvKind::RScript => "R",
            EnvKind::Rustc => "Rust",
            EnvKind::Custom => "自定义",
        }
    }

    /// 持久化键（存库 / 存配置；展示名可以改，存库的值不能）
    pub fn as_key(self) -> &'static str {
        match self {
            EnvKind::Python => "python",
            EnvKind::RScript => "rscript",
            EnvKind::Rustc => "rustc",
            EnvKind::Custom => "custom",
        }
    }

    /// 从持久化键还原（认不出来的值 = 自定义，不 panic）
    pub fn from_key(key: &str) -> Self {
        match key {
            "python" => EnvKind::Python,
            "rscript" => EnvKind::RScript,
            "rustc" => EnvKind::Rustc,
            _ => EnvKind::Custom,
        }
    }
}

/// 一个执行环境（外部解释器）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    /// 稳定 id（跨会话引用；不是展示名）
    pub id: String,
    /// 展示名（如「Python 3.12」）
    pub name: String,
    /// 解释器类型
    pub kind: EnvKind,
    /// 可执行命令（`python` / `Rscript` / `rustc` / 绝对路径）
    pub command: String,
    /// 附加参数（如 `-u`；脚本代码经 stdin 传入，不走参数，避免引号坑）
    pub args: Vec<String>,
    /// 探测到的版本（`Some("3.12.1")`）；`None` = 没探测到
    pub version: Option<String>,
}

impl Environment {
    /// 环境选择器里的展示文案：`名字`（+ 版本，有就带）
    pub fn display_name(&self) -> String {
        match &self.version {
            Some(v) => format!("{} {}", self.name, v),
            None => self.name.clone(),
        }
    }
}

/// 一次脚本执行请求（编辑器只表达意图：用哪个环境 + 跑什么代码）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRequest {
    /// 目标环境的 id（执行器拿它去 [环境目录 / 已注册环境] 里找命令）
    pub env_id: String,
    /// 要跑的代码（原样，不预包装）
    pub code: String,
}

/// 一次脚本执行的结论（执行器给的真实值）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptOutcome {
    /// 退出码（进程被超时杀掉 = 仍然有退出码；spawn 失败走 Err 不带这个）
    pub exit_code: Option<i32>,
    /// stdout 全文（可能为空）
    pub stdout: String,
    /// stderr 全文（可能为空）
    pub stderr: String,
    /// 是否因超时被中断（退出码是杀进程给的，不是脚本自己退的）
    pub timed_out: bool,
}

/// 脚本执行的结论（回到主线程；与 `ExecOutcome` 分开——脚本结果不是网格形状）
#[derive(Debug, Clone)]
pub struct ScriptNote {
    pub document: crate::model::DocumentId,
    /// 目标环境 id（宿主渲染时经 `EnvPort` 解析展示名；环境已不在列表就回退用 id）
    pub env_id: String,
    /// 执行结果（`Err` = 失败原因，含 spawn 失败 / 未实现）
    pub result: Result<ScriptOutcome, String>,
}

/// 环境端口：宿主注入"有哪些环境 / 装没装"
///
/// 与 `CompletionPort` 同款口径：editor 不依赖任何具体探测逻辑（系统命令探测、
/// 配置目录、还是假数据，都归宿主决定；单测注入假实现）。
pub trait EnvPort: 'static {
    /// 列出可用环境（未注入 = 空列表，如实）
    fn list(&self) -> Vec<Environment>;

    /// 该环境是否已安装（命令可达；缺解释器 = 运行不亮的原因）
    fn installed(&self, id: &str) -> bool;
}

/// 环境端口的句柄（宿主注入）
pub type EnvHandle = Rc<dyn EnvPort>;

/// 环境选择器的当前选择（持久化到文档：与 connection 同类）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvSelection {
    pub env_id: String,
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**（父模块引入了 gpui 相关符号，`use super::*` 有风险）
    use super::{EnvKind, Environment};

    #[test]
    fn env_kind_keys_round_trip_and_are_not_the_display_names() {
        // 展示名与持久化键分开：键是存库的值，展示名可以随时改文案
        assert_eq!(EnvKind::Python.as_key(), "python");
        assert_eq!(EnvKind::Python.label(), "Python");
        assert_eq!(EnvKind::RScript.as_key(), "rscript");
        assert_eq!(EnvKind::Rustc.as_key(), "rustc");
        assert_eq!(EnvKind::Custom.as_key(), "custom");
        assert_eq!(EnvKind::from_key("python"), EnvKind::Python);
        assert_eq!(EnvKind::from_key("rscript"), EnvKind::RScript);
        assert_eq!(EnvKind::from_key("rustc"), EnvKind::Rustc);
        // 认不出来的值 = 自定义（不 panic，不猜）
        assert_eq!(EnvKind::from_key("wat"), EnvKind::Custom);
    }

    #[test]
    fn display_name_includes_version_when_present() {
        let plain = Environment {
            id: "py".to_string(),
            name: "Python".to_string(),
            kind: EnvKind::Python,
            command: "python".to_string(),
            args: vec![],
            version: None,
        };
        assert_eq!(plain.display_name(), "Python");

        let versioned = Environment {
            version: Some("3.12.1".to_string()),
            ..plain.clone()
        };
        assert_eq!(versioned.display_name(), "Python 3.12.1");
    }
}
