    use super::*;

    fn temp_project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rds_payload_{tag}_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("create temp project");
        dir
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

        tokio::fs::write(&file, b"select 2;").await.expect("rewrite");
        let third = store.content_hash(&file).await.expect("hash 3");
        assert_ne!(first, third, "内容变化必须改变指纹");

        cleanup(&project);
    }

    #[tokio::test]
    async fn version_copy_is_idempotent_and_prunes_oldest() {
        let project = temp_project("versions");
        let store = PayloadStore::new(&project);
        let source = project.join("draft.sql");
        tokio::fs::write(&source, b"v").await.expect("write");
        store
            .archive_in(&source, "keep.sql")
            .await
            .expect("archive");

        for version in 1..=4 {
            let stored = store
                .store_version_copy("ar_test", version, "keep.sql")
                .await
                .expect("version copy")
                .expect("some path");
            assert!(stored.is_file());
        }
        // 幂等：重复写同版本不报错，也不新增目录。
        assert!(
            store
                .store_version_copy("ar_test", 4, "keep.sql")
                .await
                .expect("idempotent")
                .is_some()
        );

        let removed = store
            .prune_version_copies("ar_test", 2)
            .await
            .expect("prune");
        assert_eq!(removed, 2, "保留 2 份、应删 2 份");
        assert!(store.versions_dir().join("ar_test").join("4").is_dir());
        assert!(!store.versions_dir().join("ar_test").join("1").is_dir());

        cleanup(&project);
    }

    #[tokio::test]
    async fn version_copies_can_be_listed_copied_out_deleted_and_restored() {
        let project = temp_project("versions_ops");
        let store = PayloadStore::new(&project);
        let source = project.join("draft.sql");
        tokio::fs::write(&source, b"old").await.expect("write");
        store.archive_in(&source, "dau.sql").await.expect("archive");

        // 造两份历史副本：v1 = "old"、v2 = "mid"（本体当前是 "mid"）。
        store
            .store_version_copy("ar_test", 1, "dau.sql")
            .await
            .expect("copy v1");
        let mid = project.join("mid.sql");
        tokio::fs::write(&mid, b"mid").await.expect("write mid");
        store
            .replace_payload(&mid, "dau.sql")
            .await
            .expect("replace");
        store
            .store_version_copy("ar_test", 2, "dau.sql")
            .await
            .expect("copy v2");

        assert_eq!(
            store.version_copies("ar_test").await.expect("list"),
            vec![2, 1],
            "副本列举降序（新的在前）"
        );
        assert!(
            store
                .version_copy_file("ar_test", 3)
                .await
                .expect("probe")
                .is_none(),
            "没写过的版本没有副本"
        );
        assert!(
            store
                .version_copies("ar_never")
                .await
                .expect("missing dir")
                .is_empty(),
            "从未留过副本的存档 = 空，而不是错误"
        );

        // 复制出去 = “取回该版本为草稿”：内容是那一版的字节，且副本本身不动。
        let dest = project.join("scratchpad").join("dau（v1 工作副本）.sql");
        assert!(
            store
                .copy_version_out("ar_test", 1, &dest)
                .await
                .expect("copy out")
        );
        assert_eq!(tokio::fs::read(&dest).await.expect("read"), b"old");
        assert!(!store.is_readonly(&dest), "工作副本不应只读");
        assert!(
            !store
                .copy_version_out("ar_test", 9, &dest)
                .await
                .expect("missing copy"),
            "没有副本的版本 = false（调用方据此给明确原因）"
        );

        // 写回本体 = “还原为当前版本”的本体侧动作：内容回到 v1，且本体重新只读。
        assert!(
            store
                .restore_version_copy("ar_test", 1, "dau.sql")
                .await
                .expect("restore")
        );
        let payload = store.resolve("dau.sql").expect("resolve");
        assert_eq!(tokio::fs::read(&payload).await.expect("read"), b"old");
        assert!(store.is_readonly(&payload), "写回的本体也要带只读属性");
        assert!(
            store
                .version_copy_file("ar_test", 1)
                .await
                .expect("probe")
                .is_some(),
            "还原不消耗副本（它仍是历史）"
        );

        // 删副本：只删这一份（版本行不在这一层）；重复删 = false。
        assert!(
            store
                .delete_version_copy("ar_test", 2)
                .await
                .expect("delete")
        );
        assert_eq!(store.version_copies("ar_test").await.expect("list"), vec![1]);
        assert!(
            !store
                .delete_version_copy("ar_test", 2)
                .await
                .expect("again"),
            "已经删过 = false"
        );

        cleanup(&project);
    }

    #[tokio::test]
    async fn copy_out_keeps_payload_in_place() {
        let project = temp_project("checkout");
        let store = PayloadStore::new(&project);
        let source = project.join("draft.sql");
        tokio::fs::write(&source, b"select 1;").await.expect("write");
        let payload = store
            .archive_in(&source, "dau.sql")
            .await
            .expect("archive");

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
        tokio::fs::write(&first, b"select 1;").await.expect("write v1");
        let payload = store.archive_in(&first, "dau.sql").await.expect("archive v1");
        assert!(store.is_readonly(&payload));
        let hash_v1 = store.content_hash(&payload).await.expect("hash v1");

        let second = project.join("v2.sql");
        tokio::fs::write(&second, b"select 2;").await.expect("write v2");
        let replaced = store.replace_payload(&second, "dau.sql").await.expect("replace");

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
        tokio::fs::write(&source, b"select 1;").await.expect("write");
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

    #[test]
    fn safe_segment_rejects_traversal() {
        assert!(safe_segment("ar_1234").is_ok());
        assert!(safe_segment("../etc").is_err());
        assert!(safe_segment("a/b").is_err());
        assert!(safe_segment(".hidden").is_err());
        assert!(safe_segment("  ").is_err());
    }
