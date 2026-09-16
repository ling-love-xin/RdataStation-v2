//! Mock 临时表清理的集成测试（单独一个测试二进制 = 独立进程）。
//!
//! 为什么必须独立进程：`clear_temp_tables()` 清的是**本进程内存库**里的全部 mock 临时表，
//! 与 `mock_engine_tests.rs` 里那些「先生成、后导出」的用例并行跑时会互相踩
//! （把对方刚生成的临时表删掉）。和 `mock_job_cancel.rs` 同一个理由：进程级副作用
//! 只能在自己的进程里验。

use rds_mock::{
    ColumnDataType, ColumnDef, GeneratorConfig, Locale, MockConfig, MockEngine, MockResult,
};

/// 清理是**进程级**动作：同二进制内的用例必须串行，否则会把对方刚生成的临时表删掉。
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn auto_increment(name: &str) -> ColumnDef {
    ColumnDef {
        name: name.to_string(),
        data_type: ColumnDataType::Integer,
        generator: GeneratorConfig::AutoIncrement { start: 1, step: 1 },
        nullable_ratio: 0.0,
        unique: true,
        dependency: None,
    }
}

fn config(table: &str) -> MockConfig {
    MockConfig {
        table_name: table.to_string(),
        row_count: 20,
        seed: Some(1),
        locale: Locale::ZhCn,
        columns: vec![auto_increment("id")],
    }
}

/// 生成两张不同名字的临时表 → 都在库里；清理后一张不剩；注册表也跟着归零。
#[tokio::test]
async fn clear_temp_tables_drops_every_mock_table() {
    let _guard = serial();
    let first = MockEngine::generate(config("t_clean_a"))
        .await
        .expect("生成 A");
    let second = MockEngine::generate(config("t_clean_b"))
        .await
        .expect("生成 B");
    assert_ne!(
        first.temp_table_name, second.temp_table_name,
        "不同目标表名应落在不同临时表"
    );

    let before = MockEngine::temp_tables().expect("列临时表");
    assert!(
        before.contains(&first.temp_table_name) && before.contains(&second.temp_table_name),
        "两张都应还在: {before:?}"
    );

    let dropped = MockEngine::clear_temp_tables().expect("清理应当成功");
    assert!(
        dropped.len() >= 2,
        "至少要删掉刚生成的两张: {dropped:?}"
    );

    let after = MockEngine::temp_tables().expect("列临时表");
    assert!(
        after.is_empty(),
        "清理后不应再有 mock 临时表: {after:?}"
    );

    // 清理是幂等的：再清一次不报错、没有可删的
    let again = MockEngine::clear_temp_tables().expect("重复清理应当无害");
    assert!(again.is_empty(), "{again:?}");
}

/// 同名重复生成只留一张（引擎每次 DROP + 重建），清理后仍然为空。
#[tokio::test]
async fn repeated_generation_keeps_a_single_table() -> MockResult<()> {
    let _guard = serial();
    let first = MockEngine::generate(config("t_clean_same")).await?;
    let second = MockEngine::generate(config("t_clean_same")).await?;
    assert_eq!(first.temp_table_name, second.temp_table_name);

    let tables = MockEngine::temp_tables()?;
    assert_eq!(
        tables.iter().filter(|name| name.contains("t_clean_same")).count(),
        1,
        "同名只应有一张: {tables:?}"
    );

    MockEngine::clear_temp_tables()?;
    assert!(MockEngine::temp_tables()?.is_empty());
    Ok(())
}
