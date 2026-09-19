//! sidecar 子进程的启动与回收
//!
//! 决策内核（`lifecycle`）只说「起一个实例 / 收掉一个实例」；落到真实进程的细节都在这里：
//! 三根管道怎么接、工作目录在哪、stderr 落哪、以及**退出时的回收顺序**。
//!
//! # 回收顺序（这一块的重点）
//!
//! 1. 先发 `shutdown` 通知（尽力而为，**不等回执**）；
//! 2. 丢掉连接 —— [`SidecarConn`] 持有 stdin 的写侧，丢掉它就等于关掉对端的 stdin；
//! 3. 对端把 stdin EOF 当作「宿主已死」并自行退出（协议级约定，`dev-plan` §4.2.1）；
//! 4. 宽限期内还没退的，强杀。
//!
//! 于是宿主的正常退出、崩溃、被任务管理器结束**都走同一条路**：管道一关，对端自己收场。
//! 不需要 Windows Job Object，也不需要 Linux `PDEATHSIG`（参考实现恰恰缺这一环，§3.5）。
//! 第 4 步只是兜底：**它被触发就说明对端没守约定**，所以要留痕（[`ReapOutcome::Forced`]）。
//!
//! # 为什么不做环境变量白名单
//!
//! sidecar 是原生进程，权限本就等同宿主（`plugin-dev-plan` §4.6.1 明写「❌ 原生进程权限等同宿主」）；
//! 清空环境不会让它更安全（它照样能读文件、开网络），却会让对端起不来 —— Windows 少了
//! `SystemRoot`，连 DNS 都解析不了。真正的信任闸在**安装期**（显式信任 + 包签名 + 来源标注）。
//! [`SpawnSpec::spawn_env`] 给的那几个变量是**事实交代**（告诉它自己是谁、私有目录在哪），不是限制。

use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};

use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::process::{Child, ChildStderr, Command};
use tokio::task::JoinHandle;

use super::conn::{ConnEvents, SidecarConn};

/// stderr 日志文件名（`plugin-cache/<id>/sidecar.log`；stdout 是协议通道，不进日志）。
pub const SIDECAR_LOG_NAME: &str = "sidecar.log";

/// 丢了 stdin 之后，给对方多久自己退。
pub const DEFAULT_REAP_GRACE: Duration = Duration::from_secs(5);

/// 强杀之后再等它被收掉的时限：`start_kill` 本身失败时不能把收摊流程挂死。
const FORCE_WAIT: Duration = Duration::from_secs(5);

/// 起一个实例需要知道的一切。
///
/// 决策内核不管这些（它只说「起 `plugin_id#index`」）；来源是插件清单的 `[backend]` 段
/// 与 [`SpawnSpec::new`] 拼出的几个路径。
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    /// 插件 id（会进到我们创建的路径里，起进程前先过白名单）。
    pub plugin_id: String,
    /// 实例序号（同插件的第几个实例）。
    pub index: usize,
    /// 可执行文件。
    pub program: PathBuf,
    /// 命令行参数。
    pub args: Vec<String>,
    /// 追加 / 覆盖的环境变量（**不是白名单**，见模块文档）。
    pub env: Vec<(String, String)>,
    /// 工作目录。
    pub current_dir: PathBuf,
    /// stderr 落这个文件。
    pub log_path: PathBuf,
}

impl SpawnSpec {
    /// 按插件 id 拼一份默认规格：目录一律来自 `paths::*`（不在这里手拼路径）。
    pub fn new(plugin_id: &str, index: usize, program: impl Into<PathBuf>) -> Self {
        Self {
            plugin_id: plugin_id.to_string(),
            index,
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            current_dir: paths::sidecar_work_dir(plugin_id),
            log_path: paths::plugin_cache_dir(plugin_id).join(SIDECAR_LOG_NAME),
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// 追加一个环境变量（后写的覆盖先写的，也覆盖继承来的同名变量）。
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// 交给子进程的环境变量：**继承宿主环境 + 下面这几个**。
    ///
    /// 不做白名单的理由见模块文档。给出去的是三件事实交代：
    /// 宿主数据根在哪、自己是谁、私有数据目录在哪 —— 对端据此知道该往哪写自己的东西，
    /// 而不是去猜 `~/.config`。
    pub fn spawn_env(&self) -> Vec<(String, String)> {
        let mut env = vec![
            ("RDS_HOME".to_string(), display(&paths::home())),
            ("RDS_PLUGIN_ID".to_string(), self.plugin_id.clone()),
            ("RDS_PLUGIN_INDEX".to_string(), self.index.to_string()),
            (
                "RDS_PLUGIN_DATA_DIR".to_string(),
                display(&paths::plugin_data_dir(&self.plugin_id)),
            ),
        ];
        env.extend(self.env.iter().cloned());
        env
    }
}

/// 起进程 / 收进程这一层的错误（**不是**协议错：对端还没说话就出的事）。
#[derive(Debug)]
pub enum ProcessError {
    /// 插件 id 不合法 —— 它会进到我们创建的路径里，必须先过白名单。
    BadPluginId { plugin_id: String },
    /// 工作目录 / 日志目录准备失败。
    Dir {
        path: PathBuf,
        source: std::io::Error,
    },
    /// 起进程失败（可执行文件不存在 / 没有执行权限 / 平台不匹配）。
    Launch {
        program: PathBuf,
        source: std::io::Error,
    },
    /// 进程起来了，但没拿到我们要的管道（stdio 是我们自己指定的，正常不会发生）。
    MissingPipe { pipe: &'static str },
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadPluginId { plugin_id } => {
                write!(f, "插件 id 不合法，不能用于起进程：{plugin_id:?}")
            }
            Self::Dir { path, source } => write!(f, "准备目录 {} 失败：{source}", path.display()),
            Self::Launch { program, source } => {
                write!(f, "启动 {} 失败：{source}", program.display())
            }
            Self::MissingPipe { pipe } => write!(f, "拿不到子进程的 {pipe} 管道"),
        }
    }
}

impl std::error::Error for ProcessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Dir { source, .. } | Self::Launch { source, .. } => Some(source),
            Self::BadPluginId { .. } | Self::MissingPipe { .. } => None,
        }
    }
}

/// 收摊的结局。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReapOutcome {
    /// 宽限期内自己退了 —— **正常路径**（我们丢了 stdin，它见 EOF 就退）。
    Exited {
        status: ExitStatus,
        /// 从发起收摊到它退出的耗时（诊断用）。
        after: Duration,
    },
    /// 宽限期内没退，被强杀。
    ///
    /// 说明对端**没守 EOF 约定**（`dev-plan` §4.2.1）：要记下来，不要当正常收场。
    Forced {
        pid: Option<u32>,
        /// 强杀之后的退出状态（拿不到就是 `None`，见 `tracing` 里的告警）。
        status: Option<ExitStatus>,
    },
}

impl ReapOutcome {
    /// 是不是「自己退的」（不管退出码）。
    pub fn exited_on_its_own(&self) -> bool {
        matches!(self, Self::Exited { .. })
    }

    /// 退出码（被打断 / 拿不到都是 `None`）。
    pub fn exit_code(&self) -> Option<i32> {
        match self {
            Self::Exited { status, .. } => status.code(),
            Self::Forced { status, .. } => status.as_ref().and_then(ExitStatus::code),
        }
    }

    /// 干净退出（自己退的 **且** 退出码为 0）。
    pub fn is_clean(&self) -> bool {
        matches!(self, Self::Exited { status, .. } if status.success())
    }
}

/// 一个真实存在的 sidecar 进程（不含连接）。
///
/// 单独成类型而不是塞进 [`SidecarProcess`]：它带着 [`Drop`] 兜底，
/// 而「先丢连接再收进程」这个顺序得在上一层显式写出来（见模块文档的回收顺序）。
pub struct SidecarChild {
    plugin_id: String,
    index: usize,
    child: Child,
    log_path: PathBuf,
    stderr_task: Option<JoinHandle<()>>,
}

impl SidecarChild {
    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn index(&self) -> usize {
        self.index
    }

    /// 进程 pid（已退出时可能还是旧值：`Child::id` 在 `wait` 之后返回 `None`）。
    pub fn pid(&self) -> Option<u32> {
        self.child.id()
    }

    /// stderr 日志的落点。
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// 它是不是已经退出了（不阻塞）。
    ///
    /// 崩溃检测**不能只靠这里**：管道断开才是第一手信号（对端可能死在任何时刻），
    /// 这个方法用来在断线之后补上「怎么死的」（退出码）。
    pub fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    /// 收掉它：等 `grace` → 还在就强杀。
    ///
    /// ⚠️ 调用方**必须先丢掉 [`SidecarConn`]**（那才关掉它的 stdin）。顺序反了也能收掉，
    /// 但走的是强杀分支 —— 而对端本来是可以自己退的。
    pub async fn reap(mut self, grace: Duration) -> ReapOutcome {
        let pid = self.child.id();
        let started = Instant::now();
        let outcome = match tokio::time::timeout(grace, self.child.wait()).await {
            Ok(Ok(status)) => ReapOutcome::Exited {
                status,
                after: started.elapsed(),
            },
            Ok(Err(e)) => {
                // `wait` 本身出错（子进程句柄异常）：状态不明，别赌它已经退了
                tracing::warn!(plugin_id = %self.plugin_id, index = self.index, error = %e,
                    "等待 sidecar 退出失败，按未退处理");
                self.force_kill(pid).await
            }
            Err(_) => {
                tracing::warn!(
                    plugin_id = %self.plugin_id, index = self.index, pid = ?pid,
                    grace_ms = grace.as_millis(),
                    "sidecar 未在宽限期内响应 stdin EOF，强制结束（对端没守 §4.2.1 的约定）"
                );
                self.force_kill(pid).await
            }
        };

        // 对端已退 → stderr 管道读完即 EOF；把尾巴写完再走
        if let Some(task) = self.stderr_task.take() {
            let _ = task.await;
        }
        outcome
    }

    /// 强杀并把它收掉。
    ///
    /// **杀完要再 `wait` 一次**：光杀不等，进程表里会留一条没人读退出码的记录；
    /// 而再等一次也要有时限 —— `start_kill` 失败时不能把收摊流程挂死。
    async fn force_kill(&mut self, pid: Option<u32>) -> ReapOutcome {
        if let Err(e) = self.child.start_kill() {
            tracing::warn!(pid = ?pid, error = %e, "强杀失败");
        }
        let status = match tokio::time::timeout(FORCE_WAIT, self.child.wait()).await {
            Ok(Ok(status)) => Some(status),
            Ok(Err(e)) => {
                tracing::warn!(pid = ?pid, error = %e, "强杀后等待退出失败");
                None
            }
            Err(_) => {
                tracing::warn!(pid = ?pid, "强杀后仍未在时限内退出");
                None
            }
        };
        ReapOutcome::Forced { pid, status }
    }
}

impl Drop for SidecarChild {
    fn drop(&mut self) {
        // 走到这里说明调用方没走 `reap`（忘了收 / 收摊路径本身在崩）。
        // `Drop` 不能 await，只能做「能立刻做的那件事」：还在跑就强杀。
        // 真正的孤儿防线不在这里 —— 而在「宿主一死，管道关闭，对端见 stdin EOF 自退」。
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            return;
        }
        tracing::warn!(
            plugin_id = %self.plugin_id, index = self.index,
            "sidecar 进程在 drop 时仍在运行，强制结束（调用方应走 SidecarProcess::retire）"
        );
        let _ = self.child.start_kill();
    }
}

/// 连接 + 进程：sidecar 在宿主侧的全部样子。
pub struct SidecarProcess {
    conn: SidecarConn,
    events: ConnEvents,
    child: SidecarChild,
}

impl SidecarProcess {
    /// 起一个 sidecar 进程，并把它的 stdout/stdin 接成一条 [`SidecarConn`]。
    ///
    /// 管道接法（宿主视角）：
    /// ```text
    /// 我们读 child.stdout  ← 对端写 stdout（协议帧）
    /// 我们写 child.stdin   → 对端读 stdin （协议帧；**它还兼着“宿主还在不在”的哨兵**）
    /// child.stderr        → plugin-cache/<id>/sidecar.log
    /// ```
    pub async fn spawn(spec: SpawnSpec) -> Result<Self, ProcessError> {
        paths::validate_plugin_id(&spec.plugin_id).map_err(|_| ProcessError::BadPluginId {
            plugin_id: spec.plugin_id.clone(),
        })?;
        prepare_dirs(&spec)?;

        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args)
            .current_dir(&spec.current_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // 不交给 tokio 在 drop 时静默杀：杀与不杀都要留痕（见 `SidecarChild::Drop`）
            .kill_on_drop(false);
        for (key, value) in spec.spawn_env() {
            cmd.env(key, value);
        }

        let mut child = cmd.spawn().map_err(|source| ProcessError::Launch {
            program: spec.program.clone(),
            source,
        })?;

        let pipes = (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let (Some(stdin), Some(stdout), Some(stderr)) = pipes else {
            // 半接好的进程不能丢下不管：`kill_on_drop(false)` 时它会活着留在系统里
            let _ = child.start_kill();
            return Err(ProcessError::MissingPipe {
                pipe: "stdin/stdout/stderr",
            });
        };

        let pid = child.id();
        let stderr_task = spawn_stderr_drain(
            stderr,
            &spec.plugin_id,
            spec.index,
            pid,
            spec.log_path.clone(),
        );
        let (conn, events) = SidecarConn::spawn(stdout, stdin);

        Ok(Self {
            conn,
            events,
            child: SidecarChild {
                plugin_id: spec.plugin_id,
                index: spec.index,
                child,
                log_path: spec.log_path,
                stderr_task: Some(stderr_task),
            },
        })
    }

    pub fn conn(&self) -> &SidecarConn {
        &self.conn
    }

    /// 连接上的异步事件（通知 / 错位 / 断开）。
    pub fn events(&mut self) -> &mut ConnEvents {
        &mut self.events
    }

    pub fn child(&mut self) -> &mut SidecarChild {
        &mut self.child
    }

    pub fn plugin_id(&self) -> &str {
        self.child.plugin_id()
    }

    pub fn index(&self) -> usize {
        self.child.index()
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.pid()
    }

    /// stderr 日志的落点（`plugin-cache/<id>/sidecar.log`）。
    pub fn log_path(&self) -> &Path {
        self.child.log_path()
    }

    /// 收摊，按模块文档里那四步来。
    ///
    /// 消耗 `self` 是刻意的：收完就不该再有人握着这条连接（`conn` 的引用会被借用检查挡住）。
    pub async fn retire(self, grace: Duration) -> ReapOutcome {
        // 允许：`SidecarProcess` 自身没有 `Drop`（有的话字段就移不出来了）
        let Self { conn, child, .. } = self;
        conn.shutdown().await;
        drop(conn); // ← 就是这一步关掉对端的 stdin
        child.reap(grace).await
    }
}

/// 把 stderr 抽干并落到日志文件。
///
/// **打不开日志也要继续抽**：管道缓冲一满，对端就会卡在「写日志」上再也不处理请求 ——
/// 一个坏掉的日志目录不该让插件假死。
fn spawn_stderr_drain(
    stderr: ChildStderr,
    plugin_id: &str,
    index: usize,
    pid: Option<u32>,
    log_path: PathBuf,
) -> JoinHandle<()> {
    let plugin_id = plugin_id.to_string();
    let pid = pid.map_or_else(|| "?".to_string(), |p| p.to_string());
    tokio::spawn(async move {
        let mut stderr = stderr;
        let mut sink: Box<dyn tokio::io::AsyncWrite + Unpin + Send> = match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .await
        {
            Ok(file) => Box::new(file),
            Err(e) => {
                tracing::warn!(
                    path = %log_path.display(), error = %e,
                    "sidecar stderr 日志打不开，这一路的输出将丢弃"
                );
                Box::new(tokio::io::sink())
            }
        };

        // 每次起的进程在日志里留个抬头，否则多次启动的 stderr 会连成一片
        let header = format!(
            "--- {}#{} pid={} 启动于 {} ---\n",
            plugin_id,
            index,
            pid,
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        );
        let _ = sink.write_all(header.as_bytes()).await;

        let mut buf = vec![0u8; 8 * 1024];
        loop {
            match stderr.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if sink.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = sink.flush().await;
    })
}

fn prepare_dirs(spec: &SpawnSpec) -> Result<(), ProcessError> {
    std::fs::create_dir_all(&spec.current_dir).map_err(|source| ProcessError::Dir {
        path: spec.current_dir.clone(),
        source,
    })?;
    if let Some(parent) = spec.log_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ProcessError::Dir {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    Ok(())
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_directories_come_from_paths() {
        let spec = SpawnSpec::new("example.demo", 2, "/plugins/example.demo/agent");
        assert_eq!(spec.current_dir, paths::sidecar_work_dir("example.demo"));
        assert_eq!(
            spec.log_path,
            paths::plugin_cache_dir("example.demo").join(SIDECAR_LOG_NAME)
        );
        assert_eq!(spec.index, 2);
    }

    #[test]
    fn spawn_env_tells_the_child_who_it_is() {
        let spec = SpawnSpec::new("example.demo", 1, "agent");
        let env: std::collections::HashMap<_, _> = spec.spawn_env().into_iter().collect();

        assert_eq!(env["RDS_HOME"], display(&paths::home()));
        assert_eq!(env["RDS_PLUGIN_ID"], "example.demo");
        assert_eq!(env["RDS_PLUGIN_INDEX"], "1");
        assert_eq!(
            env["RDS_PLUGIN_DATA_DIR"],
            display(&paths::plugin_data_dir("example.demo"))
        );
    }

    /// 调用方给的变量写在后面 —— `Command::env` 按顺序生效，后写的赢。
    #[test]
    fn caller_env_comes_after_the_builtin_ones() {
        let spec = SpawnSpec::new("example.demo", 0, "agent")
            .with_env("RDS_PLUGIN_ID", "覆盖掉了")
            .with_env("MY_OWN", "1");
        let env = spec.spawn_env();
        let last_id = env
            .iter()
            .filter(|(k, _)| k == "RDS_PLUGIN_ID")
            .next_back()
            .expect("应有 RDS_PLUGIN_ID");
        assert_eq!(last_id.1, "覆盖掉了");
        assert!(env.contains(&("MY_OWN".to_string(), "1".to_string())));
    }

    #[test]
    fn reap_outcome_helpers() {
        use std::process::ExitStatus;

        #[cfg(unix)]
        fn status(code: i32) -> ExitStatus {
            use std::os::unix::process::ExitStatusExt;
            ExitStatus::from_raw(code << 8)
        }
        #[cfg(windows)]
        fn status(code: i32) -> ExitStatus {
            use std::os::windows::process::ExitStatusExt;
            ExitStatus::from_raw(code as u32)
        }

        let clean = ReapOutcome::Exited {
            status: status(0),
            after: Duration::from_millis(3),
        };
        assert!(clean.exited_on_its_own() && clean.is_clean());
        assert_eq!(clean.exit_code(), Some(0));

        let crash = ReapOutcome::Exited {
            status: status(3),
            after: Duration::from_millis(3),
        };
        assert!(crash.exited_on_its_own() && !crash.is_clean());
        assert_eq!(crash.exit_code(), Some(3));

        let forced = ReapOutcome::Forced {
            pid: Some(1),
            status: None,
        };
        assert!(!forced.exited_on_its_own() && !forced.is_clean());
        assert_eq!(forced.exit_code(), None);
    }
}
