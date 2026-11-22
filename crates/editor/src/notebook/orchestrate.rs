//! 内核单元事件的收口规则（**纯函数，不依赖 gpui**）
//!
//! 从 `view/host/notebook.rs::drain_kernel_events` 抽出（R6）：归属过滤、
//! stderr 前缀、mime 分发、`ExecuteReply` 收口、`Error` 落地。
//!
//! 视图侧只负责三件事：取事件流 → 调这里 → 按 [`Applied`] 决定 notify 与队列推进。
//! 这样这几条**业务规则**可以在无窗口环境下单测（R7），而不是必须挂 GPUI 观察层。

use crate::cell::{Cell, CellState, Output};
use rds_kernel::{KernelEvent, KernelEventEnvelope};

/// 一次进行中执行的归属身份（由视图从 `KernelRun` 构造）
#[derive(Debug, Clone, Copy)]
pub struct ExecutionTag<'a> {
    /// 本次 `execute_request` 的 msg_id（来源 `KernelSession::execute` 的返回值）
    pub msg_id: &'a str,
}

/// 单个事件的归属判定
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 属于本次执行，或会话级事件（`msg_id = None`）：处理
    Accept,
    /// 有归属但不是本次执行（上一单元迟到）：丢弃
    Foreign,
}

/// 判定一条事件该不该落到当前单元
///
/// - 会话级事件（`Failed` / `Status`，`msg_id = None`）**无条件接受**：它们不属于
///   任何一次执行，但会决定单元是否失败/内核是否停。
/// - 有归属且 id 不等于本次执行 → `Foreign`：既不能混进本单元输出，也不能用它的
///   `ExecuteReply` 把本单元提前收口。
pub fn verdict(envelope: &KernelEventEnvelope, tag: ExecutionTag<'_>) -> Verdict {
    match envelope.msg_id.as_deref() {
        Some(id) if id != tag.msg_id => Verdict::Foreign,
        _ => Verdict::Accept,
    }
}

/// 一批事件应用到单元后的结论
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    /// 这一批里出现了收口信号（`ExecuteReply` 或 `Failed`）
    pub finished: bool,
    /// 收口失败原因（`None` = 成功收口；`finished = false` 时无意义）
    pub failure: Option<String>,
    /// 实际接受并落地的事件数（`Foreign` 不计）
    pub accepted: usize,
}

/// 把一批事件按到达顺序应用到单元
///
/// 语义与抽出前逐条对齐（行为等价）：
/// - `Display` → [`crate::media::media_to_outputs`]（图片 / 表格 / markdown / 文本）
/// - `Stream` → stdout 原样、stderr 加 `stderr: ` 前缀（如实反映来源）
/// - `Error` → `Output::Error("{ename}: {evalue}")`
/// - `ExecuteReply` → 收口：`ok` 置 [`CellState::Done`]，否则 [`CellState::Error`]
/// - `Failed` → 收口失败
/// - `Status` → 忽略
///
/// 收口时**不重复**追加 `Error` 输出：内核已经给了 `Error` 事件时，只改状态。
pub fn apply_events<'a, I>(cell: &mut Cell, tag: ExecutionTag<'_>, events: I) -> Applied
where
    I: IntoIterator<Item = &'a KernelEventEnvelope>,
{
    let mut applied = Applied::default();

    for envelope in events {
        if verdict(envelope, tag) == Verdict::Foreign {
            continue;
        }
        applied.accepted += 1;

        match &envelope.event {
            KernelEvent::Display(media) => {
                // mime bundle → 原生输出（纯函数在 notebook/media.rs）
                let outputs = crate::media::media_to_outputs(media);
                cell.outputs.extend(outputs);
            }
            KernelEvent::Stream { name, text } => {
                if name == "stderr" {
                    cell.outputs.push(Output::Text(format!("stderr: {text}")));
                } else {
                    cell.outputs.push(Output::Text(text.clone()));
                }
            }
            KernelEvent::ExecuteReply { status, .. } => {
                applied.finished = true;
                if status != "ok" {
                    applied.failure = Some("内核返回执行失败".to_string());
                }
            }
            KernelEvent::Error { ename, evalue, .. } => {
                cell.outputs
                    .push(Output::Error(format!("{ename}: {evalue}")));
            }
            KernelEvent::Failed(reason) => {
                applied.finished = true;
                applied.failure = Some(reason.clone());
            }
            KernelEvent::Status(_) => {}
        }
    }

    if applied.finished {
        match applied.failure.as_ref() {
            Some(reason) => {
                cell.state = CellState::Error;
                // 内核已经落过 Error 输出时不重复追加（避免同一条错误出现两遍）
                if !cell.outputs.iter().any(|o| matches!(o, Output::Error(_))) {
                    cell.outputs.push(Output::Error(reason.clone()));
                }
            }
            None => cell.state = CellState::Done,
        }
    }

    applied
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::CellLanguage;

    fn cell() -> Cell {
        Cell::new("c1", CellLanguage::Python, "1+1")
    }

    fn env(msg_id: Option<&str>, event: KernelEvent) -> KernelEventEnvelope {
        KernelEventEnvelope {
            msg_id: msg_id.map(str::to_string),
            event,
        }
    }

    fn stream(msg_id: Option<&str>, name: &str, text: &str) -> KernelEventEnvelope {
        env(
            msg_id,
            KernelEvent::Stream {
                name: name.to_string(),
                text: text.to_string(),
            },
        )
    }

    fn reply(msg_id: Option<&str>, status: &str) -> KernelEventEnvelope {
        env(
            msg_id,
            KernelEvent::ExecuteReply {
                status: status.to_string(),
                execution_count: Some(1),
            },
        )
    }

    /// 区分性证明：若归属过滤缺失（仍处理全部事件），这条必红
    #[test]
    fn foreign_event_is_dropped() {
        let mut c = cell();
        let events = vec![
            stream(Some("old-exec"), "stdout", "上一单元迟到"),
            stream(Some("this-exec"), "stdout", "本次输出"),
        ];
        let applied = apply_events(&mut c, ExecutionTag { msg_id: "this-exec" }, &events);

        assert_eq!(applied.accepted, 1, "只应接受归属本次执行的那一条");
        assert_eq!(c.outputs.len(), 1);
        assert_eq!(c.outputs[0], Output::Text("本次输出".to_string()));
        assert!(
            !c.outputs
                .iter()
                .any(|o| matches!(o, Output::Text(t) if t.contains("迟到") )),
            "上一单元的迟到输出不得混入"
        );
    }

    /// 区分性证明：若丢掉归属过滤，迟到的旧 ExecuteReply 会把本单元提前收口
    #[test]
    fn foreign_execute_reply_does_not_finish() {
        let mut c = cell();
        c.mark_running();
        let events = vec![reply(Some("old-exec"), "ok")];
        let applied = apply_events(&mut c, ExecutionTag { msg_id: "this-exec" }, &events);

        assert!(!applied.finished, "旧回执不得收口本单元");
        assert_eq!(c.state, CellState::Running, "仍在运行中，等本次执行的 reply");
    }

    #[test]
    fn stdout_keeps_raw_text_and_stderr_is_prefixed() {
        let mut c = cell();
        let events = vec![
            stream(Some("e"), "stdout", "hello"),
            stream(Some("e"), "stderr", "warn"),
        ];
        apply_events(&mut c, ExecutionTag { msg_id: "e" }, &events);

        assert_eq!(c.outputs[0], Output::Text("hello".to_string()));
        assert_eq!(c.outputs[1], Output::Text("stderr: warn".to_string()));
    }

    #[test]
    fn ok_reply_finishes_done() {
        let mut c = cell();
        let applied = apply_events(
            &mut c,
            ExecutionTag { msg_id: "e" },
            &[reply(Some("e"), "ok")],
        );

        assert!(applied.finished);
        assert_eq!(applied.failure, None);
        assert_eq!(c.state, CellState::Done);
    }

    #[test]
    fn error_reply_finishes_error_without_duplicating_error_output() {
        let mut c = cell();
        let events = vec![
            env(
                Some("e"),
                KernelEvent::Error {
                    ename: "ValueError".to_string(),
                    evalue: "boom".to_string(),
                    traceback: vec![],
                },
            ),
            reply(Some("e"), "error"),
        ];
        let applied = apply_events(&mut c, ExecutionTag { msg_id: "e" }, &events);

        assert!(applied.finished);
        assert_eq!(applied.failure.as_deref(), Some("内核返回执行失败"));
        assert_eq!(c.state, CellState::Error);
        assert_eq!(c.outputs.len(), 1, "已有 Error 输出时不再追加一条");
    }

    #[test]
    fn error_reply_without_error_event_appends_reason() {
        let mut c = cell();
        let applied = apply_events(
            &mut c,
            ExecutionTag { msg_id: "e" },
            &[reply(Some("e"), "error")],
        );

        assert_eq!(c.state, CellState::Error);
        assert!(matches!(c.outputs[0], Output::Error(_)));
    }

    /// 会话级事件（msg_id = None）必须被接受：通道失败要能收口
    #[test]
    fn session_level_failure_is_accepted() {
        let mut c = cell();
        let events = vec![env(None, KernelEvent::Failed("通道断开".to_string()))];
        let applied = apply_events(&mut c, ExecutionTag { msg_id: "e" }, &events);

        assert!(applied.finished);
        assert_eq!(c.state, CellState::Error);
    }

    #[test]
    fn status_events_are_ignored_but_counted_as_accepted() {
        let mut c = cell();
        let events = vec![env(None, KernelEvent::Status(rds_kernel::KernelStatus::Busy))];
        let applied = apply_events(&mut c, ExecutionTag { msg_id: "e" }, &events);

        assert!(!applied.finished);
        assert_eq!(applied.accepted, 1);
        assert!(c.outputs.is_empty());
    }
}
