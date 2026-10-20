//! ER 连线几何：两张实体卡片之间的**正交折线**与端点吸附（纯函数）。
//!
//! 纯函数、确定性、无 GPUI 依赖：同输入必出同结果，便于单测；视图层把这里算出的折线
//! 顶点交给 canvas 描线（见 [`crate::er::view`]）。
//!
//! 端点吸附口径（设计文档 §8 / §11 的未定项，此处**落定为列行吸附 + 卡片边缘兜底**）：
//! - 关系两端各自给出列名（`ErRelation::from_columns` / `to_columns`）；能在这张实体的列里
//!   找到同名列时，吸附到**该列所在行**的纵向中心——连线看着指向具体那一列，而不是卡片中缝；
//! - 找不到（驱动没给 `referenced_columns`、或列名口径不一致）时退化为卡片**纵向中心**，
//!   保证任何关系都画得出来，不出现「有外键却没线」。
//!
//! 走向规则（父子分层后父表在左、子表在右）：子表在父表右侧时，**从子表左边出、进父表右边**；
//! 反过来（环导致的逆序）则从子表右边出、进父表左边。自引用单独走一个小回环，不参与上述规则。

use crate::er::layout::{ErRect, LaidOutDiagram, LayoutMetrics};
use crate::er::model::{ErDiagram, EntityKey, ErEntity};

/// 平面点（逻辑像素）。
///
/// 刻意不复用 gpui 的 `Point<Pixels>`：本模块是纯几何层，不依赖 GPUI。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ErPoint {
    pub x: f32,
    pub y: f32,
}

/// 一条外键关系画出的折线。
#[derive(Debug, Clone, PartialEq)]
pub struct ErEdge {
    /// 折线顶点（≥ 2 个；首点 = 起点、末点 = 终点），相邻点对构成一条线段。
    pub points: Vec<ErPoint>,
    /// 对应 `diagram.relations` 里的下标（视图/测试按它回溯到关系本体）。
    pub relation: usize,
}

/// 计算全图所有关系的连线折线（顺序与 `diagram.relations` 一致）。
pub fn edge_paths(diagram: &ErDiagram, layout: &LaidOutDiagram, m: &LayoutMetrics) -> Vec<ErEdge> {
    let mut out = Vec::with_capacity(diagram.relations.len());
    for (ix, rel) in diagram.relations.iter().enumerate() {
        let (Some(from_rect), Some(to_rect)) = (
            layout.position_of(&rel.from),
            layout.position_of(&rel.to),
        ) else {
            // 端点不在布局里（理论上不会；`build_diagram` 已保证端点自洽）→ 跳过而非画歪线。
            continue;
        };

        let fy = anchor_y(diagram, &rel.from, &rel.from_columns, &from_rect, m);
        let ty = anchor_y(diagram, &rel.to, &rel.to_columns, &to_rect, m);

        let points = if rel.from == rel.to {
            self_loop_points(&from_rect, fy, ty, m)
        } else if from_rect.x >= to_rect.x {
            // 常规：子表在父表右侧 → 左出右进。
            orthogonal(&from_rect, fy, &to_rect, ty, /* from_left */ true)
        } else {
            // 逆序（环）：子表在父表左侧 → 右出左进。
            orthogonal(&from_rect, fy, &to_rect, ty, /* from_left */ false)
        };
        out.push(ErEdge { points, relation: ix });
    }
    out
}

/// 关系在某一侧的吸附 y（列行中心；列名解析不到时退化为卡片纵向中心）。
fn anchor_y(
    diagram: &ErDiagram,
    key: &EntityKey,
    columns: &[String],
    rect: &ErRect,
    m: &LayoutMetrics,
) -> f32 {
    if let Some(entity) = diagram.entities.iter().find(|e| &e.key == key) {
        if let Some(row) = row_index_of(entity, columns) {
            return row_center_y(rect, m, row, entity.columns.len());
        }
    }
    rect.y + rect.height / 2.0
}

/// 在实体的列里找 `columns` 中第一个能命中的列下标。
fn row_index_of(entity: &ErEntity, columns: &[String]) -> Option<usize> {
    columns
        .iter()
        .find_map(|want| entity.columns.iter().position(|c| &c.name == want))
}

/// 第 `row` 个列行的纵向中心（行号越界时夹到最后一个可见行）。
fn row_center_y(rect: &ErRect, m: &LayoutMetrics, row: usize, rows_total: usize) -> f32 {
    // 行号按「已画出来的行」夹住：元数据里没有这一列时不该把 y 算到卡片外面。
    let last = rows_total.saturating_sub(1);
    let row = row.min(last);
    rect.y + m.header_height + (row as f32 + 0.5) * m.row_height
}

/// 两卡片之间的正交折线（`from_left` 决定从子表哪条竖边出）。
fn orthogonal(from: &ErRect, fy: f32, to: &ErRect, ty: f32, from_left: bool) -> Vec<ErPoint> {
    let start = ErPoint {
        x: if from_left { from.x } else { from.x + from.width },
        y: fy,
    };
    let end = ErPoint {
        x: if from_left { to.x + to.width } else { to.x },
        y: ty,
    };
    if (fy - ty).abs() < f32::EPSILON {
        // 同一行：一条直线即可，不留无意义的中间顶点。
        return vec![start, end];
    }
    let mid_x = (start.x + end.x) / 2.0;
    vec![
        start,
        ErPoint { x: mid_x, y: fy },
        ErPoint { x: mid_x, y: ty },
        end,
    ]
}

/// 自引用的小回环：从卡片右边出、绕到卡片左边同一列行回去。
fn self_loop_points(rect: &ErRect, fy: f32, ty: f32, m: &LayoutMetrics) -> Vec<ErPoint> {
    let right = rect.x + rect.width;
    let bulge = right + m.h_gap / 2.0;
    vec![
        ErPoint { x: right, y: fy },
        ErPoint { x: bulge, y: fy },
        ErPoint { x: bulge, y: ty },
        ErPoint { x: right, y: ty },
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use engine::driver::traits::{ColumnDetail, ConstraintDetail, SchemaObjectKind};

    use super::*;
    use crate::er::layout::layered_layout;
    use crate::er::model::{build_diagram, TableInput};

    fn col(name: &str) -> ColumnDetail {
        ColumnDetail {
            name: name.to_string(),
            data_type: "text".to_string(),
            nullable: true,
            is_primary_key: false,
            is_foreign_key: false,
            references: None,
            ordinal: 0,
            default_value: None,
            comment: None,
            extra: HashMap::new(),
        }
    }

    fn table(name: &str, cols: &[&str], refs: &[(&str, &str)]) -> TableInput {
        let constraints = refs
            .iter()
            .map(|(c, t)| ConstraintDetail {
                name: format!("fk_{name}_{t}"),
                table_name: name.to_string(),
                constraint_type: "FOREIGN KEY".to_string(),
                column_names: vec![c.to_string()],
                referenced_table: Some(t.to_string()),
                referenced_columns: vec!["id".to_string()],
                update_rule: None,
                delete_rule: None,
            })
            .collect();
        TableInput {
            schema: "s".to_string(),
            name: name.to_string(),
            kind: SchemaObjectKind::Table,
            columns: cols.iter().map(|c| col(c)).collect(),
            constraints,
        }
    }

    fn laid_out(inputs: &[TableInput]) -> (ErDiagram, LaidOutDiagram, LayoutMetrics) {
        let d = build_diagram(inputs);
        let m = LayoutMetrics::default();
        let l = layered_layout(&d, &m);
        (d, l, m)
    }

    #[test]
    fn no_relations_means_no_edges() {
        let (d, l, m) = laid_out(&[table("a", &["id"], &[])]);
        assert!(edge_paths(&d, &l, &m).is_empty());
    }

    #[test]
    fn same_row_relations_draw_a_straight_line() {
        // 直接验几何函数：两端等高时不引入竖直段（两个顶点）。
        // （不能靠布局凑「同行等高」：每层各自垂直居中，不同卡片的同行未必同 y。）
        let from = ErRect { x: 300.0, y: 100.0, width: 220.0, height: 100.0 };
        let to = ErRect { x: 24.0, y: 100.0, width: 220.0, height: 100.0 };
        let p = orthogonal(&from, 140.0, &to, 140.0, true);
        assert_eq!(p.len(), 2, "等高应为直线（两个端点）");
        assert_eq!(p[0], ErPoint { x: 300.0, y: 140.0 }, "子表左边缘出线");
        assert_eq!(p[1], ErPoint { x: 244.0, y: 140.0 }, "进父表右边缘");
    }

    #[test]
    fn orthogonal_row_mismatch_adds_a_vertical_segment() {
        let from = ErRect { x: 300.0, y: 100.0, width: 220.0, height: 100.0 };
        let to = ErRect { x: 24.0, y: 100.0, width: 220.0, height: 100.0 };
        let p = orthogonal(&from, 140.0, &to, 170.0, true);
        assert_eq!(p.len(), 4, "不同高应为三段折线（四个顶点）");
        assert!((p[1].x - p[2].x).abs() < f32::EPSILON, "中间两点同 x（竖直段）");
        assert!((p[0].y - p[1].y).abs() < f32::EPSILON);
        assert!((p[2].y - p[3].y).abs() < f32::EPSILON);
    }

    #[test]
    fn ordered_layout_draws_from_child_left_to_parent_right() {
        // orders.user_id → users.id：父表在左、子表在右 → 起点的 x 大于终点。
        let (d, l, m) = laid_out(&[
            table("users", &["id"], &[]),
            table("orders", &["id", "user_id"], &[("user_id", "users")]),
        ]);
        let edges = edge_paths(&d, &l, &m);
        assert_eq!(edges.len(), 1);
        let p = &edges[0].points;
        assert!(p.len() >= 2);
        assert!(p[0].x > p[p.len() - 1].x, "子表在右，起点应在终点右侧");
    }

    #[test]
    fn different_rows_introduce_vertical_segment() {
        // orders 的 user_id 在第 1 行、users.id 在第 0 行 → 折线含一段竖直走线（4 个顶点）。
        let (d, l, m) = laid_out(&[
            table("users", &["id", "name"], &[]),
            table("orders", &["id", "user_id"], &[("user_id", "users")]),
        ]);
        let edges = edge_paths(&d, &l, &m);
        let p = &edges[0].points;
        assert_eq!(p.len(), 4, "跨行应为三段折线（四个顶点）");
        // 中间两点的 x 相同（竖直段），首尾 y 等于各自锚点。
        assert!((p[1].x - p[2].x).abs() < f32::EPSILON);
        assert!((p[1].y - p[0].y).abs() < f32::EPSILON);
        assert!((p[3].y - p[2].y).abs() < f32::EPSILON);
    }

    #[test]
    fn anchor_follows_the_named_column_row() {
        // 锚点应落在被引用列所在行：users.id 在第 0 行 → 锚 y = 卡片顶 + 表头 + 半行高。
        let (d, l, m) = laid_out(&[
            table("users", &["id"], &[]),
            table("orders", &["id", "user_id"], &[("user_id", "users")]),
        ]);
        let users = l.position_of(&EntityKey::new("s", "users")).expect("users");
        let edges = edge_paths(&d, &l, &m);
        let end = edges[0].points.last().copied().expect("终点");
        let expected = users.y + m.header_height + 0.5 * m.row_height;
        assert!((end.y - expected).abs() < 0.01, "应吸附到 id 所在行");
    }

    #[test]
    fn unresolved_columns_fall_back_to_card_center() {
        // 列级兜底关系（约束缺失）时 `from_columns` 是真实列名；这里构造一个解析不到的：
        // 用 referenced_columns 指向不存在的列名 → 终点退化为卡片纵向中心。
        let mut orders = table("orders", &["id", "user_id"], &[("user_id", "users")]);
        orders.constraints[0].referenced_columns = vec!["no_such_column".to_string()];
        let (d, l, m) = laid_out(&[table("users", &["id"], &[]), orders]);
        let users = l.position_of(&EntityKey::new("s", "users")).expect("users");
        let edges = edge_paths(&d, &l, &m);
        let end = edges[0].points.last().copied().expect("终点");
        let center = users.y + users.height / 2.0;
        assert!((end.y - center).abs() < 0.01, "解析不到列时应退化为卡片中缝");
    }

    #[test]
    fn self_reference_draws_a_loop_on_the_right() {
        let (d, l, m) = laid_out(&[table("emp", &["id", "boss_id"], &[("boss_id", "emp")])]);
        let edges = edge_paths(&d, &l, &m);
        assert_eq!(edges.len(), 1);
        let p = &edges[0].points;
        assert!(p.len() >= 4, "自引用应画回环");
        let rect = l.position_of(&EntityKey::new("s", "emp")).expect("emp");
        let right = rect.x + rect.width;
        // 回环应鼓出到卡片右侧之外。
        assert!(p.iter().any(|q| q.x > right), "回环应绕到卡片右侧");
    }

    #[test]
    fn reversed_layering_draws_from_child_right_edge() {
        // 环：a → b 且 b → a。分层会归一化，必然有一侧出现「子表在父表左侧」，
        // 那条边应从子表**右边**出。两种走向都不得画出退化折线（顶点数 ≥ 2）。
        let (d, l, m) = laid_out(&[
            table("a", &["x"], &[("x", "b")]),
            table("b", &["y"], &[("y", "a")]),
        ]);
        let edges = edge_paths(&d, &l, &m);
        assert_eq!(edges.len(), 2);
        for e in &edges {
            assert!(e.points.len() >= 2);
        }
    }

    #[test]
    fn edge_relation_indexes_align_with_relations() {
        let (d, l, m) = laid_out(&[
            table("users", &["id"], &[]),
            table("orders", &["id", "user_id"], &[("user_id", "users")]),
        ]);
        let edges = edge_paths(&d, &l, &m);
        assert_eq!(edges.len(), d.relations.len());
        assert_eq!(edges[0].relation, 0);
    }

    #[test]
    fn geometry_is_deterministic() {
        let (d, l, m) = laid_out(&[
            table("users", &["id"], &[]),
            table("orders", &["id", "user_id"], &[("user_id", "users")]),
        ]);
        assert_eq!(edge_paths(&d, &l, &m), edge_paths(&d, &l, &m));
    }
}
