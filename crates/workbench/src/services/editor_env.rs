//! 【2b】通用模式的执行环境（工作台实现）：系统解释器探测 + 脚本进程执行
//!
//! 编辑器只表达意图（[`editor::env::EnvPort`] 回答「有哪些环境、装没装」，
//! [`editor::execution::QueryRunner::run_script`] 回答「怎么跑」），这里是工作台侧的
//! 真实现：
//!
//! ## 探测（只发生在 attach 时）
//!
//! [`SystemEnvPort::new`] 对内置解释器（python / Rscript / rustc）各问一次 `--version`，
//! 结果缓存进内存。**渲染路径（`list` / `installed`）只读内存，绝不做 I/O**——GPUI-kit
//! 纪律：render 不能有副作用。没装 / 命令不可达 = 如实报未安装（运行按钮置灰 + 原因）。
//!
//! ## 执行（在工作线程上被调用，可以阻塞）
//!
//! [`run_script_process`] 用 tokio 把进程跑起来：代码经 **stdin** 写入（不走 `-c` 参数——
//! Windows 引号坑 + 命令行长度限制），stdout / stderr 并发收尽，超时（[`SCRIPT_TIMEOUT_MS`]）
//! 就杀**进程树**（Windows `taskkill /T /F`，否则解释器留下的子进程会残留）。
//!
//! 自定义环境（用户配置命令 / 参数）是 T2b-3「管理环境」的事；届时把 [`BUILTIN_ENVS`]
//! 换成可注册的表即可，端口与执行入口不变。

use std::collections::HashMap;
use std::process::Stdio;
use std::rc::Rc;

use editor::env::{EnvKind, EnvPort, Environment, ScriptOutcome};
use editor::shared::EditorShared;
use tokio::io::AsyncWriteExt as _;

/// 内置解释器的探测 / 执行清单：`(id, 类型, 候选命令（按优先级）, 默认参数)`
///
/// 候选命令解决 Windows 的 python 别名问题：`python` 可能是 Store 桩（`--version`
/// 退出码 49），这时回退 `py`。探测按优先级取第一个能用的；执行也用同一个
/// （写进 [`resolved_envs`] 缓存）。参数进**执行**不走命令行（代码走 stdin），这些是
/// 解释器自身的运行参数（`-u` = python 无缓冲；`--vanilla` = R 不带用户环境，跑脚本更可预期）。
const BUILTIN_ENVS: &[(&str, EnvKind, &[&str], &[&str])] = &[
    ("python", EnvKind::Python, &["python", "py"], &["-u"]),
    ("rscript", EnvKind::RScript, &["Rscript"], &["--vanilla"]),
    ("rustc", EnvKind::Rustc, &["rustc"], &[]),
];

/// 解析好的环境（执行器用）：命令是**探测成功的那一个**（如 python 桩 → 回退 py）
pub struct ResolvedEnv {
    /// 实际可用的命令（探测成功的候选）
    pub command: &'static str,
    /// 默认参数（进执行，不走命令行）
    pub args: Vec<String>,
}

/// 探测结果 memo（attach 时写入；`run_script` 执行时读取）
///
/// 为什么要全局：环境端口（[`SystemEnvPort`]，editor 侧渲染用）与执行器
/// （`EngineQueryRunner::run_script`，工作线程）各自持自己的句柄，中间隔着 editor 的
/// shared 状态；同一个「探测一次」的结果两边都要读，就放这里。只写一次、只读，
/// 无并发写（attach 发生在启动装配，单线程）。
static RESOLVED_ENVS: std::sync::OnceLock<std::sync::Mutex<HashMap<String, ResolvedEnv>>> =
    std::sync::OnceLock::new();

/// 脚本执行的超时（毫秒）：防止 `while True: pass` 这类脚本把执行线程永远占住。
///
/// MVP 先给一个保守默认；T2b-3「管理环境」落地后由环境配置暴露。到点杀**进程树**，
/// 结论如实带 `timed_out = true`（退出码是杀进程给的，不是脚本自己退的）。
pub const SCRIPT_TIMEOUT_MS: u64 = 60_000;

/// 环境端口的工作台实现（探测结果缓存进内存，渲染路径零 I/O）
pub struct SystemEnvPort {
    envs: Vec<Environment>,
    installed: HashMap<String, bool>,
}

impl SystemEnvPort {
    /// 探测一次并缓存（**启动装配时调用**：I/O 只在这一步发生）
    pub fn new() -> Self {
        let mut envs = Vec::with_capacity(BUILTIN_ENVS.len());
        let mut installed = HashMap::new();
        for (id, kind, candidates, args) in BUILTIN_ENVS {
            // 按优先级探测候选命令：第一个能报出版本的 = 实际可用命令（Windows 的
            // `python` Store 桩会失败，回退 `py`）。没装 = 命令不可达 / 非零退出。
            let probe = candidates
                .iter()
                .find_map(|candidate| probe_version(candidate).map(|v| (*candidate, v)));
            let version = probe.as_ref().map(|(_, v)| v.clone());
            installed.insert(id.to_string(), probe.is_some());
            if let Some((command, _)) = probe {
                remember_resolved(id, command, args);
            }
            envs.push(Environment {
                id: id.to_string(),
                name: kind.label().to_string(),
                kind: *kind,
                command: candidates[0].to_string(),
                args: args.iter().map(|s| s.to_string()).collect(),
                version,
            });
        }
        Self { envs, installed }
    }
}

/// 把探测成功的命令写进 memo（执行器 `resolve` 时读）
fn remember_resolved(id: &str, command: &'static str, args: &[&str]) {
    let map = RESOLVED_ENVS.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    if let Ok(mut map) = map.lock() {
        map.insert(
            id.to_string(),
            ResolvedEnv {
                command,
                args: args.iter().map(|s| s.to_string()).collect(),
            },
        );
    }
}

impl EnvPort for SystemEnvPort {
    fn list(&self) -> Vec<Environment> {
        self.envs.clone()
    }

    fn installed(&self, id: &str) -> bool {
        self.installed.get(id).copied().unwrap_or(false)
    }
}

/// 注入环境端口（**启动装配调用一次**；探测失败 = 该环境如实报未安装，不 panic）
pub fn attach(editor: &EditorShared) {
    editor.attach_env(Rc::new(SystemEnvPort::new()));
}

/// 解析环境定义（命令 + 参数；未探测 / 未注册 = 如实拒绝，不猜）
///
/// 执行器（`EngineQueryRunner::run_script`）用它把 `env_id` 翻译成可执行命令。
/// 命令是**探测成功的那一个**（见 [`SystemEnvPort::new`]）：python Store 桩回退 py，
/// 执行就用 py，而不是明知会失败的 python。
pub fn resolve(env_id: &str) -> Result<ResolvedEnv, String> {
    if let Some(map) = RESOLVED_ENVS.get() {
        if let Ok(map) = map.lock() {
            if let Some(resolved) = map.get(env_id) {
                return Ok(ResolvedEnv {
                    command: resolved.command,
                    args: resolved.args.clone(),
                });
            }
        }
    }
    Err(format!("执行环境不可用：{env_id}（未安装或未探测）"))
}

/// 脚本执行的核心（async）：spawn + stdin 写代码 + 并发收输出 + 超时杀进程树
///
/// 调用方（执行器的专用 runtime）`block_on` 它；编辑器保证它在**工作线程**上被调用，
/// 阻塞无妨。
pub async fn run_script_process(
    command: &str,
    args: &[String],
    code: &str,
    timeout_ms: u64,
) -> Result<ScriptOutcome, String> {
    let mut child = tokio::process::Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("spawn 失败：{command} —— {error}"))?;

    // 代码经 stdin 写入（写完即关 → 子进程读到 EOF）。不走 `-c`：Windows 引号坑 +
    // 命令行长度限制，长脚本会直接炸。
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or("无法获取子进程 stdin（spawn 后即被占用）")?;
        stdin
            .write_all(code.as_bytes())
            .await
            .map_err(|error| format!("写入代码失败：{error}"))?;
    }

    let mut stdout = child
        .stdout
        .take()
        .ok_or("无法获取子进程 stdout")?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or("无法获取子进程 stderr")?;
    let mut out_buf = Vec::new();
    let mut err_buf = Vec::new();

    // 并发收尽 stdout / stderr，同时等退出（只 `wait` 不收输出会让管道填满 → 子进程
    // 永远写不完 → 假死到超时被杀，输出还丢了）。超时到点：`timeout` 返回 `Err` 时
    // 内层 future 已被 drop（上面的 `&mut` 借用随之释放），这里才能自由地杀进程树
    // 并补读残余输出（进程已死 → EOF 立即返回）。
    let (out, err, status, timed_out) = match tokio::time::timeout(
        std::time::Duration::from_millis(timeout_ms),
        async {
            let _ = tokio::join!(
                tokio::io::copy(&mut stdout, &mut out_buf),
                tokio::io::copy(&mut stderr, &mut err_buf),
            );
            child.wait().await
        },
    )
    .await
    {
        Ok(status) => {
            let status =
                status.map_err(|error| format!("等待子进程失败：{error}"))?;
            (out_buf, err_buf, Some(status), false)
        }
        Err(_elapsed) => {
            // 超时：杀进程树（Windows `taskkill /T /F`——光杀解释器，它 spawn 的子进程会残留）
            let pid = child.id().unwrap_or(0);
            if pid != 0 {
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .status();
            }
            let _ = child.wait().await;
            let _ = tokio::join!(
                tokio::io::copy(&mut stdout, &mut out_buf),
                tokio::io::copy(&mut stderr, &mut err_buf),
            );
            (out_buf, err_buf, None, true)
        }
    };

    Ok(ScriptOutcome {
        exit_code: status.as_ref().and_then(|status| status.code()),
        stdout: String::from_utf8_lossy(&out).into_owned(),
        stderr: String::from_utf8_lossy(&err).into_owned(),
        timed_out,
    })
}

/// 解析解释器 `--version` 输出里的版本号（纯函数，可穷举）
///
/// 取第一行里的第一个以数字开头的词：`Python 3.12.1` → `Some("3.12.1")`；
/// `rustc 1.75.0 (…)` → `Some("1.75.0")`；`R scripting front-end version 4.3.2 (…)`
/// → `Some("4.3.2")`；没有数字词 / 空行 → `None`。
fn parse_version_line(line: &str) -> Option<String> {
    line.split_whitespace()
        .find(|word| {
            word.chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        })
        .map(str::to_string)
}

/// 探测解释器版本（`command --version`；一次进程往返，只在构造期发生）
///
/// 输出取第一行交给 [`parse_version_line`]。命令不可达 / 非零退出
/// （如 Windows Store 的 python 桩退出码 49）= `None`（如实报未安装）。
fn probe_version(command: &str) -> Option<String> {
    let output = std::process::Command::new(command)
        .arg("--version")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    // Rscript 的 `--version` 走 stderr；两路都看，别跟具体实现较劲
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    text.lines().next().and_then(parse_version_line)
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**（父模块引入了 gpui 相关符号，`use super::*` 有风险）
    use super::{BUILTIN_ENVS, RESOLVED_ENVS, parse_version_line, remember_resolved, resolve};

    #[test]
    fn version_line_parses_the_first_numeric_word() {
        assert_eq!(parse_version_line("Python 3.12.1"), Some("3.12.1".to_string()));
        assert_eq!(
            parse_version_line("rustc 1.75.0 (0f16a21 2023-12-27)"),
            Some("1.75.0".to_string())
        );
        assert_eq!(
            parse_version_line("R scripting front-end version 4.3.2 (2023-10-31)"),
            Some("4.3.2".to_string())
        );
    }

    #[test]
    fn version_line_without_a_number_is_none() {
        assert_eq!(parse_version_line(""), None);
        assert_eq!(parse_version_line("command not found"), None);
    }

    #[test]
    fn resolve_returns_what_probing_remembered() {
        // 不依赖真机解释器（跨机器不稳）：直接写 memo → resolve 读回，命令原样
        remember_resolved("python", "py", &["-u"]);
        remember_resolved("rustc", "rustc", &[]);
        let resolved = resolve("python").expect("memo 里有的应能解析");
        assert_eq!(resolved.command, "py", "探测回退的命令要原样给执行器");
        assert_eq!(resolved.args, vec!["-u"]);

        let resolved = resolve("rustc").expect("memo 里有的应能解析");
        assert_eq!(resolved.command, "rustc");

        // 没探测过的环境如实拒绝
        assert!(resolve("not-a-real-env").is_err());
    }

    #[test]
    fn builtin_list_has_all_kinds_with_candidates() {
        // 清单是端口与执行器的共同事实：每个 id 都有类型、候选命令（≥1）、默认参数
        for (id, kind, candidates, _args) in BUILTIN_ENVS {
            assert!(!id.is_empty());
            assert!(!candidates.is_empty(), "{id} 至少要有一个候选命令");
            let _ = kind; // 类型标签在枚举里穷举过（env 模块的单测覆盖）
        }
        // 测试会污染全局 memo，清理掉，避免影响其它用例
        if let Some(map) = RESOLVED_ENVS.get() {
            if let Ok(mut map) = map.lock() {
                map.clear();
            }
        }
    }
}
