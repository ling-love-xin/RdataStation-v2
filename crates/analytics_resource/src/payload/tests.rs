use super::*;

fn temp_project(tag: &str) -> PathBuf {
    crate::tests::temp_project_dir(&format!("payload_{tag}"))
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// 读开头（内容预览用）：截断时**在最后一个换行处切齐**、开头有 NUL 就当不是文本。
#[tokio::test]
async fn read_head_cuts_at_a_line_boundary_and_reports_binary() {
    let project = temp_project("read_head");
    let store = PayloadStore::new(&project);
    store.ensure_dir().await.expect("ensure dir");

    // 小文件：一次读完 → complete = true。
    let small = store.resources_dir().join("月报.sql");
    std::fs::write(&small, b"select 1\nfrom dual\n").expect("write small");
    let (head, complete) = store
        .read_head("月报.sql", 1024)
        .await
        .expect("read")
        .expect("是文本");
    assert!(complete);
    assert_eq!(head, "select 1\nfrom dual\n");

    // 大一点的文件：只读开头，且**切在换行处**（不切会让最后一行看起来被文件截断）。
    let long_line = "x".repeat(80);
    let body: String = (0..50).map(|i| format!("{i:02} {long_line}\n")).collect();
    let big = store.resources_dir().join("big.sql");
    std::fs::write(&big, body.as_bytes()).expect("write big");
    let (head, complete) = store
        .read_head("big.sql", 200)
        .await
        .expect("read")
        .expect("是文本");
    assert!(!complete, "没读完：调用方据此标截断");
    assert!(head.ends_with('\n'), "切在换行处：{head:?}");
    assert!(head.len() <= 200);
    assert_eq!(head.lines().count(), 2, "200 字节正好两行（每行 83 字节）");

    // 二进制（开头带 NUL）：`None` = 不预览，而不是给一堆乱码。
    let binary = store.resources_dir().join("model.parquet");
    std::fs::write(&binary, [0x50, 0x41, 0x52, 0x00, 0x01, 0x02]).expect("write binary");
    assert!(
        store
            .read_head("model.parquet", 1024)
            .await
            .expect("read")
            .is_none()
    );

    // 路径越界 / 不存在：报错误（调用方只在日志里记一声，不打断刷新）。
    assert!(store.read_head("../outside.sql", 64).await.is_err());

    cleanup(&project);
}

#[test]
fn free_rel_path_avoids_taken_names_keeping_dirs_and_extension() {
    let project = temp_project("free_rel");
    let store = PayloadStore::new(&project);

    // 没人占用：原样返回（不无谓地改名）。
    assert_eq!(store.free_rel_path("dau.sql"), "dau.sql");

    let reports = store.resources_dir().join("reports");
    std::fs::create_dir_all(&reports).expect("create dir");
    std::fs::write(reports.join("dau.sql"), b"x").expect("write");
    std::fs::write(reports.join("dau-2.sql"), b"x").expect("write");

    assert!(store.rel_path_taken("reports/dau.sql"));
    assert!(!store.rel_path_taken("reports/other.sql"));
    assert_eq!(
        store.free_rel_path("reports/dau.sql"),
        "reports/dau-3.sql",
        "目录保留、后缀递增、扩展名在末尾"
    );

    // 越界路径不会被当成“可用名”：resolve 失败按已占用对待，退回原名交给服务层报错。
    assert_eq!(store.free_rel_path("../outside.sql"), "../outside.sql");

    cleanup(&project);
}

#[test]
fn resolve_rejects_escape_and_internal_paths() {
    let project = temp_project("resolve");
    let store = PayloadStore::new(&project);

    assert!(store.resolve("a/b.sql").is_ok());
    assert!(store.resolve("../outside.sql").is_err());
    assert!(store.resolve("a/../../outside.sql").is_err());
    assert!(store.resolve(".RSmeta/project.db").is_err());
    assert!(store.resolve("a/.hidden").is_err());
    assert!(store.resolve("/abs.sql").is_err());
    assert!(store.resolve("C:\\abs.sql").is_err());
    assert!(store.resolve("\\abs\\x.sql").is_err());
    assert!(store.resolve("   ").is_err());

    cleanup(&project);
}

#[test]
fn resolve_normalizes_curdir_component() {
    let project = temp_project("normalize");
    let store = PayloadStore::new(&project);

    let normalized = store.resolve("reports/./dau.sql").expect("./ 应视为无操作");
    assert!(normalized.starts_with(store.resources_dir()));
    assert!(
        normalized.ends_with(Path::new("reports").join("dau.sql")),
        "结果路径应已规范化：{}",
        normalized.display()
    );

    cleanup(&project);
}

#[tokio::test]
async fn archive_in_moves_file_and_marks_readonly() {
    let project = temp_project("archive");
    let store = PayloadStore::new(&project);
    let source = project.join("draft.sql");
    tokio::fs::write(&source, b"select 1;")
        .await
        .expect("write draft");

    let dest = store
        .archive_in(&source, "reports/dau.sql")
        .await
        .expect("archive");

    assert!(dest.is_file());
    assert!(!source.exists(), "归档是移动：源文件不应还在原处");
    assert!(dest.starts_with(store.resources_dir()));
    assert!(store.is_readonly(&dest), "归档后应带只读属性");
    assert!(store.ensure_writable(&dest).is_err());

    cleanup(&project);
}

#[tokio::test]
async fn archive_in_refuses_existing_target() {
    let project = temp_project("conflict");
    let store = PayloadStore::new(&project);

    let first = project.join("a.sql");
    tokio::fs::write(&first, b"one").await.expect("write a");
    store.archive_in(&first, "same.sql").await.expect("first");

    let second = project.join("b.sql");
    tokio::fs::write(&second, b"two").await.expect("write b");
    assert!(
        store.archive_in(&second, "same.sql").await.is_err(),
        "目标已存在时必须报错（不静默覆盖）"
    );
    assert!(second.exists(), "失败时源文件必须原样保留");

    cleanup(&project);
}

#[tokio::test]
async fn content_hash_is_stable_and_content_sensitive() {
    let project = temp_project("hash");
    let store = PayloadStore::new(&project);
    let file = project.join("x.sql");

    tokio::fs::write(&file, b"select 1;").await.expect("write");
    let first = store.content_hash(&file).await.expect("hash 1");
    let second = store.content_hash(&file).await.expect("hash 2");
    assert_eq!(first, second, "同一内容指纹必须稳定");
    assert_eq!(first.len(), 64, "sha256 十六进制长度");

    tokio::fs::write(&file, b"select 2;")
        .await
        .expect("rewrite");
    let third = store.content_hash(&file).await.expect("hash 3");
    assert_ne!(first, third, "内容变化必须改变指纹");

    cleanup(&project);
}

#[tokio::test]
async fn copy_out_keeps_payload_in_place() {
    let project = temp_project("checkout");
    let store = PayloadStore::new(&project);
    let source = project.join("draft.sql");
    tokio::fs::write(&source, b"select 1;")
        .await
        .expect("write");
    let payload = store.archive_in(&source, "dau.sql").await.expect("archive");

    let dest = project.join("scratchpad").join("dau（工作副本）.sql");
    store.copy_out("dau.sql", &dest).await.expect("checkout");

    assert!(dest.is_file(), "取回应产生草稿副本");
    assert!(payload.is_file(), "取回不得移动本体");
    assert!(!store.is_readonly(&dest), "工作副本不应只读");

    cleanup(&project);
}

#[tokio::test]
async fn replace_payload_overwrites_readonly_payload() {
    let project = temp_project("replace");
    let store = PayloadStore::new(&project);

    let first = project.join("v1.sql");
    tokio::fs::write(&first, b"select 1;")
        .await
        .expect("write v1");
    let payload = store
        .archive_in(&first, "dau.sql")
        .await
        .expect("archive v1");
    assert!(store.is_readonly(&payload));
    let hash_v1 = store.content_hash(&payload).await.expect("hash v1");

    let second = project.join("v2.sql");
    tokio::fs::write(&second, b"select 2;")
        .await
        .expect("write v2");
    let replaced = store
        .replace_payload(&second, "dau.sql")
        .await
        .expect("replace");

    assert_eq!(replaced, payload, "路径不变（引用不断）");
    assert!(store.is_readonly(&replaced), "替换后仍只读");
    assert_ne!(
        store.content_hash(&replaced).await.expect("hash v2"),
        hash_v1,
        "内容应为新版本"
    );

    cleanup(&project);
}

#[tokio::test]
async fn move_payload_out_returns_writable_file() {
    let project = temp_project("moveout");
    let store = PayloadStore::new(&project);

    let source = project.join("draft.sql");
    tokio::fs::write(&source, b"select 1;")
        .await
        .expect("write");
    store.archive_in(&source, "dau.sql").await.expect("archive");

    // 回滚 / 腾位：移出后应回到工作区且可写。
    store
        .move_payload_out("dau.sql", &source)
        .await
        .expect("move out");
    assert!(source.is_file());
    assert!(!store.is_readonly(&source), "移出的文件必须可写");
    assert!(!store.resources_dir().join("dau.sql").exists());

    cleanup(&project);
}

#[tokio::test]
async fn list_files_walks_recursively_and_skips_hidden() {
    let project = temp_project("list");
    let store = PayloadStore::new(&project);
    store.ensure_dir().await.expect("ensure");

    for rel in ["a.sql", "reports/dau.sql", "reports/sub/x.sql"] {
        let src = project.join(format!("src_{}", rel.replace('/', "_")));
        tokio::fs::write(&src, b"select 1;").await.expect("write");
        store.archive_in(&src, rel).await.expect("archive");
    }
    // 隐藏目录不入结果（内部态不得被当成"未登记本体"）。
    tokio::fs::create_dir_all(store.resources_dir().join(".cache"))
        .await
        .expect("mkdir hidden");
    tokio::fs::write(store.resources_dir().join(".cache").join("x"), b"x")
        .await
        .expect("write hidden");

    let files = store.list_files().await.expect("list");
    assert_eq!(
        files,
        vec![
            "a.sql".to_string(),
            "reports/dau.sql".to_string(),
            "reports/sub/x.sql".to_string()
        ],
        "应递归、排序、跳过隐藏项"
    );

    cleanup(&project);
}

/// 2026-11-12 暂存重放：stage 必须留下可反查 `rel` 的 sidecar，commit 成功后
/// tmp 与 meta 都要清掉——否则刷新会把已提交的内容又重放一次覆盖本体。
#[tokio::test]
async fn staging_roundtrip_writes_sidecar_and_cleans_up_on_commit() {
    let project = temp_project("staging");
    let store = PayloadStore::new(&project);
    store.ensure_dir().await.expect("ensure dir");
    let source = project.join("draft.sql");
    std::fs::write(&source, b"select 1;\n").expect("write source");

    let staged = store
        .stage_payload(&source, "reports/dau.sql")
        .await
        .expect("stage");
    assert!(staged.is_file(), "暂存内容应在同卷暂存区");
    assert!(
        staged.with_extension("meta").is_file(),
        "sidecar 必须落地，否则崩溃后无法重放"
    );
    let listed = store.list_staged();
    assert_eq!(listed.len(), 1, "只应发现这一份暂存");
    assert_eq!(listed[0].rel, "reports/dau.sql");
    assert_eq!(listed[0].staged, staged);

    store
        .commit_staged(&staged, "reports/dau.sql")
        .await
        .expect("commit staged");
    let dest = store.resources_dir().join("reports/dau.sql");
    assert_eq!(std::fs::read(&dest).expect("read dest"), b"select 1;\n");
    assert!(!staged.exists(), "rename 成功后 tmp 不应残留");
    assert!(!staged.with_extension("meta").exists(), "meta 不应残留");
    assert!(store.list_staged().is_empty(), "不应再有可重放内容");

    // Windows 只读属性会让临时目录删不掉（这个测试自己造的脏，自己清）。
    let mut perms = std::fs::metadata(&dest).expect("meta").permissions();
    perms.set_readonly(false);
    std::fs::set_permissions(&dest, perms).expect("clear readonly");
    cleanup(&project);
}
