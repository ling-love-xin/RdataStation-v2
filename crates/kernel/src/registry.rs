//! 内核注册表（#20 / #21）：可用内核的**探测与快照**
//!
//! ## 为什么要有注册表
//!
//! #17 之前的实现是「一个 python 路径走天下」：`RDS_KERNEL_PYTHON` 或 `python`，
//! 分析模式 Python 单元只能跑在系统解释器上，R / Julia 单元更是占位。注册表把
//! 「本机有哪些内核可用」变成一份可渲染的快照：
//!
//! - **探测只在 attach 时做一次**（后台线程，不阻塞 UI），渲染路径只读内存快照；
//! - 快照来源分两路，**kernelspec 优先**：
//!   1. **Jupyter kernelspec**（#21）：按 Jupyter 的标准数据目录扫 `kernel.json`，
//!      所以装过 IRkernel / IJulia / 别的内核的环境会被直接认出来——与用户机器上
//!      已有的 Jupyter 共用同一份注册信息，不重复维护一份「本产品专用内核清单」；
//!   2. **旧探测兜底**：没有任何 kernelspec 覆盖某语言时，仍按 `py` / `python` /
//!      `Rscript` / `julia` 探测解释器，让「没装过 Jupyter 的机器」也能用 Python 单元。
//! - 两路都拿**绝对路径**启动（`sys.executable` / kernelspec 的 argv），不再靠 PATH
//!   猜——曾实测 `py` 会解析到 Store 桩另一套解释器，单元执行因此跑到错的 python 上。
//!
//! ## 与 editor 侧 `KernelsPort` 的关系
//!
//! 本模块是**纯数据 + 纯函数**（不依赖 gpui）：editor 侧定义 `KernelsPort` trait、
//! workbench 宿主把 [`detect_kernels`] 的结果注进去，editor 只消费快照。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use jupyter_protocol::JupyterKernelspec;

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

/// 内核的中断方式（kernelspec 的 `interrupt_mode`）
///
/// 缺省 / 未声明按 `Message`：Python 与大多数内核支持 control 通道的
/// `InterruptRequest`；R（IRkernel）声明 `signal`，需要给进程发 SIGINT。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelInterruptMode {
    /// 走内核协议（control 通道 InterruptRequest）
    Message,
    /// 走进程信号（Windows 上没有标准 SIGINT 投递方式，见 `session` 的说明）
    Signal,
}

/// 一个内核的启动参数（kernelspec 的 argv 模板 + env + 中断方式）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelLaunch {
    /// 启动参数模板；`{connection_file}` 由会话层替换成真实路径
    pub argv: Vec<String>,
    /// kernelspec 声明的额外环境变量（按 key 排序，保证快照可比较）
    pub env: Vec<(String, String)>,
    pub interrupt_mode: KernelInterruptMode,
}

impl KernelLaunch {
    /// ipykernel 直启模板（没有 kernelspec 时的兜底，等价 #20 的旧行为）
    ///
    /// 与 Jupyter 生成的 `python3/kernel.json` 同一形状，只是 `argv[0]` 一定是绝对路径。
    pub fn ipykernel(python: &str) -> Self {
        Self {
            argv: vec![
                python.to_string(),
                "-m".to_string(),
                "ipykernel_launcher".to_string(),
                "-f".to_string(),
                "{connection_file}".to_string(),
            ],
            env: Vec::new(),
            interrupt_mode: KernelInterruptMode::Message,
        }
    }

    /// 占位内核（解释器在、但内核包不在）：没有可执行 argv，只用于菜单置灰
    pub fn placeholder() -> Self {
        Self {
            argv: Vec::new(),
            env: Vec::new(),
            interrupt_mode: KernelInterruptMode::Message,
        }
    }

    /// 模板是否可直接启动（有 argv 且带 `{connection_file}` 占位）
    pub fn is_launchable(&self) -> bool {
        !self.argv.is_empty()
            && self
                .argv
                .iter()
                .any(|arg| arg.as_str() == "{connection_file}")
    }
}

/// 一个可用（或占位）内核的注册表条目
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelSpec {
    /// 稳定 id（菜单选中 / `ExecutionSource::Kernel(id)` / 会话默认内核都用它）
    ///
    /// 来自 kernelspec 的条目 id 形如 `ks:python3`（带命名空间，避免与旧探测的
    /// `python` / `python-venv` 撞号）；旧探测条目沿用 #20 的 id。
    pub id: String,
    /// 界面展示名（kernelspec 的 `display_name`，或「环境名 + 版本」）
    pub display_name: String,
    pub language: KernelLanguage,
    /// 主命令（= `launch.argv[0]`；UI / 日志展示用）
    pub command: String,
    /// 是否可执行（kernelspec 命令存在 / Python 装了 ipykernel）
    pub ready: bool,
    /// 不可用原因（`ready == false` 时菜单行尾展示）
    pub note: Option<&'static str>,
    /// 启动参数（按 Jupyter kernelspec 的 argv 模板）
    pub launch: KernelLaunch,
}

/// 一个 python 解释器的探测结果（一次子进程拿齐三项，避免多次起进程）
#[derive(Debug, Clone, PartialEq, Eq)]
struct PythonProbe {
    /// `sys.executable`（绝对路径；启动内核用它，不靠 PATH）
    exe: PathBuf,
    /// `sys.prefix`（venv 根 / 独立安装根；kernelspec 目录要靠它定位）
    prefix: PathBuf,
    has_ipykernel: bool,
}

/// 扫到的一条 kernelspec（还没转成 `KernelSpec`）
struct FoundSpec {
    name: String,
    data_dir: PathBuf,
    kernelspec: JupyterKernelspec,
}

/// 探测候选：`(id, 展示名, 语言, 候选命令（按优先级）, 不可用原因)`
///
/// 候选命令解决 Windows 的 python 别名问题（`python` 可能是 Store 桩，
/// `--version` 退出码 49）：显式 venv 优先，`py` / `python` 兜底。
///
/// R / Julia 只探测解释器存在（内核包 IRkernel / IJulia 不代装）：`ready = false`
/// 并给原因，菜单里置灰——用户装了内核包，kernelspec 那一路就会把它点亮。
const KERNEL_CANDIDATES: &[(&str, &str, KernelLanguage, &[&str], &'static str)] = &[
    (
        "python-venv",
        "Python (venv)",
        KernelLanguage::Python,
        &[], /* env 指定，动态填 */
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
        "未安装 R / 未装 IRkernel",
    ),
    (
        "julia",
        "Julia",
        KernelLanguage::Julia,
        &["julia"],
        "未安装 Julia / 未装 IJulia",
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

/// 探测一个 python 解释器：一次子进程拿 `sys.executable` / `sys.prefix` / ipykernel 是否装上
///
/// `find_spec` 只查已安装的分布，不真 import——首次 import 会编译，可能几百毫秒。
fn probe_python(command: &str) -> Option<PythonProbe> {
    const PROBE: &str = "import sys,importlib.util;print(sys.executable);print(sys.prefix);print(importlib.util.find_spec('ipykernel') is not None)";
    let output = std::process::Command::new(command)
        .arg("-c")
        .arg(PROBE)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let mut lines = text.lines();
    let exe = PathBuf::from(lines.next()?.trim());
    let prefix = PathBuf::from(lines.next()?.trim());
    let has_ipykernel = lines
        .next()
        .map(|line| line.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if exe.as_os_str().is_empty() {
        return None;
    }
    Some(PythonProbe {
        exe,
        prefix,
        has_ipykernel,
    })
}

/// 路径里是否有分隔符（无分隔符 = 裸名，要按 PATH 解析或按环境的 bin 目录补全）
fn has_path_separator(text: &str) -> bool {
    text.contains('/') || text.contains('\\')
}

/// `<prefix>/share/jupyter` → `<prefix>`（kernelspec 的数据目录反推所属环境根）
fn prefix_of_data_dir(data_dir: &Path) -> Option<PathBuf> {
    let share = data_dir.parent()?;
    if share.file_name()?.to_str()? != "share" {
        return None;
    }
    Some(share.parent()?.to_path_buf())
}

/// 在某个环境的 bin 目录里找裸名可执行文件（Windows = `Scripts`，其余 = `bin`）
fn find_env_executable(prefix: &Path, bare: &str) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if cfg!(windows) {
        candidates.push(prefix.join("Scripts").join(format!("{bare}.exe")));
        candidates.push(prefix.join("Scripts").join(bare));
    } else {
        candidates.push(prefix.join("bin").join(bare));
    }
    candidates.into_iter().find(|path| path.is_file())
}

/// `jupyter --paths --json` 报的 data 目录（覆盖「venv 装了 kernelspec 但不在 PATH 上」）
///
/// 与 `jupyter-zmq-client` 的 `dirs::ask_jupyter()` 同一机制，只是这里是同步调用
/// （探测本来就在后台线程跑，多一个几十毫秒的子进程可接受）。`jupyter` 不可用、
/// 退出码非 0、输出不是合法 JSON → 返回空表静默回退（不因为缺 jupyter 让注册表变空）。
fn jupyter_data_dirs() -> Vec<PathBuf> {
    let output = std::process::Command::new("jupyter")
        .args(["--paths", "--json"])
        .output()
        .ok();
    let Some(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return Vec::new();
    };
    value
        .get("data")
        .and_then(|entry| entry.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

/// 去重追加（保持先出现的目录优先）
fn push_unique(dirs: &mut Vec<PathBuf>, dir: PathBuf) {
    if !dirs.contains(&dir) {
        dirs.push(dir);
    }
}

/// kernelspec 的搜索目录（顺序 = 优先级，同名先到先得）
///
/// 与 Jupyter 的解析顺序对齐：**环境自己的 `<prefix>/share/jupyter` 最高**，
/// 然后 `jupyter --paths --json` 报的 data 目录、`JUPYTER_PATH`（用户显式指定）、
/// 用户级 `%APPDATA%\jupyter`、系统级 `%PROGRAMDATA%\jupyter`。
///
/// 注意：`jupyter-zmq-client` 的 `dirs::data_dirs()` **不含** `<prefix>/share/jupyter`
/// （其源码里明确留着 TODO「Use the sys.prefix from python」），所以 venv 里装的
/// kernelspec（`python -m ipykernel install --sys-prefix` 的产物）必须由这里补上，
/// 否则「venv 装了 ipykernel 却看不见」。`jupyter` 命令是最通用的一条补齐方式：
/// 它自己就知道所在环境的 `sys.prefix`。
fn kernelspec_data_dirs(prefixes: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    for prefix in prefixes {
        push_unique(&mut dirs, prefix.join("share").join("jupyter"));
    }

    for dir in jupyter_data_dirs() {
        push_unique(&mut dirs, dir);
    }

    if let Some(path) = std::env::var_os("JUPYTER_PATH") {
        for entry in std::env::split_paths(&path) {
            push_unique(&mut dirs, entry);
        }
    }

    if let Some(appdata) = std::env::var_os("APPDATA") {
        push_unique(&mut dirs, PathBuf::from(appdata).join("jupyter"));
    }
    if let Some(program_data) = std::env::var_os("PROGRAMDATA") {
        push_unique(&mut dirs, PathBuf::from(program_data).join("jupyter"));
    }

    dirs
}

/// 扫一遍 kernelspec 目录（同步、只读 `kernel.json`）
///
/// 容错口径：读不到 / 解析不了的目录与 spec 一律跳过（不因为一个坏 spec 让整份
/// 注册表为空）；同名 spec 以**先出现的目录**为准，与 Jupyter 一致。
fn scan_kernelspecs(dirs: &[PathBuf]) -> Vec<FoundSpec> {
    let mut out: Vec<FoundSpec> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for dir in dirs {
        let kernels = dir.join("kernels");
        let Ok(entries) = std::fs::read_dir(&kernels) else {
            continue;
        };
        // 目录遍历顺序不保证稳定 → 先按名字排序，快照才可复现
        let mut names: Vec<String> = entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().to_str().map(|name| name.to_string()))
            .collect();
        names.sort();

        for name in names {
            if seen.contains(&name) {
                continue;
            }
            let path = kernels.join(&name).join("kernel.json");
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok(kernelspec) = serde_json::from_slice::<JupyterKernelspec>(&bytes) else {
                continue;
            };
            seen.insert(name.clone());
            out.push(FoundSpec {
                name,
                data_dir: dir.clone(),
                kernelspec,
            });
        }
    }

    out
}

/// 把一条 kernelspec 转成注册表条目（不认的语言 / 不合契约的 spec 返回 `None`）
fn spec_from_kernelspec(found: &FoundSpec) -> Option<KernelSpec> {
    let language = match found.kernelspec.language.to_ascii_lowercase().as_str() {
        "python" => KernelLanguage::Python,
        "r" => KernelLanguage::R,
        "julia" => KernelLanguage::Julia,
        // 没有对应单元类型的语言（rust / scala / …）不摆进菜单：选了也没有单元能用
        _ => return None,
    };

    let mut argv = found.kernelspec.argv.clone();
    if argv.is_empty() || !argv.iter().any(|arg| arg.as_str() == "{connection_file}") {
        return None; // 不符合 Jupyter 契约（没带连接文件占位）的 spec 启动必失败
    }

    // 裸名（venv 生成的 kernel.json 常见写法，如 `"python"`）：用该 kernelspec 所属
    // 环境的解释器绝对化。否则会解析到 PATH 上另一个解释器——实测这条会把内核跑到
    // 系统 3.14 桩上，venv 里的 ipykernel 根本用不上。
    if !has_path_separator(&argv[0]) {
        if let Some(prefix) = prefix_of_data_dir(&found.data_dir) {
            if let Some(exe) = find_env_executable(&prefix, &argv[0]) {
                argv[0] = exe.display().to_string();
            }
        }
    }

    // 绝对路径不存在 = 这个内核实际装不上（环境被删 / 挪走），如实置灰
    let ready = has_path_separator(&argv[0]) && Path::new(&argv[0]).is_file();
    let note = if ready {
        None
    } else {
        Some("内核启动命令不存在（环境可能已删除）")
    };

    let env: Vec<(String, String)> = found
        .kernelspec
        .env
        .as_ref()
        .map(|map| {
            let mut pairs: Vec<(String, String)> =
                map.iter().map(|(key, value)| (key.clone(), value.clone())).collect();
            pairs.sort();
            pairs
        })
        .unwrap_or_default();

    let interrupt_mode = match found.kernelspec.interrupt_mode.as_deref() {
        Some("signal") => KernelInterruptMode::Signal,
        _ => KernelInterruptMode::Message,
    };

    Some(KernelSpec {
        id: format!("ks:{}", found.name),
        display_name: found.kernelspec.display_name.clone(),
        language,
        command: argv[0].clone(),
        ready,
        note,
        launch: KernelLaunch {
            argv,
            env,
            interrupt_mode,
        },
    })
}

/// 旧探测口径的 python 条目（绝对路径启动，`ready` 取决于是否装了 ipykernel）
fn python_spec(id: &str, display: &str, probe: &PythonProbe) -> KernelSpec {
    let command = probe.exe.display().to_string();
    let (ready, note) = if probe.has_ipykernel {
        (true, None)
    } else {
        (false, Some("未装 ipykernel（python -m pip install ipykernel）"))
    };
    KernelSpec {
        id: id.to_string(),
        display_name: display.to_string(),
        language: KernelLanguage::Python,
        command: command.clone(),
        ready,
        note,
        launch: KernelLaunch::ipykernel(&command),
    }
}

/// 占位条目（解释器在、内核包不在）：菜单置灰 + 原因
fn placeholder_spec(
    id: &str,
    display: &str,
    language: KernelLanguage,
    note: &'static str,
    command: String,
) -> KernelSpec {
    KernelSpec {
        id: id.to_string(),
        display_name: display.to_string(),
        language,
        command,
        ready: false,
        note: Some(note),
        launch: KernelLaunch::placeholder(),
    }
}

/// 探测当前机器可用的内核快照（**纯函数**：只做进程探测与目录读取，不碰 UI / 网络）
///
/// 顺序 = 优先序：
/// 1. Jupyter kernelspec（venv 的 `<prefix>/share/jupyter` 最先）；
/// 2. 该语言没有可用 kernelspec 时，补旧探测条目（Python / R / Julia 占位）。
pub fn detect_kernels() -> Vec<KernelSpec> {
    // 1) 先探 python：既产出兜底条目，也给出 kernelspec 目录要用的 `sys.prefix`
    let mut pythons: Vec<KernelSpec> = Vec::new();
    let mut prefixes: Vec<PathBuf> = Vec::new();

    if let Some(venv) = std::env::var_os("RDS_KERNEL_PYTHON") {
        let command = PathBuf::from(venv).display().to_string();
        match probe_python(&command) {
            Some(probe) => {
                prefixes.push(probe.prefix.clone());
                pythons.push(python_spec("python-venv", "Python (venv)", &probe));
            }
            None => pythons.push(placeholder_spec(
                "python-venv",
                "Python (venv)",
                KernelLanguage::Python,
                "venv 解释器不可用",
                command,
            )),
        }
    }

    // 系统 python：按优先级取第一个能跑的（`py` 优先，绕开 Store 的 `python` 桩）
    let system = KERNEL_CANDIDATES[1]
        .3
        .iter()
        .copied()
        .find_map(|command| probe_python(command).map(|probe| (command, probe)));
    if let Some((_command, probe)) = system {
        prefixes.push(probe.prefix.clone());
        pythons.push(python_spec("python", "Python (系统)", &probe));
    }

    // 2) kernelspec 优先
    let dirs = kernelspec_data_dirs(&prefixes);
    let found = scan_kernelspecs(&dirs);
    let mut specs: Vec<KernelSpec> = found.iter().filter_map(spec_from_kernelspec).collect();

    let has_ready = |specs: &[KernelSpec], language: KernelLanguage| {
        specs
            .iter()
            .any(|spec| spec.language == language && spec.ready)
    };

    // 3) 该语言没有可用 kernelspec → 补旧探测（菜单不重复，也不会选到装不上的死项）
    if !has_ready(&specs, KernelLanguage::Python) {
        specs.extend(pythons);
    }

    // 4) R / Julia：没有 kernelspec 时按旧口径占位（解释器在 → 置灰 + 原因）
    for (id, name, language, commands, note) in KERNEL_CANDIDATES.iter().skip(2) {
        if has_ready(&specs, *language) {
            continue;
        }
        if commands.iter().any(|command| command_available(command)) {
            specs.push(placeholder_spec(
                id,
                name,
                *language,
                note,
                commands[0].to_string(),
            ));
        }
    }

    specs
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

    /// 无 `RDS_KERNEL_PYTHON` 时，Python 一定以某种形式出现（kernelspec 或旧探测）
    ///
    /// 有 kernelspec 时旧条目会被让位（避免重复），所以这里只断言「有语言为 Python
    /// 的条目，且字段完整」——两种来源都满足。
    #[test]
    fn python_shows_up_from_kernelspec_or_probe() {
        let specs = detect_kernels();
        // 诊断用（默认被测试框架捕获；`--nocapture` 时能看到本机真实快照）
        eprintln!("detect_kernels → {specs:#?}");
        let pythons: Vec<&KernelSpec> = specs
            .iter()
            .filter(|spec| spec.language == KernelLanguage::Python)
            .collect();
        if pythons.is_empty() {
            // 本机既没有 kernelspec 也没有可用 python：这是环境事实，不是实现错误
            return;
        }
        for spec in pythons {
            assert!(!spec.command.is_empty(), "{spec:?}");
            if spec.ready {
                assert!(spec.launch.is_launchable(), "{spec:?}");
            }
        }
    }

    /// 条目字段契约：id / 展示名 / 命令非空，ready=false 必有原因，ready=true 无原因
    #[test]
    fn every_spec_has_its_fields_filled() {
        for spec in detect_kernels() {
            assert!(!spec.id.is_empty(), "{spec:?}");
            assert!(!spec.display_name.is_empty(), "{spec:?}");
            assert!(!spec.command.is_empty(), "{spec:?}");
            if spec.ready {
                assert!(spec.note.is_none(), "可用内核不该给原因：{spec:?}");
                assert!(spec.launch.is_launchable(), "可用内核必须可启动：{spec:?}");
            } else {
                assert!(spec.note.is_some(), "不可用内核必须给原因：{spec:?}");
            }
        }
    }

    /// kernelspec 解析：语言映射 / argv 模板 / 裸名绝对化 / 异构语言跳过 / 同名去重
    #[test]
    fn kernelspec_json_becomes_a_spec_and_bare_command_is_absolutized() {
        let root = std::env::temp_dir().join(format!(
            "rds-ks-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let data_dir = root.join("share").join("jupyter");
        let kernels = data_dir.join("kernels");

        // 造两个 spec：py3（裸名 python，应被绝对化）+ rust（无对应单元语言，应跳过）
        std::fs::create_dir_all(kernels.join("py3")).unwrap();
        std::fs::write(
            kernels.join("py3").join("kernel.json"),
            r#"{"argv":["python","-m","ipykernel_launcher","-f","{connection_file}"],
                "display_name":"Python 3 (test)","language":"python"}"#,
        )
        .unwrap();
        std::fs::create_dir_all(kernels.join("rust")).unwrap();
        std::fs::write(
            kernels.join("rust").join("kernel.json"),
            r#"{"argv":["evcxr_jupyter","--control_file","{connection_file}"],
                "display_name":"Rust","language":"rust"}"#,
        )
        .unwrap();

        // 环境的 bin 目录里放一个假的 python（验证裸名绝对化）
        let scripts = root.join("Scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(scripts.join("python.exe"), b"").unwrap();

        let found = scan_kernelspecs(&[data_dir.clone()]);
        assert_eq!(found.len(), 2, "两个 spec 都应被扫到（rust 在转换阶段才丢）");
        let specs: Vec<KernelSpec> = found.iter().filter_map(spec_from_kernelspec).collect();
        assert_eq!(specs.len(), 1, "rust 没有对应单元语言，不该进注册表");
        let spec = &specs[0];
        assert_eq!(spec.id, "ks:py3");
        assert_eq!(spec.language, KernelLanguage::Python);
        // 裸名 `python` 被替换成该环境 Scripts\python.exe 的绝对路径
        assert!(
            spec.launch.argv[0].contains("Scripts"),
            "裸名未被绝对化：{:?}",
            spec.launch.argv
        );
        assert!(spec.launch.is_launchable(), "{:?}", spec.launch);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 同名 spec：先出现的目录胜（与 Jupyter 的 data dir 优先级一致）
    #[test]
    fn same_kernel_name_first_directory_wins() {
        let root = std::env::temp_dir().join(format!(
            "rds-ks-dup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let first = root.join("a");
        let second = root.join("b");
        for (dir, display) in [(&first, "First"), (&second, "Second")] {
            std::fs::create_dir_all(dir.join("kernels").join("ir")).unwrap();
            std::fs::write(
                dir.join("kernels").join("ir").join("kernel.json"),
                format!(
                    r#"{{"argv":["R","--slave","-e","IRkernel::main()","--args","{{connection_file}}"],
                        "display_name":"{display}","language":"R"}}"#
                ),
            )
            .unwrap();
        }

        let found = scan_kernelspecs(&[first.clone(), second.clone()]);
        assert_eq!(found.len(), 1, "同名只应保留一条");
        assert_eq!(found[0].kernelspec.display_name, "First");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 不合 Jupyter 契约（argv 没带 `{connection_file}`）的 spec 不认
    #[test]
    fn kernelspec_without_connection_placeholder_is_rejected() {
        let found = FoundSpec {
            name: "broken".to_string(),
            data_dir: PathBuf::from("/nowhere/share/jupyter"),
            kernelspec: serde_json::from_str(
                r#"{"argv":["python","-m","ipykernel_launcher"],"display_name":"Broken","language":"python"}"#,
            )
            .unwrap(),
        };
        assert!(spec_from_kernelspec(&found).is_none());
    }
}
