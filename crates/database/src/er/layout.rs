//! ER 分层布局：按外键引用分层（被引用者在前）+ 层内重心排序。
//!
//! 纯函数、确定性：同输入必出同结果，便于单测与快照；无 GPUI 依赖。
//! 几何参数由 [`LayoutMetrics`] 显式传入——视图层用 `workbench_shell::ui::*` 派生，
//! 本层 [`LayoutMetrics::default`] 只服务测试与无主题环境（**不要**在视图里用默认值）。

use crate::er::model::{EntityKey, ErDiagram};

/// 布局度量（单位：逻辑像素）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutMetrics {
    /// 卡片宽。
    pub card_width: f32,
    /// 卡片表头（表名行）高。
    pub header_height: f32,
    /// 每个列行高。
    pub row_height: f32,
    /// 层间水平间距。
    pub h_gap: f32,
    /// 同层卡片垂直间距。
    pub v_gap: f32,
    /// 画布内边距。
    pub padding: f32,
}

impl LayoutMetrics {
    /// 卡片总高（表头 + 列行数 × 行高）。
    pub fn card_height(&self, rows: usize) -> f32 {
        self.header_height + rows as f32 * self.row_height
    }
}

impl Default for LayoutMetrics {
    fn default() -> Self {
        // 中性默认：仅测试 / 无主题环境使用；视图必须传 ui.rs 派生值。
        Self {
            card_width: 220.0,
            header_height: 28.0,
            row_height: 22.0,
            h_gap: 80.0,
            v_gap: 28.0,
            padding: 24.0,
        }
    }
}

/// 矩形（左上角 + 宽高）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ErRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// 单个实体的布局结果。
#[derive(Debug, Clone, PartialEq)]
pub struct EntityBox {
    pub key: EntityKey,
    /// 所在层（0 = 最左；层号随「被引用深度」递增）。
    pub layer: u32,
    pub rect: ErRect,
}

/// 布局结果：实体框集合 + 内容包围盒。
///
/// `boxes` 与 `diagram.entities` **同序同长**，视图可直接 `zip` 迭代。
#[derive(Debug, Clone, PartialEq)]
pub struct LaidOutDiagram {
    pub boxes: Vec<EntityBox>,
    /// 内容包围盒（原点在 (0, 0)，尺寸含内边距）。
    pub bounds: ErRect,
}

impl LaidOutDiagram {
    /// 按实体键取矩形（画连线时按端点定位）。
    pub fn position_of(&self, key: &EntityKey) -> Option<ErRect> {
        self.boxes.iter().find(|b| &b.key == key).map(|b| b.rect)
    }
}

/// 分层布局入口。
///
/// 层号由「子表引用父表」的依赖图按**最长路径**算：父表（被引用者）层号更小、排在左，
/// 子表在右；同层内按父节点位置的重心排序以减少连线交叉；每层在垂直方向居中。
pub fn layered_layout(diagram: &ErDiagram, m: &LayoutMetrics) -> LaidOutDiagram {
    let n = diagram.entities.len();
    if n == 0 {
        return LaidOutDiagram {
            boxes: Vec::new(),
            bounds: ErRect { x: 0.0, y: 0.0, width: m.padding * 2.0, height: m.padding * 2.0 },
        };
    }

    let index_of = |key: &EntityKey| diagram.entities.iter().position(|e| &e.key == key);

    // 依赖边：child（引用方）→ parent（被引用方）
    let mut deps: Vec<Vec<usize>> = vec![Vec::new(); n];
    for r in &diagram.relations {
        let (Some(from), Some(to)) = (index_of(&r.from), index_of(&r.to)) else {
            continue;
        };
        // 自引用不入依赖（否则该节点自我抬高一层，且对布局无意义）
        if from != to && !deps[from].contains(&to) {
            deps[from].push(to);
        }
    }

    let layers = assign_layers(n, &deps);
    let max_layer = layers.iter().copied().max().unwrap_or(0);
    let layer_count = max_layer as usize + 1;

    // 分层聚合
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); layer_count];
    for (i, &l) in layers.iter().enumerate() {
        buckets[l as usize].push(i);
    }

    // 层内重心排序：已定序父节点位置的平均（无已定序父者排后，按原始下标兜底）
    let mut order: Vec<f32> = vec![0.0; n];
    for bucket in buckets.iter_mut() {
        let mut keyed: Vec<(Option<f32>, usize)> = bucket
            .iter()
            .map(|&i| {
                let mut sum = 0.0f32;
                let mut cnt = 0usize;
                for &d in &deps[i] {
                    if layers[d] < layers[i] {
                        sum += order[d];
                        cnt += 1;
                    }
                }
                ((cnt > 0).then(|| sum / cnt as f32), i)
            })
            .collect();
        keyed.sort_by(|(bx, a), (by, b)| match (bx, by) {
            (Some(x), Some(y)) => x
                .partial_cmp(y)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.cmp(b)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.cmp(b),
        });
        *bucket = keyed.into_iter().map(|(_, i)| i).collect();
        for (pos, &i) in bucket.iter().enumerate() {
            order[i] = pos as f32;
        }
    }

    // 卡片高度与每层总高
    let heights: Vec<f32> = diagram
        .entities
        .iter()
        .map(|e| m.card_height(e.columns.len()))
        .collect();
    let mut layer_heights = vec![0.0f32; layer_count];
    for (l, bucket) in buckets.iter().enumerate() {
        let sum: f32 = bucket.iter().map(|&i| heights[i]).sum();
        let gaps = bucket.len().saturating_sub(1) as f32 * m.v_gap;
        layer_heights[l] = sum + gaps;
    }
    let content_h = layer_heights.iter().copied().fold(0.0f32, f32::max);

    // 落位：每层垂直居中，层内自顶向下堆叠
    let mut slots: Vec<Option<EntityBox>> = vec![None; n];
    for (l, bucket) in buckets.iter().enumerate() {
        let x = m.padding + l as f32 * (m.card_width + m.h_gap);
        let mut y = m.padding + (content_h - layer_heights[l]) / 2.0;
        for &i in bucket {
            slots[i] = Some(EntityBox {
                key: diagram.entities[i].key.clone(),
                layer: l as u32,
                rect: ErRect { x, y, width: m.card_width, height: heights[i] },
            });
            y += heights[i] + m.v_gap;
        }
    }
    let boxes: Vec<EntityBox> = slots.into_iter().flatten().collect();

    let width = m.padding * 2.0
        + layer_count as f32 * m.card_width
        + layer_count.saturating_sub(1) as f32 * m.h_gap;
    let height = m.padding * 2.0 + content_h;

    LaidOutDiagram {
        boxes,
        bounds: ErRect { x: 0.0, y: 0.0, width, height },
    }
}

/// 最长路径分层（容忍环）：`layer(v) = 1 + max(layer(parent))`，无父者为 0。
///
/// 有环时值会持续增长，故以 `n` 封顶，并把最小值归一化到 0；迭代至多 `n` 轮必终止。
/// 复杂度 O(n·E)，ER 图规模有限，够用。
fn assign_layers(n: usize, deps: &[Vec<usize>]) -> Vec<u32> {
    let mut layer = vec![0u32; n];
    let cap = n as u32;
    for _ in 0..n {
        let mut changed = false;
        for v in 0..n {
            if deps[v].is_empty() {
                continue;
            }
            let want = deps[v].iter().map(|&d| layer[d]).max().unwrap_or(0) + 1;
            if want > layer[v] && want <= cap {
                layer[v] = want;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let min = layer.iter().copied().min().unwrap_or(0);
    if min > 0 {
        layer.iter_mut().for_each(|l| *l -= min);
    }
    layer
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use engine::driver::traits::{ColumnDetail, ConstraintDetail, SchemaObjectKind};

    use super::*;
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
            is_identity: false,
            identity_generation: None,
            collation: None,
            extra: HashMap::new(),
        }
    }

    fn table(name: &str, refs: &[(&str, &str)]) -> TableInput {
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
            columns: vec![col("id")],
            constraints,
        }
    }

    fn diagram_of(tables: &[TableInput]) -> ErDiagram {
        build_diagram(tables)
    }

    fn layer_of(l: &LaidOutDiagram, name: &str) -> u32 {
        l.boxes.iter().find(|b| b.key.display_name() == name).expect("实体在手").layer
    }

    #[test]
    fn referenced_entity_is_left_of_referrer() {
        let d = diagram_of(&[table("orders", &[("user_id", "users")]), table("users", &[])]);
        let l = layered_layout(&d, &LayoutMetrics::default());
        let users = l.position_of(&EntityKey::new("s", "users")).expect("users");
        let orders = l.position_of(&EntityKey::new("s", "orders")).expect("orders");
        assert!(users.x < orders.x, "被引用者应排在左");
        assert_eq!(layer_of(&l, "users"), 0);
        assert_eq!(layer_of(&l, "orders"), 1);
    }

    #[test]
    fn entities_without_references_share_layer_zero() {
        let d = diagram_of(&[table("a", &[]), table("b", &[])]);
        let l = layered_layout(&d, &LayoutMetrics::default());
        assert_eq!(layer_of(&l, "a"), 0);
        assert_eq!(layer_of(&l, "b"), 0);
    }

    #[test]
    fn longest_path_layering_puts_chain_in_order() {
        // c → b → a：a 最左、c 最右
        let d = diagram_of(&[table("c", &[("b_id", "b")]), table("b", &[("a_id", "a")]), table("a", &[])]);
        let l = layered_layout(&d, &LayoutMetrics::default());
        assert_eq!(layer_of(&l, "a"), 0);
        assert_eq!(layer_of(&l, "b"), 1);
        assert_eq!(layer_of(&l, "c"), 2);
    }

    #[test]
    fn cyclic_references_terminate_and_stay_bounded() {
        let d = diagram_of(&[table("a", &[("x", "b")]), table("b", &[("y", "a")])]);
        let l = layered_layout(&d, &LayoutMetrics::default());
        assert_eq!(l.boxes.len(), 2);
        let max = l.boxes.iter().map(|b| b.layer).max().unwrap();
        assert!(max <= 1, "环内层号应归一化到 0..=1，实得 {max}");
    }

    #[test]
    fn boxes_are_entity_ordered_and_within_bounds() {
        let d = diagram_of(&[table("orders", &[("user_id", "users")]), table("users", &[])]);
        let l = layered_layout(&d, &LayoutMetrics::default());
        assert_eq!(l.boxes.len(), d.entities.len());
        for (b, e) in l.boxes.iter().zip(&d.entities) {
            assert_eq!(&b.key, &e.key, "boxes 与 entities 同序");
            assert!(b.rect.x >= 0.0 && b.rect.y >= 0.0);
            assert!(b.rect.x + b.rect.width <= l.bounds.width + 0.01);
            assert!(b.rect.y + b.rect.height <= l.bounds.height + 0.01);
        }
    }

    #[test]
    fn layer_x_is_monotonic_and_non_overlapping() {
        let d = diagram_of(&[table("c", &[("b_id", "b")]), table("b", &[("a_id", "a")]), table("a", &[])]);
        let m = LayoutMetrics::default();
        let l = layered_layout(&d, &m);
        let mut xs: Vec<f32> = l.boxes.iter().map(|b| b.rect.x).collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        xs.dedup();
        for pair in xs.windows(2) {
            assert!(pair[1] - pair[0] >= m.card_width, "层间不应重叠");
        }
    }

    #[test]
    fn layout_is_deterministic() {
        let d = diagram_of(&[table("orders", &[("user_id", "users")]), table("users", &[])]);
        let m = LayoutMetrics::default();
        assert_eq!(layered_layout(&d, &m), layered_layout(&d, &m));
    }

    #[test]
    fn empty_diagram_has_padding_sized_bounds() {
        let d = ErDiagram::default();
        let m = LayoutMetrics::default();
        let l = layered_layout(&d, &m);
        assert!(l.boxes.is_empty());
        assert_eq!(l.bounds.width, m.padding * 2.0);
        assert_eq!(l.bounds.height, m.padding * 2.0);
    }

    #[test]
    fn card_height_grows_with_rows() {
        let m = LayoutMetrics::default();
        assert_eq!(m.card_height(0), m.header_height);
        assert_eq!(m.card_height(3), m.header_height + 3.0 * m.row_height);
    }
}
