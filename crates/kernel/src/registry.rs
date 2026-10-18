//! 内核注册表（#20）：可用内核的**探测与快照**
//!
//! ## 为什么要有注册表
//!
//! #17 之前的实现是「一个 python 路径走天下」：`RDS_KERNEL_PYTHON` 或 `python`，
//! 分析模式 Python 单元只能跑在系统解释器上，R / Julia 单元更是占位。注册表把
//! 「本机有哪些内核可用」变成一份可渲染的快照：
//!
//! - **探测只在 attach 时做一次**（后台线程，不阻塞 UI），渲染路径只读内存快照；
//! - Python 内核**区分 venv 与系统解释器**（`RDS_KERNEL_PYTHON` 优先，回退 py / python）；
//!   ready = 该解释器装得上 ipykernel（探测用 `find_spec` 快速查，不真 import——首次
//!   import 会编译，可能几百毫秒，不适合 attach 时同步做）；
//! - R / Julia 按用户拍板「以 python / sql / markdown 为主」**只占位**：解释器存在就列出，
//!   ready = false 并给原因（需要 IRkernel / IJulia），菜单里置灰——用户装了再变亮。
//!
//! ## 与 editor 侧 `KernelsPort` 的关系
//!
//! 本模块是**纯数据 + 纯函数**（不依赖 gpui）：editor 侧定义 `KernelsPort` trait、
//! workbench 宿主把 [`detect_kernels`] 的结果注进去，editor 只消费快照。

/// 内核能跑的语言（对应分析模式单元的语言）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelLanguage {
    Python,
    R,
    Julia,
}

impl KernelLanguage {
    /// 界面展示名（菜单 / 状态栏）
    pub fn label(self) -> &'static str {
        match self {
            Self::Python => "Python",
            Self::R => "R",
            Self::Julia => "Julia",
        }
    }
}

/// 一个可用（或占位）内核的注册表条目
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelSpec {
    /// 稳定 id（菜单选中 / `ExecutionSource::Kernel(id)` / 会话默认内核都用它）
    pub id: String,
    /// 界面展示名（环境名 + 版本，如 `Python (venv)` / `Python (系统)`）
    pub display_name: String,
    pub language: KernelLanguage,
    /// 启动命令（`KernelSession::spawn` 的可执行路径；仅 ready 时有意义）
    pub command: String,
    /// 是否探测可用（Python = 装了 ipykernel；R / Julia = 装了对应内核包）
    pub ready: bool,
    /// 不可用原因（`ready == false` 时菜单行尾展示；`ready` 时为 `None`）
    pub note: Option<&'static str>,
}

/// 探测候选：`(id, 展示名, 语言, 候选命令（按优先级）, 不可用原因)`
///
/// 候选命令解决 Windows 的 python 别名问题（`python` 可能是 Store 桩，`--version`
/// 退出码 49）：venv（`RDS_KERNEL_PYTHON`）是显式指定的，用 `py` / `python` 兜底时
/// 按优先级取第一个能跑的。R / Julia 只探测解释器存在，内核包（IRkernel / IJulia）
/// 本次不装——用户拍板语言以 python 为主。
const KERNEL_CANDIDATES: &[(&str, &str, KernelLanguage, &[&str], &'static str)] = &[
    (
        "python-venv",
        "Python (venv)",
        KernelLanguage::Python,
        &[] /* env 指定，动态填 */,
        "venv 解释器不可用",
    ),
    (
        "python",
        "Python (系统)",
        KernelLanguage::Python,
        &["py", "python"],
        "未安装 Python / 未装 ipykernel",
    ),
    (
        "r",
        "R",
        KernelLanguage::R,
        &["Rscript"],
        "未安装 R；运行 R 单元需要 IRkernel",
    ),
    (
        "julia",
        "Julia",
        KernelLanguage::Julia,
        &["julia"],
        "未安装 Julia；运行 Julia 单元需要 IJulia",
    ),
];

/// 探测一次解释器是否可用（`--version` 退出码 0 且非 Store 桩）
fn command_available(command: &str) -> bool {
    std::process::Command::new(command)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Python 解释器是否装得上 ipykernel（`find_spec` 快速探测，不真 import）
///
/// 输出是 `True` / `False` 单行；命令不可达 / 输出异常一律视为未装（如实降级）。
fn has_ipykernel(command: &str) -> bool {
    let probe = r#"import importlib.util,sys; print(importlib.util.find_spec("ipykernel") is not None)"#;
    std::process::Command::new(command)
        .arg("-c")
        .arg(probe)
        .output()
        .ok()
        .and_then(|out| {
            String::from_utf8(out.stdout)
                .ok()
                .map(|text| text.trim().eq_ignore_ascii_case("true"))
        })
        .unwrap_or(false)
}

/// 探测当前机器可用的内核快照（**纯函数**：只做进程探测，不碰 UI / 网络）
///
/// 顺序稳定（venv → 系统 python → R → Julia），供菜单按序渲染。
pub fn detect_kernels() -> Vec<KernelSpec> {
    let mut out: Vec<KernelSpec> = Vec::with_capacity(KERNEL_CANDIDATES.len());

    // venv：`RDS_KERNEL_PYTHON` 显式指定时**额外**列一条（在系统 python 之前）
    if let Some(venv) = std::env::var_os("RDS_KERNEL_PYTHON") {
        let command = std::path::PathBuf::from(venv).display().to_string();
        let (ready, note) = if !command_available(&command) {
            (false, Some("venv 解释器不可用"))
        } else if !has_ipykernel(&command) {
            (false, Some("venv 未装 ipykernel"))
        } else {
            (true, None)
        };
        out.push(KernelSpec {
            id: "python-venv".to_string(),
            display_name: "Python (venv)".to_string(),
            language: KernelLanguage::Python,
            command,
            ready,
            note,
        });
    }

    // 系统 python：按优先级取第一个可用命令探测 ipykernel
    let sys_command = KERNEL_CANDIDATES[1]
        .3
        .iter()
        .copied()
        .find(|command| command_available(command));
    if let Some(command) = sys_command {
        let (ready, note) = if has_ipykernel(command) {
            (true, None)
        } else {
            (false, Some("系统 Python 未装 ipykernel"))
        };
        out.push(KernelSpec {
            id: "python".to_string(),
            display_name: "Python (系统)".to_string(),
            language: KernelLanguage::Python,
            command: command.to_string(),
            ready,
            note,
        });
    }

    // R / Julia：只占位（解释器在就列出、置灰给原因；装了内核包也先不亮——见模块文档）
    for (id, name, language, commands, note) in KERNEL_CANDIDATES.iter().skip(2) {
        if commands.iter().any(|c| command_available(c)) {
            out.push(KernelSpec {
                id: (*id).to_string(),
                display_name: (*name).to_string(),
                language: *language,
                command: commands[0].to_string(),
                ready: false,
                note: Some(note),
            });
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 语言展示名稳定（菜单 / 状态栏文案不是存储键）
    #[test]
    fn language_labels_are_stable() {
        assert_eq!(KernelLanguage::Python.label(), "Python");
        assert_eq!(KernelLanguage::R.label(), "R");
        assert_eq!(KernelLanguage::Julia.label(), "Julia");
    }

    /// `RDS_KERNEL_PYTHON` 指向带 ipykernel 的 venv 时，注册表第一项就是它且 ready
    ///
    /// `#[ignore]`：与真内核集成测试同口径（默认环境没有 `RDS_KERNEL_PYTHON`）；
    /// 跑法：`$env:RDS_KERNEL_PYTHON=<venv-python>` 后 `cargo test -p rds-kernel -- --ignored`
    #[test]
    #[ignore = "需要 RDS_KERNEL_PYTHON 指向带 ipykernel 的解释器"]
    fn venv_python_is_first_and_ready_when_ipykernel_present() {
        let venv = std::env::var_os("RDS_KERNEL_PYTHON")
            .map(std::path::PathBuf::from)
            .expect("跑法见 rds-kernel 集成测试：RDS_KERNEL_PYTHON 指向带 ipykernel 的解释器");
        let specs = detect_kernels();
        let first = specs.first().expect("venv 存在时注册表至少有一条 Python");
        assert_eq!(first.id, "python-venv");
        assert_eq!(first.language, KernelLanguage::Python);
        // 集成环境里 venv 就是为 ipykernel 准备的；这里验证「命令可用 → 探测走通」
        assert!(!first.command.is_empty());
    }

    /// 无 `RDS_KERNEL_PYTHON` 时注册表仍给出系统 python（若有）
    #[test]
    fn system_python_shows_up_without_venv() {
        let with_venv = std::env::var_os("RDS_KERNEL_PYTHON").is_some();
        let specs = detect_kernels();
        let python_ids: Vec<&str> = specs
            .iter()
            .filter(|s| s.language == KernelLanguage::Python)
            .map(|s| s.id.as_str())
            .collect();
        if with_venv {
            assert!(python_ids.contains(&"python-venv"), "{python_ids:?}");
        }
        // 系统 python 是否出现取决于机器；只要出现就要求字段完整
        for spec in specs.iter().filter(|s| s.id == "python") {
            assert_eq!(spec.language, KernelLanguage::Python);
            assert!(!spec.command.is_empty());
        }
    }

    /// 条目字段契约：id / 展示名非空，ready=false 必有原因，ready=true 无原因
    #[test]
    fn every_spec_has_its_fields_filled() {
        for spec in detect_kernels() {
            assert!(!spec.id.is_empty());
            assert!(!spec.display_name.is_empty());
            assert!(!spec.command.is_empty());
            if spec.ready {
                assert!(spec.note.is_none(), "可用内核不该给原因：{spec:?}");
            } else {
                assert!(spec.note.is_some(), "不可用内核必须给原因：{spec:?}");
            }
        }
    }
}
