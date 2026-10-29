//! 【2b】通用模式的执行环境（工作台实现）：系统解释器探测 + 脚本进程执行 + 管理环境
//!
//! 编辑器只表达意图（[`crate::env::EnvPort`] 回答「有哪些环境、装没装、能不能改」，
//! [`crate::execution::QueryRunner::run_script`] 回答「怎么跑」），这里是工作台侧的
//! 真实现：
//!
//! ## 探测（只发生在 attach 与增删改时）
//!
//! [`SystemEnvPort::new`] 对内置解释器（python / Rscript / rustc）各问一次 `--version`，
//! 再把**用户自定义环境**（T2b-3 管理环境：全局库 `script_environments` 表）读进来一起
//! 探测。结果缓存进内存。**渲染路径（`list` / `installed`）只读内存，绝不做 I/O**——
//! GPUI-kit 纪律：render 不能有副作用。没装 / 命令不可达 = 如实报未安装（运行按钮置灰 + 原因）。
//!
//! ## 管理（T2b-3：增删改路径 / 参数）
//!
//! 管理环境 modal（editor 侧 `view/dialogs.rs`）经 [`EnvPort`] 的
//! `create` / `update` / `delete` 调到这里：写全局库 + 刷新内存缓存 + 同步执行器 memo
//! （[`RESOLVED_ENVS`]）。内置环境是系统探测的，**只读**——modal 里不提供增删改。
//!
//! ## 执行（在工作线程上被调用，可以阻塞）
//!
//! [`run_script_process`] 用 tokio 把进程跑起来：代码经 **stdin** 写入（不走 `-c` 参数——
//! Windows 引号坑 + 命令行长度限制），stdout / stderr 并发收尽，超时（[`SCRIPT_TIMEOUT_MS`]）
//! 就杀**进程树**（Windows `taskkill /T /F`，否则解释器留下的子进程会残留）。

use std::cell::RefCell;
use std::collections::HashMap;
use std::process::Stdio;
use std::rc::Rc;

use crate::env::{EnvKind, EnvPort, Environment, ScriptOutcome};
use crate::shared::EditorShared;
use engine::persistence::script_env_store::{ScriptEnvRow, ScriptEnvStore};
use tokio::io::AsyncWriteExt as _;

/// 内置解释器的探测 / 执行清单：`(id, 类型, 候选命令（按优先级）, 默认参数)`
///
/// 候选命令解决 Windows 的 python 别名问题：`python` 可能是 Store 桩（`--version`
/// 退出码 49），这时回退 `py`。探测按优先级取第一个能用的；执行也用同一个
/// （写进 [`RESOLVED_ENVS`] 缓存）。参数进**执行**不走命令行（代码走 stdin），这些是
/// 解释器自身的运行参数（`-u` = python 无缓冲；`--vanilla` = R 不带用户环境，跑脚本更可预期）。
const BUILTIN_ENVS: &[(&str, EnvKind, &[&str], &[&str])] = &[
    ("python", EnvKind::Python, &["python", "py"], &["-u"]),
    ("rscript", EnvKind::RScript, &["Rscript"], &["--vanilla"]),
    ("rustc", EnvKind::Rustc, &["rustc"], &[]),
];

/// 解析好的环境（执行器用）：命令是**探测成功的那一个**（如 python 桩 → 回退 py；
/// 自定义环境 = 用户填的路径）。owned `String`：自定义环境的命令不是编译期常量。
#[derive(Debug, Clone)]
pub struct ResolvedEnv {
    /// 实际可用的命令（探测成功的候选 / 用户配置的路径）
    pub command: String,
    /// 默认参数（进执行，不走命令行）
    pub args: Vec<String>,
}

/// 探测结果 memo（attach 与增删改时写入；`run_script` 执行时读取）
///
/// 为什么要全局：环境端口（[`SystemEnvPort`]，editor 侧渲染用）与执行器
/// （`EngineQueryRunner::run_script`，工作线程）各自持自己的句柄，中间隔着 editor 的
/// shared 状态；同一个「探测一次」的结果两边都要读，就放这里。写发生在 attach（启动装配）
/// 与增删改（GPUI 主线程事件路径），读发生在工作线程——Mutex 防并发写。
static RESOLVED_ENVS: std::sync::OnceLock<std::sync::Mutex<HashMap<String, ResolvedEnv>>> =
    std::sync::OnceLock::new();

/// 脚本执行的超时（毫秒）：防止 `while True: pass` 这类脚本把执行线程永远占住。
///
/// MVP 先给一个保守默认；T2b-3「管理环境」落地后仍用这个默认（超时可配列为后续项）。
/// 到点杀**进程树**，结论如实带 `timed_out = true`（退出码是杀进程给的，不是脚本自己退的）。
pub const SCRIPT_TIMEOUT_MS: u64 = 60_000;

/// 环境端口的工作台实现（探测结果缓存进内存，渲染路径零 I/O）
///
/// `store` = 全局库的脚本环境配置表（`None` = 全局库未就绪 → 管理能力如实拒绝，
/// 但内置探测与脚本执行不受影响）。
pub struct SystemEnvPort {
    envs: RefCell<Vec<Environment>>,
    installed: RefCell<HashMap<String, bool>>,
    store: Option<ScriptEnvStore>,
}

impl SystemEnvPort {
    /// 探测一次并缓存（**启动装配时调用**：I/O 只在这一步发生）
    pub fn new() -> Self {
        let store = engine::migration::get_global_db_manager()
            .and_then(|manager| manager.get_script_env_store().ok());
        Self::with_store(store)
    }

    /// 接一份给定的存储（测试用：临时库，不碰全局单例）
    pub fn with_store(store: Option<ScriptEnvStore>) -> Self {
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
                remember_resolved(id, command, &args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            }
            envs.push(Environment {
                id: id.to_string(),
                name: kind.label().to_string(),
                kind: *kind,
                command: candidates[0].to_string(),
                args: args.iter().map(|s| s.to_string()).collect(),
                version,
                // 内置环境 = 系统探测，管理环境 modal 不提供增删改
                readonly: true,
            });
        }

        // 自定义环境：从配置表读进来（名字 / 命令 / 参数是用户填的），一样探测版本。
        // 全局库没就绪（store = None）→ 自定义环境列表为空（如实，不假装记得）。
        if let Some(store) = &store {
            if let Ok(rows) = store.list() {
                for row in rows {
                    let kind = EnvKind::from_key(&row.kind);
                    let version = probe_version(&row.command);
                    installed.insert(row.id.clone(), version.is_some());
                    remember_resolved(&row.id, &row.command, &row.args);
                    envs.push(Environment {
                        id: row.id.clone(),
                        name: row.name.clone(),
                        kind,
                        command: row.command.clone(),
                        args: row.args.clone(),
                        version,
                        readonly: false,
                    });
                }
            }
        }

        Self {
            envs: RefCell::new(envs),
            installed: RefCell::new(installed),
            store,
        }
    }

    /// 增删改后的公共收口：改内存缓存 + 执行器 memo，保证渲染与执行读到的是一份
    fn upsert_memory(&self, env: &Environment) {
        let mut envs = self.envs.borrow_mut();
        if let Some(existing) = envs.iter_mut().find(|e| e.id == env.id) {
            *existing = env.clone();
        } else {
            envs.push(env.clone());
        }
        drop(envs);
        self.installed
            .borrow_mut()
            .insert(env.id.clone(), env.version.is_some());
        remember_resolved(&env.id, &env.command, &env.args);
    }

    fn drop_memory(&self, id: &str) {
        self.envs.borrow_mut().retain(|env| env.id != id);
        self.installed.borrow_mut().remove(id);
        if let Some(map) = RESOLVED_ENVS.get() {
            if let Ok(mut map) = map.lock() {
                map.remove(id);
            }
        }
    }
}

/// 把探测成功的命令写进 memo（执行器 `resolve` 时读）
fn remember_resolved(id: &str, command: &str, args: &[String]) {
    let map = RESOLVED_ENVS.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    if let Ok(mut map) = map.lock() {
        map.insert(
            id.to_string(),
            ResolvedEnv {
                command: command.to_string(),
                args: args.to_vec(),
            },
        );
    }
}

impl EnvPort for SystemEnvPort {
    fn list(&self) -> Vec<Environment> {
        self.envs.borrow().clone()
    }

    fn installed(&self, id: &str) -> bool {
        self.installed.borrow().get(id).copied().unwrap_or(false)
    }

    /// 新增自定义环境：写全局库 → 探测版本 → 刷新内存与 memo → 返回落定后的环境
    fn create(
        &self,
        name: &str,
        kind: EnvKind,
        command: &str,
        args: &[String],
    ) -> Result<Environment, String> {
        let name = name.trim();
        let command = command.trim();
        if name.is_empty() {
            return Err("环境名称不能为空".to_string());
        }
        if command.is_empty() {
            return Err("命令不能为空".to_string());
        }
        let store = self
            .store
            .as_ref()
            .ok_or("环境配置存储未接入（全局库未就绪）")?;
        let id = format!("custom-{}", uuid::Uuid::new_v4());
        let version = probe_version(command);
        store
            .create(&ScriptEnvRow {
                id: id.clone(),
                name: name.to_string(),
                kind: kind.as_key().to_string(),
                command: command.to_string(),
                args: args.to_vec(),
                created_at: now_ms(),
            })
            .map_err(|e| format!("保存环境失败：{e}"))?;
        let env = Environment {
            id: id.clone(),
            name: name.to_string(),
            kind,
            command: command.to_string(),
            args: args.to_vec(),
            version,
            readonly: false,
        };
        self.upsert_memory(&env);
        Ok(env)
    }

    /// 更新自定义环境：写全局库 → 重新探测版本 → 刷新内存与 memo
    fn update(
        &self,
        id: &str,
        name: &str,
        kind: EnvKind,
        command: &str,
        args: &[String],
    ) -> Result<Environment, String> {
        let name = name.trim();
        let command = command.trim();
        if name.is_empty() {
            return Err("环境名称不能为空".to_string());
        }
        if command.is_empty() {
            return Err("命令不能为空".to_string());
        }
        // 内置环境只读：改它 = 改系统探测逻辑，不在管理环境 modal 的范围
        let current = self
            .envs
            .borrow()
            .iter()
            .find(|env| env.id == id)
            .cloned()
            .ok_or("环境不存在（可能已被删除）")?;
        if current.readonly {
            return Err("内置环境是系统探测的，不能在这里改".to_string());
        }
        let store = self
            .store
            .as_ref()
            .ok_or("环境配置存储未接入（全局库未就绪）")?;
        let version = probe_version(command);
        store
            .update(&ScriptEnvRow {
                id: id.to_string(),
                name: name.to_string(),
                kind: kind.as_key().to_string(),
                command: command.to_string(),
                args: args.to_vec(),
                // UPDATE 不碰 created_at 列（建行时间不该被改），这里传什么都可以
                created_at: now_ms(),
            })
            .map_err(|e| format!("保存环境失败：{e}"))?;
        let env = Environment {
            id: id.to_string(),
            name: name.to_string(),
            kind,
            command: command.to_string(),
            args: args.to_vec(),
            version,
            readonly: false,
        };
        self.upsert_memory(&env);
        Ok(env)
    }

    /// 删除自定义环境：写全局库 → 清内存与 memo。内置环境只读，拒绝。
    fn delete(&self, id: &str) -> Result<(), String> {
        let current = self
            .envs
            .borrow()
            .iter()
            .find(|env| env.id == id)
            .cloned()
            .ok_or("环境不存在（可能已被删除）")?;
        if current.readonly {
            return Err("内置环境是系统探测的，不能删除".to_string());
        }
        let store = self
            .store
            .as_ref()
            .ok_or("环境配置存储未接入（全局库未就绪）")?;
        store
            .delete(id)
            .map_err(|e| format!("删除环境失败：{e}"))?;
        self.drop_memory(id);
        Ok(())
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
/// 执行就用 py，而不是明知会失败的 python；自定义环境 = 用户填的路径。
pub fn resolve(env_id: &str) -> Result<ResolvedEnv, String> {
    if let Some(map) = RESOLVED_ENVS.get() {
        if let Ok(map) = map.lock() {
            if let Some(resolved) = map.get(env_id) {
                return Ok(ResolvedEnv {
                    command: resolved.command.clone(),
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

/// 探测解释器版本（`command --version`；一次进程往返，只在构造与增删改时发生）
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

fn now_ms() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**（父模块引入了 gpui 相关符号，`use super::*` 有风险）
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::env::EnvPort;
    use engine::persistence::global_db::GlobalSqlitePool;
    use engine::persistence::script_env_store::ScriptEnvStore;

    use super::{BUILTIN_ENVS, RESOLVED_ENVS, parse_version_line, remember_resolved, resolve, SystemEnvPort};

    fn temp_db(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rds_sysenv_{tag}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir.join("global.db")
    }

    /// 建池 + 开店（建池在自建 runtime 里，开店在离开 async 上下文之后）
    fn open_store(db: &PathBuf) -> ScriptEnvStore {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let pool = runtime.block_on(async {
            Arc::new(GlobalSqlitePool::new(db.clone(), 1).await.expect("create pool"))
        });
        drop(runtime);
        ScriptEnvStore::new(pool).expect("open store")
    }

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
        remember_resolved("python", "py", &["-u".to_string()]);
        remember_resolved("rustc", "rustc", &[]);
        let resolved = resolve("python").expect("memo 里有的应能解析");
        assert_eq!(resolved.command, "py", "探测回退的命令要原样给执行器");
        assert_eq!(resolved.args, vec!["-u"]);

        let resolved = resolve("rustc").expect("memo 里有的应能解析");
        assert_eq!(resolved.command, "rustc");

        // 没探测过的环境如实拒绝
        assert!(resolve("not-a-real-env").is_err());
        // 清理 memo，避免污染其它用例
        if let Some(map) = RESOLVED_ENVS.get() {
            if let Ok(mut map) = map.lock() {
                map.clear();
            }
        }
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

    /// 【T2b-3】自定义环境全生命周期：create → list/installed/resolve 三处一致 →
    /// update 改命令 → delete 清干净。命令用不存在的解释器（探测必失败）→ 版本如实 None、
    /// 未安装如实 false——管理环境不假装装好了。
    #[test]
    fn custom_env_full_lifecycle_through_a_real_store() {
        let db = temp_db("lifecycle");
        let store = open_store(&db);
        let port = SystemEnvPort::with_store(Some(store));

        // 新增：名字 / 类型 / 命令 / 参数；版本探测失败（命令不存在）→ None
        let created = port
            .create("Py Venv", crate::env::EnvKind::Python, "no-such-interp-xyz", &["-u".to_string()])
            .expect("create should succeed");
        assert!(!created.id.is_empty(), "id 由宿主生成");
        assert!(created.id.starts_with("custom-"));
        assert_eq!(created.name, "Py Venv");
        assert_eq!(created.command, "no-such-interp-xyz");
        assert_eq!(created.args, vec!["-u"]);
        assert_eq!(created.version, None, "命令不存在 → 版本如实未知");
        assert!(!created.readonly, "自定义环境可管理");

        let list = port.list();
        assert!(
            list.iter().any(|env| env.id == created.id && !env.readonly),
            "自定义环境出现在列表里且非只读"
        );
        assert!(!port.installed(&created.id), "没探测到命令 = 未安装");

        // 执行器视角：同一份 memo 能解析出用户填的命令
        let resolved = resolve(&created.id).expect("memo 里应有自定义环境");
        assert_eq!(resolved.command, "no-such-interp-xyz");
        assert_eq!(resolved.args, vec!["-u"]);

        // 改：换命令 + 参数 → 列表与 memo 都跟着变
        let updated = port
            .update(&created.id, "Py Venv 3.13", crate::env::EnvKind::Python, "no-such-interp-abc", &["-u".to_string(), "--no-site".to_string()])
            .expect("update should succeed");
        assert_eq!(updated.name, "Py Venv 3.13");
        assert_eq!(updated.command, "no-such-interp-abc");
        assert_eq!(updated.args, vec!["-u", "--no-site"]);
        let updated_in_list = port
            .list()
            .into_iter()
            .find(|env| env.id == created.id)
            .expect("updated env still listed");
        assert_eq!(updated_in_list.name, "Py Venv 3.13");
        assert_eq!(resolve(&created.id).expect("resolve after update").command, "no-such-interp-abc");

        // 删：列表 / installed / memo 全部清干净
        port.delete(&created.id).expect("delete should succeed");
        assert!(!port.list().iter().any(|env| env.id == created.id));
        assert!(!port.installed(&created.id));
        assert!(resolve(&created.id).is_err(), "删除后执行器不再认它");

        // 清理 memo
        if let Some(map) = RESOLVED_ENVS.get() {
            if let Ok(mut map) = map.lock() {
                map.clear();
            }
        }
    }

    /// 【T2b-3】内置环境只读：create 校验（空名 / 空命令拒绝），update / delete 内置拒绝
    #[test]
    fn builtin_envs_are_read_only_and_validation_blocks_blanks() {
        let db = temp_db("builtin");
        let store = open_store(&db);
        let port = SystemEnvPort::with_store(Some(store));

        // 空名 / 空命令如实拒绝（不写库）
        assert!(port
            .create("  ", crate::env::EnvKind::Custom, "cmd", &[])
            .is_err());
        assert!(port
            .create("name", crate::env::EnvKind::Custom, "  ", &[])
            .is_err());

        // 内置环境（python）不可改、不可删
        let py = port
            .list()
            .into_iter()
            .find(|env| env.id == "python")
            .expect("builtin python listed");
        assert!(py.readonly);
        assert!(port
            .update("python", "改名", crate::env::EnvKind::Python, "py", &[])
            .is_err());
        assert!(port.delete("python").is_err());

        // 改 / 删不存在的环境也如实拒绝
        assert!(port.update("env-404", "x", crate::env::EnvKind::Custom, "cmd", &[]).is_err());
        assert!(port.delete("env-404").is_err());

        // 清理 memo
        if let Some(map) = RESOLVED_ENVS.get() {
            if let Ok(mut map) = map.lock() {
                map.clear();
            }
        }
    }

    /// 端口接不接存储都行：不接（None）= 列表只有内置、管理如实拒绝，但 list/installed 正常
    #[test]
    fn without_a_store_management_is_honestly_rejected() {
        let port = SystemEnvPort::with_store(None);
        assert!(!port.list().is_empty(), "没有存储也有内置环境");
        assert!(port
            .create("x", crate::env::EnvKind::Custom, "cmd", &[])
            .is_err(), "没有存储 = 管理如实拒绝");
        assert!(port.delete("python").is_err(), "没有存储也拒绝内置删除");
    }
}
