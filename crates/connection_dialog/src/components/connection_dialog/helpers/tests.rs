    // 注意：不通配导入（`super::*` 会把 gpui 的 `test` 宏带入作用域）。
    use super::{
        DriverDerived, address_field, address_label, address_placeholder, app_level_capabilities,
        auth_config_values, auth_field_specs, build_auth_config_json, build_network_config_json,
        capability_rows, conn_display_name, create_new_db_file, default_new_scope,
        dialog_row_height, dialog_tab_defs,
        driver_auth_types, driver_capabilities, driver_form_fields, driver_short_name,
        enabled_drivers_of_type, field_spec, find_driver_by_value, first_selectable_type,
        is_secret_field, network_config_values, network_field_specs, new_db_file_suggested_name,
        policy_summary, policy_type_from_label, policy_type_label, property_note,
        result_needs_detail, save_blocker, saved_result, scope_from_label,
        staging_display_type_id,
        strip_file_db_noise, tags_from_json, tags_to_json, tr, type_badge, type_has_driver,
        type_matches_filter, url_template_example, visible_tab_index,
    };
    use connection::model::ConnectionScope;
    use connection::model::DataSourceSaveInput;
    use engine::persistence::driver_store::{DataSourceType, Driver};

    fn driver(id: &str, type_id: &str, name: &str, enabled: bool) -> Driver {
        Driver {
            id: id.into(),
            type_id: type_id.into(),
            name: name.into(),
            driver_kind: "native".into(),
            is_file: false,
            default_port: None,
            url_template: None,
            download_url: None,
            download_checksum: None,
            version: None,
            config_schema: String::new(),
            supported_auth_types: None,
            capabilities: None,
            driver_properties: None,
            enabled,
        }
    }

    fn ds_type(id: &str, name: &str, icon: Option<&str>) -> DataSourceType {
        DataSourceType {
            id: id.into(),
            name: name.into(),
            category: "relational".into(),
            icon: icon.map(|s| s.to_string()),
            enabled: true,
            created_at: String::new(),
        }
    }

    #[test]
    fn type_filter_matches_name_id_and_driver_name() {
        let mysql = ds_type("mysql", "MySQL", Some("🐬"));
        let pg = ds_type("postgresql", "PostgreSQL", Some("🐘"));
        let drivers = vec![driver("mysql_native", "mysql", "MySQL (Official)", true)];

        // 空过滤 = 全过（侧栏默认显示全部）。
        assert!(type_matches_filter(&mysql, &drivers, "  "));
        // 类型名 / 类型 id：大小写不敏感。
        assert!(type_matches_filter(&mysql, &drivers, "MySqL"));
        assert!(type_matches_filter(&pg, &drivers, "postgre"));
        // 驱动名命中“该类型”（`Official` 只出现在 mysql 的驱动上）。
        assert!(type_matches_filter(&mysql, &drivers, "official"));
        assert!(!type_matches_filter(&pg, &drivers, "official"));
    }

    #[test]
    fn keyboard_type_match_prefers_a_type_with_a_driver() {
        // 两个类型都命中 “post”，只有后者有启用驱动 → 选后者（不把用户送到必然被拒的路上）。
        let types = vec![
            ds_type("postgresql", "PostgreSQL", None),
            ds_type("postgres_legacy", "Postgres 旧实现", None),
        ];
        let drivers = vec![driver(
            "pg_sqlx",
            "postgres_legacy",
            "PostgreSQL (sqlx)",
            true,
        )];
        assert_eq!(
            first_selectable_type(&types, &drivers, "post").as_deref(),
            Some("postgres_legacy")
        );
        // 命中项全都无驱动：退回第一个命中项（让 `select_type` 给出「暂无可用驱动」原因行，
        // 而不是回车后什么都没发生）。
        assert_eq!(
            first_selectable_type(&types, &[], "post").as_deref(),
            Some("postgresql")
        );
        // 无命中：None（回车不改已选类型）。
        assert_eq!(first_selectable_type(&types, &drivers, "oracle"), None);
    }

    #[test]
    fn row_height_follows_the_viewport_and_has_a_floor() {
        // 常用窗口（1080px 高 ≈ 67.5rem）：预算 = 67.5 × 0.9 − 5.5 = 55.25rem > 基准 → 按设计基准。
        assert_eq!(dialog_row_height(67.5).0, crate::ui::DIALOG_BODY_HEIGHT);
        // 768 物理高 + 125% 缩放 ≈ 614px ≈ 38.4rem：预算 = 29.06rem → 夹小（否则 footer 出屏）。
        let capped = dialog_row_height(38.4).0;
        assert!(
            capped < crate::ui::DIALOG_BODY_HEIGHT,
            "矮窗口应夹住：{capped}"
        );
        assert!(capped > 16.0, "夹住后仍应有可用高度：{capped}");
        // 极矮：下限 16rem（内部滚动，不压成一条缝）。
        assert_eq!(dialog_row_height(10.0).0, 16.0);
    }

    #[test]
    fn driver_derived_parses_declarations_and_keys_on_them() {
        let mut d = driver("mysql_native", "mysql", "MySQL (Official)", true);
        d.config_schema = r#"{"fields":[{"key":"host","label":"主机"}]}"#.to_string();
        d.capabilities = Some(r#"["transactions","ssl"]"#.to_string());
        d.supported_auth_types = Some(r#"["password","ssl"]"#.to_string());

        let derived = DriverDerived::resolve(Some(&d));
        assert_eq!(derived.form_fields.len(), 1);
        assert_eq!(derived.form_fields[0].label, "主机");
        assert_eq!(derived.capabilities, vec!["transactions", "ssl"]);
        assert_eq!(derived.auth_types, vec!["password", "ssl"]);

        // 键只看驱动 id + 三段声明原文：声明未改则键稳定（缓存命中），改了就换键（失效重算）。
        assert_eq!(DriverDerived::cache_key(&d), derived.key);
        let mut changed = d.clone();
        changed.capabilities = Some(r#"["ssl"]"#.to_string());
        assert_ne!(DriverDerived::cache_key(&changed), derived.key);

        // 未选驱动 → 全空（不为“未选”造默认字段）。
        let empty = DriverDerived::resolve(None);
        assert!(empty.key.is_empty());
        assert!(empty.form_fields.is_empty());
        assert!(empty.capabilities.is_empty());
        assert!(empty.auth_types.is_empty());
    }

    #[test]
    fn network_field_specs_cover_supported_types() {
        for t in [
            "ssh",
            "SSH",
            "ssh_tunnel",
            "proxy",
            "http",
            "socks5",
            "TLS",
            "ssl",
        ] {
            assert!(!network_field_specs(t).is_empty(), "{t} 应有字段声明");
        }
        assert!(
            network_field_specs("chain").is_empty(),
            "协议链走原始 JSON 文本框（跳太多，不逐项展开）"
        );
    }

    #[test]
    fn network_config_json_matches_engine_models() {
        // 关键回归：组装出的 JSON 必须能被 `connection::config` 的 serde 模型直接反序列化。
        let proxy = build_network_config_json(
            "proxy",
            &[
                ("host".into(), "127.0.0.1".into()),
                ("port".into(), "3128".into()),
                ("username".into(), "u".into()),
                ("password".into(), "p".into()),
                ("no_proxy".into(), "localhost, 127.0.0.1".into()),
            ],
        )
        .expect("build proxy");
        let parsed: connection::config::ProxyConfig =
            serde_json::from_str(&proxy).expect("proxy JSON 可反序列化");
        assert_eq!(parsed.host, "127.0.0.1");
        assert_eq!(parsed.port, 3128);
        assert_eq!(parsed.auth.as_ref().map(|a| a.username.as_str()), Some("u"));
        assert_eq!(
            parsed.no_proxy,
            vec!["localhost".to_string(), "127.0.0.1".to_string()]
        );

        // SSH 密码认证（端口留空 → 模型默认 22）。
        let ssh = build_network_config_json(
            "SSH",
            &[
                ("host".into(), "jump".into()),
                ("username".into(), "deploy".into()),
                ("password".into(), "pw".into()),
                ("remote_host".into(), "db".into()),
                ("remote_port".into(), "5432".into()),
            ],
        )
        .expect("build ssh");
        let parsed: connection::config::SshConfig =
            serde_json::from_str(&ssh).expect("ssh JSON 可反序列化");
        assert_eq!(parsed.host, "jump");
        assert_eq!(parsed.port, 22);
        assert!(matches!(
            parsed.auth,
            connection::config::SshAuth::Password { .. }
        ));

        // SSH 私钥认证（填了私钥路径 → auth_type 切 private_key）。
        let ssh_key = build_network_config_json(
            "ssh",
            &[
                ("host".into(), "jump".into()),
                ("username".into(), "deploy".into()),
                ("key_path".into(), "/home/u/.ssh/id_ed25519".into()),
                ("remote_host".into(), "db".into()),
                ("remote_port".into(), "5432".into()),
            ],
        )
        .expect("build ssh key");
        let parsed: connection::config::SshConfig =
            serde_json::from_str(&ssh_key).expect("ssh(私钥) JSON 可反序列化");
        assert!(matches!(
            parsed.auth,
            connection::config::SshAuth::PrivateKey { .. }
        ));

        let ssl = build_network_config_json(
            "ssl",
            &[
                ("verify".into(), "false".into()),
                ("ca".into(), "/etc/ssl/ca.pem".into()),
            ],
        )
        .expect("build ssl");
        let parsed: connection::config::SslConfig =
            serde_json::from_str(&ssl).expect("ssl JSON 可反序列化");
        assert!(!parsed.verify_server_cert);
        assert_eq!(parsed.ca_cert_path.as_deref(), Some("/etc/ssl/ca.pem"));
    }

    #[test]
    fn network_config_json_validates_required_ports_and_auth() {
        let err = build_network_config_json("proxy", &[("port".into(), "1080".into())])
            .expect_err("缺主机应报错");
        assert!(err.contains("必填"), "{err}");

        let err = build_network_config_json(
            "proxy",
            &[("host".into(), "h".into()), ("port".into(), "abc".into())],
        )
        .expect_err("端口非法应报错");
        assert!(err.contains(tr("端口")), "{err}");

        let err = build_network_config_json(
            "ssh",
            &[
                ("host".into(), "h".into()),
                ("username".into(), "u".into()),
                ("remote_host".into(), "d".into()),
                ("remote_port".into(), "1".into()),
            ],
        )
        .expect_err("无凭据应报错");
        assert!(err.contains(tr("密码")) && err.contains("私钥"), "{err}");

        let err = build_network_config_json("chain", &[]).expect_err("协议链不走字段编辑");
        assert!(err.contains("chain"), "{err}");
    }

    #[test]
    fn network_config_values_roundtrip_for_edit() {
        let json = build_network_config_json(
            "proxy",
            &[
                ("host".into(), "p".into()),
                ("port".into(), "3128".into()),
                ("no_proxy".into(), "localhost".into()),
            ],
        )
        .expect("build");
        let values = network_config_values("proxy", &json);
        let get = |k: &str| {
            values
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        assert_eq!(get("host"), "p");
        assert_eq!(get("port"), "3128");
        assert_eq!(get("no_proxy"), "localhost");
        assert_eq!(get("username"), "", "未填字段回填为空");

        // 非法 JSON：不 panic，字段全空（不把垃圾反填进表单）。
        let empty = network_config_values("proxy", "not-json");
        assert!(empty.is_empty());
    }

    #[test]
    fn auth_field_specs_cover_supported_types() {
        for t in [
            "password",
            "PASSWORD",
            "ldap",
            "pg_class",
            "kerberos",
            "ssh_key",
            "ssh",
            "proxy_pwd",
            "proxy",
        ] {
            assert!(!auth_field_specs(t).is_empty(), "{t} 应有字段声明");
        }
        assert!(
            auth_field_specs("unknown_new_auth").is_empty(),
            "未知类型没有声明 → 回落到原始 JSON 文本框"
        );
    }

    #[test]
    fn auth_config_json_uses_backend_keys() {
        // 键必须与后端读取端一致（`inject_auth_into_url` / `inject_{ssh,proxy}_auth_from_auth_data`）。
        let v = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(k, s)| ((*k).to_string(), (*s).to_string()))
                .collect()
        };

        let json = build_auth_config_json(
            "password",
            &v(&[("username", "alice"), ("password", "s3cret")]),
        )
        .expect("password 组装");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(parsed["username"], "alice");
        assert_eq!(parsed["password"], "s3cret");

        let json = build_auth_config_json(
            "pg_class",
            &v(&[("cert_path", "/c/c.pem"), ("cert_key_path", "/c/k.pem")]),
        )
        .expect("pg_class 组装");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(parsed["certPath"], "/c/c.pem", "后端读 certPath（驼峰）");
        assert_eq!(parsed["certKeyPath"], "/c/k.pem");

        let json = build_auth_config_json(
            "kerberos",
            &v(&[("principal", "u@REALM"), ("keytab_path", "/k.keytab")]),
        )
        .expect("kerberos 组装");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(parsed["principal"], "u@REALM");
        assert_eq!(parsed["keytabPath"], "/k.keytab");

        // SSH：私钥路径 + 口令；未填字段不写空值
        let json = build_auth_config_json(
            "ssh_key",
            &v(&[
                ("username", "deploy"),
                ("key_path", "~/.ssh/id"),
                ("passphrase", "pp"),
            ]),
        )
        .expect("ssh 组装");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(parsed["keyPath"], "~/.ssh/id");
        assert_eq!(parsed["passphrase"], "pp");
        assert_eq!(parsed["username"], "deploy");
        assert!(parsed.get("password").is_none(), "未填字段不写空串");

        // 只填口令不填私钥：口令被忽略（注入函数只在 keyPath 分支读它）
        let json = build_auth_config_json(
            "ssh_key",
            &v(&[("password", "ssh-pwd"), ("passphrase", "pp")]),
        )
        .expect("ssh 密码组装");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert!(parsed.get("passphrase").is_none());
        assert_eq!(parsed["password"], "ssh-pwd");
    }

    #[test]
    fn auth_config_json_validation_and_roundtrip() {
        let v = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(k, s)| ((*k).to_string(), (*s).to_string()))
                .collect()
        };

        // 必填（password 类型需要用户名）
        let err = build_auth_config_json("password", &v(&[("password", "p")]))
            .expect_err("缺用户名应报错");
        assert!(err.contains(tr("用户名")), "{err}");
        // 代理凭据成对
        let err = build_auth_config_json("proxy_pwd", &v(&[("username", "u")]))
            .expect_err("缺密码应报错");
        assert!(err.contains(tr("密码")), "{err}");
        // SSH 凭据二选一
        let err =
            build_auth_config_json("ssh_key", &v(&[("username", "u")])).expect_err("无凭据应报错");
        assert!(err.contains("私钥") || err.contains(tr("密码")), "{err}");
        // 未声明字段的类型：明确提示走原始 JSON
        let err = build_auth_config_json("oauth2", &[]).expect_err("未知类型应拒绝字段编辑");
        assert!(err.contains("不支持字段编辑"), "{err}");

        // 回填往返（proxy_pwd）
        let json = build_auth_config_json("proxy_pwd", &v(&[("username", "u"), ("password", "p")]))
            .expect("build");
        let values = auth_config_values("proxy_pwd", &json);
        let get = |k: &str| {
            values
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, x)| x.clone())
                .unwrap_or_default()
        };
        assert_eq!(get("username"), "u");
        assert_eq!(get("password"), "p");

        // 非法 JSON：不 panic，字段全空
        assert!(auth_config_values("password", "not-json").is_empty());
    }

    #[test]
    fn auth_config_json_is_readable_by_backend_injector() {
        // A5 核心闭环：UI 字段组装出的 `auth_data` 必须能被后端注入函数直接读懂。
        let json = build_auth_config_json(
            "password",
            &[
                ("username".to_string(), "alice".to_string()),
                ("password".to_string(), "pwd".to_string()),
            ],
        )
        .expect("build");
        let url =
            connection::url_params::inject_auth_into_url("postgres://h:5432/db", "password", &json)
                .expect("注入凭据");
        assert_eq!(url, "postgres://alice:pwd@h:5432/db");

        // SSH 私钥分支：键名必须是后端读的 `keyPath`（驼峰），否则等于没配。
        let json = build_auth_config_json(
            "ssh_key",
            &[("key_path".to_string(), "~/.ssh/id_ed25519".to_string())],
        )
        .expect("build ssh");
        assert!(json.contains("\"keyPath\""), "{json}");
    }

    #[test]
    fn saved_result_keeps_connection_id_out_of_the_summary() {
        // #32 B 案：界面与提示词只出现名称；连接 ID 只在「详情」里（排障时点开/复制）。
        let line = saved_result("生产 PG", "G_conn_prod");
        assert!(line.summary.contains("生产 PG"), "摘要应带名称");
        assert!(
            !line.summary.contains("G_conn_prod"),
            "摘要不得出现连接 ID（否则用户又把它当主键）"
        );
        assert!(!result_needs_detail(&line.summary), "短摘要本身不触发折叠");
        let detail = line.detail_text().to_string();
        assert!(detail.contains("G_conn_prod"), "详情带 ID，供排障/报障");
        assert!(line.detail.is_some(), "有详情才会出现「详情 / 复制」入口");

        // 空名回退占位，不出现空引号。
        assert!(
            saved_result("  ", "P_conn_1")
                .summary
                .contains("未命名连接")
        );
        assert_eq!(conn_display_name(" x "), "x");
    }

    #[test]
    fn save_blocker_reports_the_first_missing_item() {
        // 顺序就是填写顺序：类型 → 驱动 → 名称 → 地址（与 `collect` 的判据一致）。
        assert_eq!(
            save_blocker(false, "", "", "", false),
            Some(tr("请先在左侧选择数据库类型"))
        );
        assert_eq!(
            save_blocker(true, "", "n", "u", false),
            Some(tr("请选择驱动实现"))
        );
        assert_eq!(
            save_blocker(true, "sqlx", "", "u", false),
            Some(tr("请填写连接名称"))
        );
        // 地址缺口按连接方式给不同的可执行动作（文件型要的是“选文件”，不是“填 URI”）。
        assert_eq!(
            save_blocker(true, "sqlx", "n", "", false),
            Some(tr("请填写连接 URI"))
        );
        assert_eq!(
            save_blocker(true, "sqlx", "n", "", true),
            Some(tr("请选择或新建数据库文件（常规 → 连接设置）"))
        );
        // 全齐 / 全空格（名称与地址都是 trim 后判空）→ 无缺口。
        assert_eq!(save_blocker(true, "sqlx", "n", "u", false), None);
        assert_eq!(
            save_blocker(true, "  ", "  ", "  ", false),
            Some(tr("请选择驱动实现"))
        );
    }

    #[test]
    fn secret_fields_are_the_credential_keys_only() {
        // 掩码只该盖住凭据：路径 / 主机 / 端口这类字段掩了反而怪。
        assert!(is_secret_field("password"));
        assert!(is_secret_field("passphrase"));
        assert!(!is_secret_field("key_path"));
        assert!(!is_secret_field("host"));
        assert!(!is_secret_field(""));
    }

    #[test]
    fn dialog_tabs_hide_network_for_file_db_and_map_visible_index() {
        // #15 迁移到 `TabBar` 后的接缝：文件型隐藏「网络」→ 可见下标与内部索引错开一位，
        // 错了就会“点能力却显示网络内容”。
        let net = dialog_tab_defs(false);
        assert_eq!(net.len(), 5);
        assert_eq!(visible_tab_index(&net, 0), 0);
        assert_eq!(visible_tab_index(&net, 4), 4);

        let file = dialog_tab_defs(true);
        assert_eq!(file.len(), 4, "文件型不显示网络 Tab");
        assert!(!file.iter().any(|(label, _)| *label == "网络"));
        assert_eq!(visible_tab_index(&file, 0), 0);
        assert_eq!(
            visible_tab_index(&file, 2),
            1,
            "能力在文件型里是第 2 个可见项"
        );
        assert_eq!(visible_tab_index(&file, 4), 3);
        // 隐藏的「网络」被选中（旧草稿 / 切类型后）回退到第一个可见项，不越界。
        assert_eq!(visible_tab_index(&file, 1), 0);

        for (label, ix) in &file {
            assert_eq!(
                file.get(visible_tab_index(&file, *ix)).map(|(l, _)| *l),
                Some(*label),
                "可见下标应能反查回同一个 Tab"
            );
        }
    }

    #[test]
    fn result_detail_needed_only_for_long_or_multiline_summaries() {
        assert!(!result_needs_detail("已保存：G_conn_x"));
        assert!(!result_needs_detail(&"字".repeat(80)), "刚好 80 字不展开");
        assert!(result_needs_detail(&"字".repeat(81)), "超 80 字提供详情");
        assert!(result_needs_detail("第一行\n第二行"), "换行必须可展开");
    }

    /// 属性行的「去向」提示：四种命运各给一句人话，直通的不啰嗝。
    ///
    /// 判据全部在引擎的 `property_spec`（库认的键 + 未知键的效果），这里只验证翻译层。
    #[test]
    fn property_notes_state_what_will_actually_happen() {
        use super::PropertyNoteLevel as L;

        // 直通同名、无说明 → 不给提示（不啰嗝）；带中性说明 → Info
        assert!(property_note("mysql", "collation").is_none());
        assert!(property_note("mysql_native", "stmt_cache_size").is_none());
        let (text, level) = property_note("mysql", "charset").expect("charset 有说明");
        assert!(text.contains("utf8mb4"), "{text}");
        assert_eq!(level, L::Info);

        // 带副作用的键（会覆盖别的设置）：警告级
        let (text, level) = property_note("mysql_native", "require_ssl").expect("TLS 键应有提醒");
        assert!(text.contains("连接安全"), "{text}");
        assert_eq!(level, L::Warn);

        // sqlx：未知键不会被应用 → 警告（写了不生效）
        let (text, level) = property_note("mysql", "connectTimeout").expect("未知键应提示");
        assert!(text.contains("不会被应用"), "{text}");
        assert_eq!(level, L::Warn);

        // native：未知键会报错 → 危险（连接失败）
        let (text, level) = property_note("mysql_native", "ssl_mode").expect("未知键应提示");
        assert!(text.contains("报错"), "{text}");
        assert_eq!(level, L::Danger);
        let (_, level) = property_note("postgres_native", "connectTimeout").expect("应提示");
        assert_eq!(level, L::Danger);

        // 文件型：PRAGMA / SET 由驱动侧在开库时应用（“会生效”，不是“不理”）
        let (text, level) = property_note("sqlite", "journalMode").expect("应提示");
        assert!(text.contains("由驱动应用为 journal_mode"), "{text}");
        assert_eq!(level, L::Info);
        assert!(property_note("duckdb", "threads").is_some());
        // 文件型里带副作用的键（会覆盖应用默认）：警告级
        let (text, level) = property_note("sqlite", "foreign_keys").expect("应提示");
        assert!(text.contains("违规写入"), "{text}");
        assert_eq!(level, L::Warn);
        // 文件型里**不在清单**的键：驱动不应用（界面不能说它会生效）
        let (text, level) = property_note("sqlite", "magic").expect("应提示");
        assert!(text.contains("不会被应用"), "{text}");
        assert_eq!(level, L::Warn);

        // 未收录的驱动：不装作知道
        let (text, level) = property_note("some_plugin_driver", "charset").expect("应提示");
        assert!(text.contains("未收录"), "{text}");
        assert_eq!(level, L::Info);
    }

    #[test]
    fn new_db_file_suggested_name_follows_driver() {
        // 建议文件名按驱动 id 判族（与 `is_file_db` 同一来源），否则类型 / 驱动两套事实会打架。
        assert_eq!(new_db_file_suggested_name("duckdb"), "new_database.duckdb");
        assert_eq!(new_db_file_suggested_name("DUCKDB"), "new_database.duckdb");
        assert_eq!(new_db_file_suggested_name("sqlite"), "new_database.db");
        assert_eq!(
            new_db_file_suggested_name(""),
            "new_database.db",
            "未知驱动回退 .db"
        );
    }

    #[test]
    fn default_new_scope_is_project_only() {
        // 规则 1（决策 #110）：新建对话框默认作用域 =「仅项目」（未显式选择时的渲染回退值）。
        assert_eq!(default_new_scope(), "仅项目");
        // 与 scope 常量索引口径一致：SCOPE_LABELS[1] 即「仅项目」。
        assert_eq!(scope_from_label(default_new_scope()), ConnectionScope::Project);
    }

    #[test]
    fn create_new_db_file_rejects_existing_and_creates_missing() {
        let dir = std::env::temp_dir().join(format!("rds_newfile_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");

        // 不存在：DuckDB 必须建出**有效库文件**（非空、写文件头，立即可连——
        // DuckDB 对 0 字节空文件只校验文件头、不初始化，真机反馈 "not a valid DuckDB database file"）
        let path = dir.join("new.duckdb");
        let (msg, ok, value) = create_new_db_file(&path, "duckdb");
        assert!(ok, "{msg}");
        assert_eq!(
            value.as_deref(),
            Some(path.to_string_lossy().to_string().as_str())
        );
        assert!(
            std::fs::metadata(&path).expect("meta").len() > 0,
            "DuckDB 新建必须是有效库文件（非空）"
        );

        // 不存在：SQLite 保持空文件契约（首次连接由驱动初始化结构）
        let sqlite_path = dir.join("new.db");
        let (msg, ok, _value) = create_new_db_file(&sqlite_path, "sqlite");
        assert!(ok, "{msg}");
        assert_eq!(
            std::fs::metadata(&sqlite_path).expect("meta").len(),
            0,
            "SQLite 新建必须仍是空文件"
        );

        // 已存在且非空：不采用、不覆盖（「新建」与「打开」职责互斥：真机反馈“新建为什么还是打开”）
        std::fs::write(&path, b"keep-me").expect("write");
        let (msg, ok, value) = create_new_db_file(&path, "duckdb");
        assert!(!ok && value.is_none(), "{msg}");
        assert!(msg.contains("已存在"), "{msg}");
        assert_eq!(
            std::fs::read(&path).expect("read"),
            b"keep-me",
            "已存在的文件内容不得被覆盖"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tags_text_and_json_roundtrip() {
        // 中英文逗号 + 空白 + 去重；空输入 → None（不写空 JSON）。
        assert_eq!(
            tags_to_json("prod, core，prod  ,  "),
            Some(r#"["prod","core"]"#.to_string())
        );
        assert_eq!(tags_to_json("   "), None);
        assert_eq!(tags_from_json(Some(r#"["prod","core"]"#)), "prod, core");
        assert_eq!(tags_from_json(Some("not-json")), "");
        assert_eq!(tags_from_json(None), "");
    }

    #[test]
    fn driver_form_fields_come_from_config_schema() {
        // 空 / 非法 / 缺 fields：不得造默认字段（调用方自行回退）。
        assert!(driver_form_fields("").is_empty());
        assert!(driver_form_fields("not-json").is_empty());
        assert!(driver_form_fields(r#"{"options":[]}"#).is_empty());
        // 真实种子：SQLite 只声明 file_path（type=file）。
        let sqlite = r#"{"fields":[{"key":"file_path","label":"数据库文件","type":"file","required":true,"placeholder":"选择 .db 或 .sqlite 文件"}]}"#;
        let fields = driver_form_fields(sqlite);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].key, "file_path");
        assert_eq!(fields[0].label, "数据库文件");
        assert_eq!(fields[0].kind, "file");
        assert!(fields[0].required);
        assert_eq!(
            address_field(&fields).map(|f| f.key.as_str()),
            Some("file_path")
        );
        // 真实种子：MySQL 声明 host/port/database/username/password（顺序保留）。
        let mysql = r#"{"fields":[{"key":"host","label":"主机","type":"text","required":true},{"key":"port","label":"端口","type":"number","required":true},{"key":"database","label":"数据库","type":"text"},{"key":"username","label":"用户名","type":"text"},{"key":"password","label":"密码","type":"password"}]}"#;
        let fields = driver_form_fields(mysql);
        assert_eq!(fields.len(), 5);
        assert_eq!(fields[1].kind, "number");
        assert_eq!(fields[4].kind, "password");
        assert_eq!(
            field_spec(&fields, "database").map(|f| f.label.as_str()),
            Some("数据库")
        );
        // 未声明的键 → None（调用方据此不出该行，而不是造默认行）。
        assert!(field_spec(&fields, "file_path").is_none());
        // 缺 key / key 为空 → 丢弃该字段。
        let broken = r#"{"fields":[{"label":"无键"},{"key":"  ","label":"空键"},{"key":"ok"}]}"#;
        let fields = driver_form_fields(broken);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].key, "ok");
        assert_eq!(fields[0].label, "ok", "未声明 label 时回退键名");
    }

    #[test]
    fn short_name_strips_type_prefix() {
        assert_eq!(driver_short_name("MySQL (sqlx)"), "sqlx");
        assert_eq!(driver_short_name("MySQL (Official)"), "Official");
        assert_eq!(driver_short_name("SQLite (rusqlite)"), "rusqlite");
        assert_eq!(driver_short_name("DuckDB（duckdb-rs）"), "duckdb-rs");
        // 无括号 / 空括号：原样返回（不丢信息）。
        assert_eq!(driver_short_name("native"), "native");
        assert_eq!(driver_short_name("MySQL ()"), "MySQL ()");
        assert_eq!(driver_short_name("  sqlx  "), "sqlx");
    }

    #[test]
    fn driver_lookup_matches_id_full_name_and_short() {
        let drivers = vec![
            driver("mysql_native", "mysql", "MySQL (Official)", true),
            driver("mysql", "mysql", "MySQL (sqlx)", true),
            driver("postgres", "postgresql", "PostgreSQL (sqlx)", true),
        ];
        assert_eq!(
            find_driver_by_value(&drivers, "mysql").map(|d| d.id.as_str()),
            Some("mysql")
        );
        assert_eq!(
            find_driver_by_value(&drivers, "MySQL (Official)").map(|d| d.id.as_str()),
            Some("mysql_native")
        );
        assert_eq!(
            find_driver_by_value(&drivers, "Official").map(|d| d.id.as_str()),
            Some("mysql_native")
        );
        assert!(find_driver_by_value(&drivers, "").is_none());
        assert!(find_driver_by_value(&drivers, "oracle").is_none());
    }

    #[test]
    fn enabled_drivers_are_filtered_by_type() {
        let drivers = vec![
            driver("mysql", "mysql", "MySQL (sqlx)", true),
            driver("mysql_native", "mysql", "MySQL (Official)", false),
            driver("postgres", "postgresql", "PostgreSQL (sqlx)", true),
        ];
        let mysql = enabled_drivers_of_type(&drivers, "mysql");
        assert_eq!(mysql.len(), 1, "禁用驱动不应出现在下拉中");
        assert_eq!(mysql[0].id, "mysql");
        assert_eq!(enabled_drivers_of_type(&drivers, "oracle").len(), 0);
    }

    #[test]
    fn type_has_driver_requires_enabled_driver() {
        let drivers = vec![
            driver("mysql", "mysql", "MySQL (sqlx)", true),
            driver("oracle_legacy", "oracle", "Oracle (Legacy)", false),
        ];
        assert!(type_has_driver(&drivers, "mysql"));
        // 只有禁用驱动的类型视为不可用（选型入口置灰 + 拒绝切换）。
        assert!(!type_has_driver(&drivers, "oracle"));
        // 目录里完全没有驱动的类型同样不可用（与种子目录一致：只内置 4 个驱动）。
        assert!(!type_has_driver(&drivers, "clickhouse"));
    }

    /// 能力矩阵只列**驱动能力**：应用级功能（导出 / Mock / 资源）不能当行出现，
    /// 否则界面上会被读成「这个驱动不支持数据导出」；功能未实现的键要标明白。
    #[test]
    fn capability_rows_exclude_app_level_features() {
        let rows = capability_rows(&[]);
        assert!(
            !rows.iter().any(|r| r.label == "数据导出"),
            "应用级功能不该出现在驱动能力矩阵里：{:?}",
            rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>()
        );
        assert!(!rows.iter().any(|r| r.label == "Mock 生成"));
        assert!(!rows.iter().any(|r| r.label == "资源分析"));
        // 驱动能力仍在（能逐行对比）
        assert!(rows.iter().any(|r| r.label == "事务"));
        assert!(rows.iter().any(|r| r.label == "数据库导航"));

        // 功能未实现的键：行里带 not_built，且**永不**是 declared（谁都不能声明它）
        let index_row = rows
            .iter()
            .find(|r| r.label == "索引分析")
            .expect("索引分析应在矩阵里（带“功能未实现”说明）");
        assert!(index_row.not_built);
        assert!(!index_row.declared);
        let editor_row = rows
            .iter()
            .find(|r| r.label == "表编辑器")
            .expect("表编辑器应存在");
        assert!(editor_row.not_built && !editor_row.declared);
        // 已实现的键不带这个标记
        let tx = rows.iter().find(|r| r.label == "事务").expect("事务应存在");
        assert!(!tx.not_built);

        // 三个应用级功能走另一句说明
        let app = app_level_capabilities();
        assert!(app.contains(&"数据导出".to_string()), "{app:?}");
        assert!(app.contains(&"Mock 生成".to_string()));
        assert!(app.contains(&"资源分析".to_string()));
    }

    #[test]
    fn capabilities_come_from_driver_json_with_label_dictionary() {
        // 解析：来自库里的 JSON 数组（非法 / 空 → 空列表，不造默认能力）。
        assert_eq!(driver_capabilities(None).len(), 0);
        assert_eq!(driver_capabilities(Some("not-json")).len(), 0);
        assert_eq!(
            driver_capabilities(Some(r#"["tree","health_check"]"#)),
            vec!["tree".to_string(), "health_check".to_string()]
        );
        // 矩阵：字典（引擎侧唯一一份）内的**驱动能力**逐个出一行（声明与否），
        // 驱动自带的未知键追加在尾部；行里同时带出运行时位与真机验收状态。
        // （应用级功能不在这里：见 `capability_rows_exclude_app_level_features`）
        let declared = vec!["tree".to_string(), "brand_new".to_string()];
        let rows = capability_rows(&declared);
        assert!(
            rows.iter().any(|r| r.label == "数据库导航" && r.declared),
            "声明项应命中"
        );
        assert!(
            rows.iter().any(|r| r.label == "模式浏览" && !r.declared),
            "字典内未声明项应标记未声明"
        );
        assert_eq!(rows.last().map(|r| r.label.as_str()), Some("brand_new"));
        // 未收录的键不假装验收（也不丢信息）
        assert!(!rows.last().expect("尾部行").acceptance.verified);
        // 事务 / 联邦两个键带运行时位；已验收项带用例名（D10 口径）
        let tx = rows.iter().find(|r| r.label == "事务").expect("事务行");
        assert_eq!(tx.meta_bit, Some(engine::driver::MetaBit::Transaction));
        assert!(tx.acceptance.verified);
        assert!(
            !tx.acceptance.evidence.is_empty(),
            "已验收必须给出可复现用例"
        );
    }

    #[test]
    fn policy_type_label_and_summary_are_db_driven() {
        // 标签字典与反向映射必须成对（管理器按库里的 policy_type 展示、按标签写回）。
        for t in ["security", "schema", "performance", "audit", "ui"] {
            let label = policy_type_label(t);
            assert_eq!(policy_type_from_label(&label), t, "标签与类型应可往返：{t}");
        }
        // 未收录的类型原样展示（不丢失库里的信息）。
        assert_eq!(policy_type_label("custom"), "custom");
        // 摘要只取库里的标量值，最多 3 项；空/非法 → “—”。
        assert_eq!(policy_summary(None), "—");
        assert_eq!(policy_summary(Some("{}")), "—");
        let s = policy_summary(Some(
            r#"{"readonly":true,"rowLimit":1000,"queryTimeout":60,"nested":{"a":1}}"#,
        ));
        assert!(s.contains("readonly=true"), "{s}");
        assert!(s.contains("rowLimit=1000"), "{s}");
        assert!(!s.contains("nested"), "对象值不应进摘要：{s}");
        assert_eq!(s.matches('=').count(), 3, "最多 3 项：{s}");
    }

    #[test]
    fn auth_types_come_from_driver_declaration() {
        // 数据源：drivers.supported_auth_types（JSON 数组）；非法 / 空 → 空列表（UI 置灰）。
        assert_eq!(driver_auth_types(None).len(), 0);
        assert_eq!(driver_auth_types(Some("not-json")).len(), 0);
        assert_eq!(driver_auth_types(Some("[]")).len(), 0);
        assert_eq!(
            driver_auth_types(Some(r#"["password","ssl"]"#)),
            vec!["password".to_string(), "ssl".to_string()]
        );
    }

    #[test]
    fn address_placeholder_and_label_follow_driver() {
        // 标签：文件型 = 地址，网络型 = URI。
        assert_eq!(address_label(true), tr("地址"));
        assert_eq!(address_label(false), tr("URI"));

        // 网络型：占位来自驱动声明的 url_template + 默认端口（不再固定 mysql 示例）。
        let mut mysql = driver("mysql", "mysql", "MySQL (sqlx)", true);
        mysql.url_template =
            Some("mysql://{username}:{password}@{host}:{port}/{database}".to_string());
        mysql.default_port = Some(3306);
        assert_eq!(
            address_placeholder(Some(&mysql), "mysql"),
            "mysql://user:password@localhost:3306/db"
        );
        // 无模板/无驱动：退回类型前缀示例与引导文案（不造连接数据）。
        let pg = driver("postgres", "postgresql", "PostgreSQL (sqlx)", true);
        assert_eq!(
            address_placeholder(Some(&pg), "postgresql"),
            "postgresql://主机:端口/数据库"
        );
        assert_eq!(
            address_placeholder(None, "mysql"),
            tr("选择数据库类型与驱动后填写连接地址")
        );

        // 文件型：提示是文件路径（选了 SQLite 不会再出现 mysql:// 示例）。
        let mut sqlite = driver("sqlite", "sqlite", "SQLite (rusqlite)", true);
        sqlite.is_file = true;
        assert!(
            address_placeholder(Some(&sqlite), "sqlite").contains(".sqlite"),
            "文件型提示应指向文件路径"
        );
        assert!(address_placeholder(None, "duckdb").contains(":memory:"));

        // 模板示例：未声明默认端口时不做臆造（写「端口」而非具体值）。
        assert_eq!(
            url_template_example("postgres://{host}:{port}/{database}", None),
            "postgres://localhost:端口/db"
        );
        assert_eq!(
            url_template_example("sqlite://{file_path}", None),
            "sqlite://路径"
        );
    }

    #[test]
    fn file_db_input_drops_credentials_and_tls() {
        // 从 MySQL 切到 SQLite：表单残留的凭据 / 网络 / TLS 不落库，策略覆盖保留。
        let mut i = DataSourceSaveInput::new("a", "sqlite", "C:/data/a.db");
        i.username = Some("root".into());
        i.password = Some("pw".into());
        i.auth_method = Some("password".into());
        i.auth_config_id = Some("auth_1".into());
        i.network_config_id = Some("net_1".into());
        i.advanced_options = Some(
            r#"{"ssl":{"mode":"require"},"network_chain":[{"kind":"ssh"}],"policy_overrides":["security"],"navigator":{"show_system_objects":true}}"#
                .into(),
        );
        strip_file_db_noise(&mut i);
        assert!(i.username.is_none() && i.password.is_none());
        assert!(i.auth_method.is_none() && i.auth_config_id.is_none());
        assert!(i.network_config_id.is_none());
        let adv: serde_json::Value =
            serde_json::from_str(i.advanced_options.as_deref().expect("adv 保留")).expect("json");
        assert!(adv.get("ssl").is_none() && adv.get("network_chain").is_none());
        assert!(adv.get("policy_overrides").is_some(), "策略覆盖仍然有效");
        assert!(
            adv.get("navigator").is_some(),
            "P1 导航设置不随文件型噪音剥离（连接级偏好与 TLS/隧道无关）"
        );
        // 无高级选项/非法 JSON：不 panic，不静默丢数据。
        let mut empty = DataSourceSaveInput::new("b", "duckdb", ":memory:");
        strip_file_db_noise(&mut empty);
        assert!(empty.advanced_options.is_none());
        let mut bad = DataSourceSaveInput::new("c", "duckdb", ":memory:");
        bad.advanced_options = Some("not-json".into());
        strip_file_db_noise(&mut bad);
        assert_eq!(bad.advanced_options.as_deref(), Some("not-json"));
    }

    #[test]
    fn staging_type_badge_prefers_live_form_for_current_entry() {
        // 当前条目：表单已切类型 → 显示表单类型（不等草稿写回）
        assert_eq!(staging_display_type_id("mysql", Some("sqlite")), "sqlite");
        // 表单未选类型 → 回退草稿快照
        assert_eq!(staging_display_type_id("mysql", Some("  ")), "mysql");
        // 非当前条目（无 live）→ 快照
        assert_eq!(staging_display_type_id("mysql", None), "mysql");
    }

    #[test]
    fn type_badge_uses_icon_with_default_fallback() {
        let types = vec![
            ds_type("mysql", "MySQL", Some("🐬")),
            ds_type("oracle", "Oracle", None),
        ];
        assert_eq!(
            type_badge(&types, "mysql"),
            Some(("🐬".to_string(), "MySQL".to_string()))
        );
        assert_eq!(
            type_badge(&types, "oracle"),
            Some(("🗄".to_string(), "Oracle".to_string()))
        );
        assert_eq!(type_badge(&types, "redis"), None);
    }

    /// 标题行交互（窗口级，决策 #108）：点标题行的**任何位置**都会触发 `on_toggle`，
    /// 而且收起 / 展开是几何上的（面板高度按揭示进度变矮、可逆）。
    ///
    /// 为什么单独开一个不带对话框的窗口：headless 下鼠标事件到不了对话框层
    /// （本仓第二次撞到，同款注记见 `analytics_resource/tests/dialog_window.rs`；实测同样结构的行
    /// 在普通窗口里一点就中）。对话框侧只钉几何与状态
    /// （`tests/connection_render_matrix.rs::outline_section_collapses_from_its_whole_title_row`），
    /// 而“点击 → 回调 → 收起”这段接线在这里用真点击钉住。
    mod header_interaction {
        // 不通配导入（`super::*` 会把 gpui 的 `test` 宏带入作用域）。
        use std::cell::Cell;
        use std::rc::Rc;

        use gpui_kit::component::ActiveTheme as _;
        use gpui_kit::test::TestWindowExt as _;
        use gpui_kit::{
            Context, ElementId, IntoElement, ParentElement, Render, SharedString, Styled as _,
            TestAppContext, VisualTestContext, WeakEntity, Window, div, point, rems,
        };

        use super::super::{lucide, outline_section};

        /// 只放一个分组：折叠态由测试观察（回调里写 `Rc<Cell>` 并要求重绘）。
        struct Probe {
            collapsed: Rc<Cell<bool>>,
            handle: WeakEntity<Probe>,
        }

        impl Render for Probe {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let theme = cx.theme().clone();
                let collapsed = self.collapsed.get();
                let flag = self.collapsed.clone();
                let handle = self.handle.clone();
                div().size_full().p_4().child(outline_section(
                    &theme,
                    "probe",
                    lucide("icons/database.svg"),
                    theme.colors.primary,
                    "分组",
                    collapsed,
                    move |_window, app| {
                        flag.set(!flag.get());
                        handle.update(app, |_, cx| cx.notify()).ok();
                    },
                    div().h(rems(8.)),
                ))
            }
        }

        /// 推时钟 + 出帧直到组件内 spring 收敛（headless 下时钟不会自己走）。
        fn settle_reveal(cx: &mut VisualTestContext) {
            for _ in 0..60 {
                cx.update(|window, cx| {
                    cx.background_executor()
                        .advance_clock(std::time::Duration::from_millis(16));
                    window.draw(cx).clear(cx);
                });
            }
        }

        #[gpui_kit::test]
        fn clicking_the_title_row_collapses_and_restores(cx: &mut TestAppContext) {
            cx.update(gpui_kit::init);
            let flag = Rc::new(Cell::new(false));
            let (_probe, cx) = cx.add_window_view({
                let flag = flag.clone();
                move |_window, cx| {
                    let handle = cx.entity().downgrade();
                    Probe {
                        collapsed: flag.clone(),
                        handle,
                    }
                }
            });
            cx.update(|window, cx| window.draw(cx).clear(cx));

            let row_id = ElementId::Name(SharedString::from("conn-sec-probe"));
            let body_id = ElementId::Name(SharedString::from("conn-sec-body-probe"));
            let snap =
                |cx: &mut VisualTestContext, id: ElementId| cx.update(|window, _| window.find(id));
            let row = snap(cx, row_id.clone());
            assert!(row.visible(), "标题行应可见");
            assert!(!flag.get(), "默认应为展开");
            assert!(snap(cx, body_id.clone()).visible(), "展开时正文应可见");

            // 点标题行**远端的空白处**（不是 chevron / 文字）：整行都是按钮才中。
            // 落点用行宽的分数算（本文件受尺寸契约扫描，不能写裸像素字面量）。
            cx.update(|window, cx| {
                window.click_at(
                    row_id.clone(),
                    point(
                        row.bounds().size.width * 3. / 4.,
                        row.bounds().size.height / 2.,
                    ),
                    cx,
                )
            });
            settle_reveal(cx);
            assert!(flag.get(), "点标题行应触发 on_toggle");
            let folded = snap(cx, body_id.clone());
            assert!(
                !folded.visible(),
                "收起后正文应被揭示进度夹成不可见（仍挂载）"
            );
            assert!(
                folded.bounds().size.width > row.bounds().size.width / 2.,
                "收起只是夹高度：正文宽度还在（不是被删掉重建）"
            );

            // 再点一次（行中）：复原。
            cx.update(|window, cx| {
                window.click_at(
                    row_id.clone(),
                    point(row.bounds().size.width / 2., row.bounds().size.height / 2.),
                    cx,
                )
            });
            settle_reveal(cx);
            assert!(!flag.get(), "再点应展开");
            assert!(snap(cx, body_id).visible(), "展开后正文应重新可见");
            // 标题行仍在元素树里（收起过不等于重建过）。
            assert!(snap(cx, row_id).visible(), "标题行应仍在元素树里");
        }
    }