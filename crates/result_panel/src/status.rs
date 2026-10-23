//! 结果面板的状态行文本换算（纯函数）
//!
//! 2026-10-23 下移自 `rds-editor` 的 `view/widgets/status_bar.rs`：结果状态行
//! （耗时 / 行数）与编辑器状态栏共用同一份人读文案，随结果面板独立成 crate，
//! editor 侧经 `result_panel::status` 导入。

/// 跑多久了（人读的短文案：′10s 一位小数，之后整秒，过分钟折成 `1m42s`）
pub fn elapsed_text(elapsed: std::time::Duration) -> String {
    let secs = elapsed.as_secs_f32();
    if secs < 10.0 {
        format!("{secs:.1}s")
    } else if secs < 60.0 {
        format!("{}s", secs as u64)
    } else {
        let total = secs as u64;
        format!("{}m{}s", total / 60, total % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::elapsed_text;
    use std::time::Duration;

    #[test]
    fn elapsed_under_ten_seconds_keeps_one_decimal() {
        assert_eq!(elapsed_text(Duration::from_millis(2500)), "2.5s");
        assert_eq!(elapsed_text(Duration::from_millis(900)), "0.9s");
    }

    #[test]
    fn elapsed_over_ten_seconds_drops_the_decimal() {
        assert_eq!(elapsed_text(Duration::from_secs(12)), "12s");
        assert_eq!(elapsed_text(Duration::from_secs(59)), "59s");
    }

    #[test]
    fn elapsed_over_a_minute_uses_minutes_and_seconds() {
        assert_eq!(elapsed_text(Duration::from_secs(102)), "1m42s");
        assert_eq!(elapsed_text(Duration::from_secs(3600)), "60m0s");
    }
}
