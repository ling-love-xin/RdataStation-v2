//! 连接 / 库 / Schema 选择器纯逻辑测试（headless 口径：不点对话框，驱动实体方法/纯函数）。

use std::rc::Rc;

use gpui_kit::{App, Window};

use super::{ConnectionPickerDelegate, SchemaPickerState};
use crate::connection::{ConnectionOption, SchemaInfo};

fn conn(id: &str, name: &str, connected: bool) -> ConnectionOption {
    ConnectionOption {
        id: id.into(),
        short: id.chars().next().unwrap_or('?').to_string(),
        name: name.into(),
        connected,
        db_type: "test".into(),
    }
}

#[test]
fn schema_picker_groups_by_catalog_and_preserves_order() {
    let schemas = vec![
        SchemaInfo {
            catalog: Some("postgres".into()),
            schema: "public".into(),
        },
        SchemaInfo {
            catalog: Some("warehouse_supply_chain".into()),
            schema: "public".into(),
        },
        SchemaInfo {
            catalog: Some("warehouse_supply_chain".into()),
            schema: "rds_nav_probe_a".into(),
        },
        SchemaInfo {
            catalog: None,
            schema: "main".into(), // MySQL 无 Catalog 层：库即 Schema
        },
    ];
    let state = SchemaPickerState::new(schemas, Some("rds_nav_probe_a".into()), |_, _, _| {});
    // 库列表去重保序：postgres → warehouse_supply_chain → None(默认库)
    assert_eq!(
        state.catalogs,
        vec![
            Some("postgres".to_string()),
            Some("warehouse_supply_chain".to_string()),
            None
        ]
    );
    assert_eq!(state.schemas_by[0], vec!["public"]);
    assert_eq!(state.schemas_by[1], vec!["public", "rds_nav_probe_a"]);
    assert_eq!(state.schemas_by[2], vec!["main"]);
    // 当前 schema 在非首库 → 左栏定位到对应库（选中 warehouse_supply_chain 那栏）
    assert_eq!(state.selected, 1);
}

#[test]
fn schema_picker_defaults_to_first_catalog_when_current_is_missing() {
    let schemas = vec![SchemaInfo {
        catalog: Some("postgres".into()),
        schema: "public".into(),
    }];
    let state = SchemaPickerState::new(schemas, Some("nope".into()), |_, _, _| {});
    assert_eq!(state.selected, 0);
    assert_eq!(state.current.as_deref(), Some("nope"));
}

#[test]
fn connection_picker_filters_to_connected_when_toggled() {
    let delegate = ConnectionPickerDelegate {
        options: vec![
            conn("a", "Alpha", true),
            conn("b", "Beta", false),
            conn("c", "Gamma", true),
        ],
        only_connected: true,
        current: None,
        selected: None,
        on_pick: Rc::new(|_: Option<String>, _: &mut Window, _: &mut App| {}),
    };
    let ids: Vec<&str> = delegate.filtered().iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, vec!["a", "c"]);
}

#[test]
fn connection_picker_shows_all_when_only_connected_is_off() {
    let delegate = ConnectionPickerDelegate {
        options: vec![conn("a", "Alpha", true), conn("b", "Beta", false)],
        only_connected: false,
        current: None,
        selected: None,
        on_pick: Rc::new(|_: Option<String>, _: &mut Window, _: &mut App| {}),
    };
    assert_eq!(delegate.filtered().len(), 2);
}
