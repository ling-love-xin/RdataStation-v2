//! 语言服务面测试（补全 / 诊断 / code action 三面）
//!
//! 自 language/language_service.rs 拆分（2026-10-29 Step 9）。
//!
    // 安全模式：**不通配导入**
    use super::{
        ADD_LIMIT, ADD_WHERE, Catalog, CodeAction, Diagnostic, ExecChannel, Request, code_actions,
        complete, diagnostics, line_column, report,
    };
    use crate::completion::{Candidate, CandidateKind};
    use engine::sql::SqlDialect;

    /// 起手：光标在文末、绑了连接、源库通道、无选区（测试里按需改字段）
    fn ask<'a>(text: &'a str, catalog: &'a Catalog) -> Request<'a> {
        Request {
            text,
            cursor: text.len(),
            selection: None,
            catalog,
            connection: Some("P_orders"),
            channel: ExecChannel::Source,
            dialect: SqlDialect::Ansi,
        }
    }

    /// 诊断只要 code（位置与文案另有测试盯着）
    fn codes(text: &str) -> Vec<&'static str> {
        diagnostics(&ask(text, &Catalog::default()))
            .into_iter()
            .map(|one| one.code)
            .collect()
    }

    fn found(text: &str) -> Vec<Diagnostic> {
        diagnostics(&ask(text, &Catalog::default()))
    }

    /// 动作（默认源库通道、光标在文末）
    fn actions(text: &str, cursor: usize) -> Vec<CodeAction> {
        code_actions(&Request {
            cursor,
            ..ask(text, &Catalog::default())
        })
    }

    fn ids(text: &str, cursor: usize) -> Vec<&'static str> {
        actions(text, cursor)
            .into_iter()
            .map(|one| one.id)
            .collect()
    }

    fn catalog_with_orders() -> Catalog {
        Catalog {
            objects: vec![Candidate::new("public.orders", CandidateKind::Table)],
            columns: vec![(
                "public.orders".to_string(),
                "id".to_string(),
                Some("INT".to_string()),
            )],
            truncated: false,
        }
    }

    // ---------------- 诊断：该报 ----------------

    #[test]
    fn unterminated_literals_and_comments_are_reported() {
        // (文本, 该报的 code)
        let cases: [(&str, &[&str]); 9] = [
            ("SELECT 'abc", &["unterminated-string"]),
            ("SELECT 1;\nSELECT 'abc", &["unterminated-string"]),
            ("SELECT \"abc", &["unterminated-quoted-identifier"]),
            ("SELECT `abc", &["unterminated-quoted-identifier"]),
            ("SELECT /* abc", &["unterminated-comment"]),
            ("SELECT 1 /* a\n/* b */ ", &["unterminated-comment"]),
            ("SELECT $tag$abc", &["unterminated-dollar-quote"]),
            ("SELECT $$abc", &["unterminated-dollar-quote"]),
            ("SELECT (a", &["unbalanced-paren"]),
        ];
        for (text, expected) in cases {
            assert_eq!(codes(text), expected.to_vec(), "{text}");
        }
    }

    #[test]
    fn stray_close_parens_are_reported_at_the_first_offender() {
        assert_eq!(codes("SELECT a)"), vec!["stray-close-paren"]);
        assert_eq!(codes("SELECT a)) "), vec!["stray-close-paren"]);
        let one = found("SELECT 1)) ").remove(0);
        assert_eq!(one.start, "SELECT 1".len(), "指第一处多出来的 )");
    }

    #[test]
    fn the_unmatched_open_paren_is_the_innermost_one() {
        let text = "SELECT (a, (b";
        let one = found(text).remove(0);
        assert_eq!(one.code, "unbalanced-paren");
        assert_eq!(one.start, text.rfind('(').expect("有"));
        assert_eq!(one.end, one.start + 1);
    }

    #[test]
    fn the_unterminated_thing_is_the_last_opener_not_the_first_quote() {
        // 第一个 `'` 是闭合的（`'fine'`）：扫不动的是第二个
        let text = "SELECT 'fine' FROM t;\nSELECT 'abc";
        let one = found(text).remove(0);
        assert_eq!(one.code, "unterminated-string");
        assert_eq!(one.start, text.rfind('\'').expect("有"));
        assert_eq!(one.end, one.start + 1);
    }

    #[test]
    fn a_quote_inside_a_comment_is_not_an_unterminated_string() {
        // 注释里的撇号：整篇扫得动（词法层把它当注释），不该报
        assert!(codes("SELECT 1 -- 别写成 don't\n").is_empty());
        assert!(codes("SELECT 1 /* don't */").is_empty());
    }

    #[test]
    fn parens_are_judged_per_statement() {
        // 整篇配平、单条不配平：按语句判才看得见（`;` 两边各自是执行的单位）
        assert_eq!(
            codes("SELECT (a;\nSELECT b);"),
            vec!["unbalanced-paren", "stray-close-paren"]
        );
    }

    // ---------------- 诊断：不该报 ----------------

    #[test]
    fn clean_sql_gets_no_diagnostics() {
        let cases = [
            "",
            "   \n\t ",
            ";;;",
            "-- 只有注释\n",
            "/* 只有注释 */",
            "SELECT 1",
            "SELECT 'it''s' AS s",
            "SELECT '中文' AS 名称 FROM 订单",
            "SELECT count(*) FROM t",
            "SELECT (a), (b) FROM t WHERE x IN (1, 2)",
            "SELECT '(' AS c",
            "SELECT * FROM t -- (",
            "SELECT /* ( */ 1",
            "SELECT $$abc$$",
            "SELECT $tag$abc$tag$",
            "CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END; $$ LANGUAGE plpgsql;",
            "SELECT 1 + 1;\nUPDATE t SET x = 1;\nDELETE FROM t WHERE id = 3;",
        ];
        for text in cases {
            assert!(codes(text).is_empty(), "不该报：{text:?}");
        }
    }

    // ---------------- 诊断：语义层（#54-①，有目录才做） ----------------

    /// 带表/列目录（未知表 / 未知列能解析出来）
    fn catalog_with_tables() -> Catalog {
        Catalog {
            objects: vec![
                Candidate {
                    label: "orders".into(),
                    kind: CandidateKind::Table,
                    detail: None,
                },
                Candidate {
                    label: "public.customers".into(),
                    kind: CandidateKind::View,
                    detail: None,
                },
            ],
            columns: vec![
                ("orders".into(), "id".into(), None),
                ("orders".into(), "total".into(), None),
                ("public.customers".into(), "name".into(), None),
            ],
            truncated: false,
        }
    }

    #[test]
    fn semantic_diagnostics_report_unknown_table_and_column() {
        let text = "SELECT missing FROM no_such_table";
        let all = diagnostics(&ask(text, &catalog_with_tables()));
        let codes: Vec<&'static str> = all.iter().map(|one| one.code).collect();
        assert!(
            codes.contains(&"unknown_table"),
            "未知表要报（实际 {codes:?}）"
        );
        assert!(
            codes.contains(&"unqualified_column"),
            "未知列要报（实际 {codes:?}）"
        );
        // 区间指向原文（位置映射没跑偏）
        let table = all.iter().find(|d| d.code == "unknown_table").unwrap();
        assert_eq!(&text[table.start..table.end], "no_such_table");
        let column = all.iter().find(|d| d.code == "unqualified_column").unwrap();
        assert_eq!(&text[column.start..column.end], "missing");
    }

    #[test]
    fn semantic_diagnostics_skip_clean_sql_even_with_catalog() {
        // 有目录但 SQL 干净：裸列能归位就不是错
        let text = "SELECT id, total FROM orders";
        let all = diagnostics(&ask(text, &catalog_with_tables()));
        assert!(
            all.is_empty(),
            "干净 SQL 不该有语义诊断（实际 {all:?}）"
        );
    }

    #[test]
    fn semantic_diagnostics_degrade_without_catalog() {
        // 空目录（scan / 未绑定连接路径）→ 无语义诊断，与现有词法诊断不打架
        let text = "SELECT missing FROM no_such_table";
        let all = diagnostics(&ask(text, &Catalog::default()));
        assert!(
            all.is_empty(),
            "空目录不产语义诊断（实际 {all:?}）"
        );
    }

    #[test]
    fn a_parameter_is_not_a_dollar_quote() {
        // `$1` 是占位符，不是美元引用的开头
        assert!(codes("SELECT * FROM t WHERE id = $1").is_empty());
    }

    #[test]
    fn diagnostics_are_sorted_and_carry_positions() {
        let text = "SELECT 'a';\nSELECT (b;\nSELECT 'c";
        let all = found(text);
        assert_eq!(
            all.iter().map(|one| one.code).collect::<Vec<_>>(),
            vec!["unbalanced-paren", "unterminated-string"]
        );
        for pair in all.windows(2) {
            assert!(pair[0].start <= pair[1].start, "{pair:?}");
        }
        // `positions` 给的是 0 基行列（内核 `Position` 直接用）：第 2 行（下标 1）的第 8 个字符
        assert_eq!(all[0].positions(text), ((1, 7), (1, 8)));
        assert_eq!(line_column(text, 0), (0, 0));
        assert_eq!(line_column(text, text.len()), (2, 9));
    }

    #[test]
    fn huge_documents_are_not_scanned() {
        let text = format!("{}SELECT 'abc", "SELECT 1;\n".repeat(200_000));
        assert!(text.len() > super::SCAN_MAX_BYTES);
        assert!(codes(&text).is_empty(), "超过门槛就不扫（宁可一条不报）");
    }

    // ---------------- code action：给 ----------------

    #[test]
    fn a_select_without_a_cap_gets_a_limit() {
        let text = "SELECT * FROM orders";
        let out = actions(text, text.len());
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].id, ADD_LIMIT);
        assert_eq!(out[0].edit.start, text.len(), "追加在语句末尾");
        assert_eq!(out[0].edit.end, text.len(), "纯插入：范围零宽");
        assert_eq!(out[0].edit.replacement, " LIMIT 100");
    }

    #[test]
    fn the_limit_lands_before_the_semicolon_and_outside_literals() {
        // 语句末尾不含 `;`：追加落在分号前
        let text = "SELECT 1;";
        let out = actions(text, 5);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].edit.start, 8);
        assert_eq!(
            format!("{}{};", &text[..8], out[0].edit.replacement),
            "SELECT 1 LIMIT 100;"
        );

        // 字面量里的分号不会切句（切分器与词法是同一份口径）
        let text = "SELECT 'a;b' FROM t";
        let out = actions(text, 8);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].edit.start, text.len());
    }

    #[test]
    fn an_unfiltered_update_or_delete_gets_the_safety_where() {
        for (text, cursor) in [
            ("UPDATE t SET x = 1", 0),
            ("DELETE FROM t", 12),
            ("DELETE FROM t /* 备注 */", 5),
        ] {
            let out = actions(text, cursor);
            assert_eq!(out.len(), 1, "{text}");
            assert_eq!(out[0].id, ADD_WHERE, "{text}");
            assert_eq!(out[0].edit.start, text.len(), "{text}");
            assert_eq!(out[0].edit.replacement, " WHERE 1 = 0", "{text}");
        }
    }

    #[test]
    fn a_word_inside_a_literal_or_comment_is_not_a_clause() {
        // 词法层说了算：字符串 / 块注释里的 `where` 不算已有 WHERE → 照样给动作
        for text in [
            "UPDATE t SET note = 'where'",
            "UPDATE t SET x = 1 /* where */",
        ] {
            assert_eq!(ids(text, 0), vec![ADD_WHERE], "{text}");
        }
        // 反过来：字面量里的 `limit` 不算已有上限 → 照样给 LIMIT
        assert_eq!(
            ids("SELECT 'limit' AS note FROM t", 0),
            vec![ADD_LIMIT],
            "字面量里的 limit 不算上限"
        );
    }

    #[test]
    fn the_selection_wins_over_the_cursor() {
        let text = "SELECT 1;\nSELECT * FROM t;";
        let second = text.find("SELECT *").expect("有");
        let out = code_actions(&Request {
            cursor: 0,
            selection: Some((second, text.len())),
            ..ask(text, &Catalog::default())
        });
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].id, ADD_LIMIT);
        assert_eq!(
            out[0].edit.start, 25,
            "改的是选中的第二条（末尾分号不算判据，追加落在它前面）"
        );
    }

    #[test]
    fn a_cursor_after_the_last_statement_targets_it() {
        let text = "SELECT 1;\nSELECT * FROM t;\n";
        let out = actions(text, text.len());
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].edit.start, 25, "中间只剩空白与分号 → 认最后那条");
    }

    #[test]
    fn a_cursor_inside_the_statement_includes_its_edges() {
        let text = "SELECT * FROM t";
        assert_eq!(ids(text, 0), vec![ADD_LIMIT], "语句起点");
        assert_eq!(ids(text, text.len()), vec![ADD_LIMIT], "语句末尾");
        assert_eq!(ids(text, 8), vec![ADD_LIMIT], "语句中间");
    }

    // ---------------- code action：回绝 ----------------

    #[test]
    fn the_limit_action_declines_what_it_cannot_place() {
        let cases: [(&str, &str); 14] = [
            ("SELECT * FROM t LIMIT 10", "已经有上限"),
            ("SELECT TOP 10 * FROM t", "T-SQL 的 TOP"),
            ("SELECT * FROM t FETCH FIRST 10 ROWS ONLY", "FETCH"),
            ("SELECT * FROM t OFFSET 10", "OFFSET 必须排在 LIMIT 前"),
            ("SELECT * FROM t FOR UPDATE", "FOR UPDATE 也得排在 LIMIT 前"),
            ("SELECT * FROM t INTO OUTFILE 'x'", "INTO 的落点判不出"),
            (
                "SELECT * FROM t UNION ALL SELECT * FROM u LIMIT 1",
                "UNION 那条已有上限",
            ),
            (
                "WITH x AS (SELECT 1) SELECT * FROM x",
                "首关键字不是 SELECT",
            ),
            ("INSERT INTO t VALUES (1)", "不是 SELECT"),
            ("SELECT * FROM", "末尾挂着 FROM（还没写完）"),
            ("SELECT * FROM t,", "末尾是逗号"),
            ("SELECT * FROM t WHERE x =", "末尾是运算符"),
            ("SELECT", "只有一个 SELECT"),
            ("SELECT * FROM t -- 尾巴", "末尾是行注释（插进去会被吃掉）"),
        ];
        for (text, why) in cases {
            assert!(
                !ids(text, text.len()).contains(&ADD_LIMIT),
                "{text}（{why}）"
            );
        }
        // 这几条连别的动作也不该有（不是 UPDATE / DELETE，也不是能改写的 SELECT）
        for text in ["INSERT INTO t VALUES (1)", "SELECT * FROM", "SELECT"] {
            assert!(actions(text, text.len()).is_empty(), "{text}");
        }
    }

    #[test]
    fn the_where_action_declines_what_it_cannot_place() {
        let cases: [(&str, &str); 11] = [
            ("UPDATE t SET x = 1 WHERE id = 2", "已经有 WHERE"),
            ("DELETE FROM t WHERE 1 = 1", "已经有 WHERE"),
            ("UPDATE t SET x = 1 ORDER BY id", "ORDER BY 在 WHERE 之后"),
            ("DELETE FROM t LIMIT 1", "LIMIT 在 WHERE 之后"),
            ("DELETE FROM t RETURNING id", "RETURNING 在 WHERE 之后"),
            (
                "UPDATE t SET x = (SELECT 1 FROM u WHERE u.id = 1)",
                "子查询里有 WHERE（保守回绝）",
            ),
            ("SELECT * FROM t", "不是 UPDATE / DELETE"),
            ("INSERT INTO t VALUES (1)", "不是 UPDATE / DELETE"),
            ("WITH x AS (SELECT 1) DELETE FROM t", "首关键字不是 DELETE"),
            ("DELETE FROM", "末尾挂着 FROM"),
            ("UPDATE t SET", "末尾挂着 SET"),
        ];
        for (text, why) in cases {
            assert!(
                !ids(text, text.len()).contains(&ADD_WHERE),
                "{text}（{why}）"
            );
        }
        // 首关键字不对 / 还没写完的那几条：连 LIMIT 也不该有（一个动作都没有）
        for text in [
            "INSERT INTO t VALUES (1)",
            "WITH x AS (SELECT 1) DELETE FROM t",
            "DELETE FROM",
            "UPDATE t SET",
        ] {
            assert!(actions(text, text.len()).is_empty(), "{text}");
        }
    }

    #[test]
    fn an_unfinished_statement_gets_no_action() {
        // 还在写字符串 / 注释：连首关键字都说不准
        assert!(actions("SELECT 'abc", 0).is_empty());
        assert!(actions("DELETE FROM t /* 备注", 0).is_empty());
    }

    #[test]
    fn a_rewrite_is_not_offered_on_a_channel_where_the_statement_cannot_run() {
        let text = "DELETE FROM t";
        let on_local = code_actions(&Request {
            channel: ExecChannel::Accelerated,
            ..ask(text, &Catalog::default())
        });
        assert!(
            on_local.is_empty(),
            "本地加速通道上这条 DELETE 会被通道闸拒，改写没有意义"
        );
        let on_source = code_actions(&Request {
            channel: ExecChannel::Source,
            ..ask(text, &Catalog::default())
        });
        assert_eq!(on_source.len(), 1, "源库通道上照给");

        // SELECT 在任何通道上都跑得动（通道闸只拦写源库对象的语句）→ 不受影响
        let select = "SELECT * FROM t";
        let on_local = code_actions(&Request {
            channel: ExecChannel::Accelerated,
            ..ask(select, &Catalog::default())
        });
        assert_eq!(on_local.len(), 1);
    }

    #[test]
    fn a_cursor_between_statements_targets_nothing() {
        let text = "SELECT 1;\nSELECT * FROM t;\n";
        let between = text.find('\n').expect("有");
        assert!(
            actions(text, between).is_empty(),
            "两条语句之间的空隙：前后都可能，不猜"
        );
        assert!(actions("", 0).is_empty(), "空文档");
        assert!(actions("   \n", 2).is_empty(), "只有空白");
        assert!(actions("-- 只有注释\n", 3).is_empty(), "只有注释");
    }

    #[test]
    fn a_selection_that_spans_statements_targets_nothing() {
        let text = "SELECT 1;\nSELECT 2;";
        let out = code_actions(&Request {
            selection: Some((0, text.len())),
            ..ask(text, &Catalog::default())
        });
        assert!(out.is_empty(), "跨语句的选区：改哪条判不出");
    }

    // ---------------- 面一：补全 ----------------

    #[test]
    fn the_completion_face_asks_the_completion_module() {
        let catalog = catalog_with_orders();
        let out = complete(&ask("SELECT * FROM ord", &catalog));
        assert!(
            out.iter().any(|one| one.label == "public.orders"),
            "{out:?}"
        );
    }

    #[test]
    fn metadata_candidates_are_not_taken_without_a_bound_connection() {
        let catalog = catalog_with_orders();
        let bound = complete(&ask("SELECT * FROM ord", &catalog));
        assert!(bound.iter().any(|one| one.label == "public.orders"));

        let unbound = complete(&Request {
            connection: None,
            ..ask("SELECT * FROM ord", &catalog)
        });
        assert!(
            !unbound.iter().any(|one| one.label == "public.orders"),
            "未绑定连接 = 没有目标库：不摆元数据候选（如实，不假装有）"
        );

        // 没元数据 ≠ 没有补全：关键字照给
        let keywords = complete(&Request {
            connection: None,
            ..ask("sel", &catalog)
        });
        assert!(
            keywords.iter().any(|one| one.label == "SELECT"),
            "{keywords:?}"
        );
    }

    #[test]
    fn out_of_range_offsets_are_clamped_not_panicked() {
        let catalog = catalog_with_orders();
        // 越界偏移 + 落在中文中间（`名` 的第二、三个字节）
        let text = "SELECT 名称 FROM ord";
        for cursor in [0, 1, 8, 9, text.len() + 99] {
            let _ = complete(&Request {
                cursor,
                ..ask(text, &catalog)
            });
        }
    }

    // ---------------- 一次拿齐 ----------------

    #[test]
    fn the_report_carries_all_three_faces() {
        let catalog = catalog_with_orders();
        let out = report(&ask("DELETE FROM ord", &catalog));
        assert!(out.diagnostics.is_empty(), "这句词法上没问题");
        assert_eq!(
            out.completion.first().map(|one| one.label.as_str()),
            Some("public.orders")
        );
        assert_eq!(out.code_actions.len(), 1);
        assert_eq!(out.code_actions[0].id, ADD_WHERE);

        // 扫不动的那句：诊断有、动作没有（连"是不是 DELETE"都说不准）
        let broken = report(&ask("DELETE FROM ord 'abc", &catalog));
        assert_eq!(broken.diagnostics.len(), 1, "{:?}", broken.diagnostics);
        assert!(broken.code_actions.is_empty());
    }

    #[test]
    fn the_same_lexis_answers_all_three_faces() {
        // 一句话把三面同源摆出来：`'where'` 是字符串 → 不产生诊断、也照样挡不住动作；
        // 而真有 `WHERE` 时动作就没了
        let catalog = catalog_with_orders();
        let out = report(&ask("DELETE FROM t WHERE note = 'where'", &catalog));
        assert!(out.diagnostics.is_empty());
        assert!(out.code_actions.is_empty(), "已经有 WHERE");

        let out = report(&ask("DELETE FROM t", &catalog));
        assert_eq!(out.code_actions.len(), 1);
        assert_eq!(out.code_actions[0].id, ADD_WHERE);
    }
