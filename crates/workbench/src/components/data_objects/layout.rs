//! 行宽预算：面板窄到多少时该丢哪一列——**按实测数字定，不靠估**。
//!
//! 数的来源：`panels/data_objects_tab/layout_probe.rs`（headless 真窗口 + gpui 真排版）。
//! 在那儿把面板放进 260–680px 的容器里各排一遍，量到：
//! - 名字那一格的宽 = 面板宽 − 290（斜率恰为 1，说明其余部分是**定宽**）；
//! - 执行通道列（`→ 进程 DuckDB`）恒 80px；小标列（`载入` / `源连接`）37–44px。
//!
//! 所以「名字还有多少地方」这件事根本不用逐行量：`面板宽 − 固定开销`。
//! 面板一窄，先丢**执行通道**（它的信息在属性窗口里有整份），名字就能多吃回 80px；
//! 再窄才丢小标（小标是业务身份的唯一判据，能留就留，见铁律 G 第 30 条）。

/// 名字之外的固定开销（内距 + 展开位 + 图标 + 各 gap + 小标 + 通道 + 状态 + 行尾按钮）。
///
/// 实测值，见模块注释；`layout_probe` 的用例会断言「名字宽 + 这个数 = 面板宽」。
pub const ROW_CHROME_PX: f32 = 290.0;

/// 丢执行通道列能腾出的宽：列本身（`→ 进程 DuckDB` 实测 80）+ 它的一个 `gap_3`（12）。
///
/// **丢列省下的是「列 + 它那份间距」**——只减列宽会算乐观 12px（第一版就踩了这个坑，
/// 被 `layout_probe` 的「名字实测 vs 预算」断言抓出来）。
pub const CHANNEL_COL_PX: f32 = 92.0;

/// 丢小标列能腾出的宽：`源连接` 是最宽档（44）+ 一个 `gap_3`（12）。
///
/// 取最宽档而不是 `载入`（37）：预算因此是**上限**——真实行可能比预算多出十来像素，
/// 但绝不会比预算更紧。判档要的是「至少能留多少」，乐观算会把档位判宽。
pub const PILL_COL_PX: f32 = 56.0;

/// 名字至少留多少：约 14 个字（14px 字号实测 8.75px/字）。低于这个数就没有辨识价值，
/// 与其挤成「客户订单…」，不如丢掉右边的列把名字让出来。
pub const NAME_MIN_PX: f32 = 120.0;

/// 这一档面板宽该显示哪些列。
///
/// 顺序就是丢的优先级：**先通道、再小标**（铁律第 30 条：小标是不靠名字猜身份的唯一依据）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowTier {
    /// 全列：名字有 `NAME_MIN_PX` 以上可用。
    Full,
    /// 丢执行通道：名字吃回 92px。
    DropChannel,
    /// 通道与小标都丢：只剩名字 + 状态 + 按钮，名字走马灯（下一刀）。
    DropChannelAndPill,
}

impl RowTier {
    /// 判档：`panel_width` = 行实际可用宽（不是窗口宽；测量见 `layout_probe`）。
    ///
    /// `0` 表示「还没量到」（首帧）：按 [`Self::Full`] 画——宁可第一帧挤一点，
    /// 也不要先画成极简再跳回全列。
    pub fn for_width(panel_width: f32) -> Self {
        if panel_width <= 0.0 {
            return Self::Full;
        }
        if panel_width >= ROW_CHROME_PX + NAME_MIN_PX {
            Self::Full
        } else if panel_width >= ROW_CHROME_PX - CHANNEL_COL_PX + NAME_MIN_PX {
            Self::DropChannel
        } else {
            Self::DropChannelAndPill
        }
    }

    /// 这一档里名字**最多**能占多宽（按最宽列估，真实行可能略窄，见 `PILL_COL_PX`）。
    pub fn name_space(self, panel_width: f32) -> f32 {
        let chrome = match self {
            Self::Full => ROW_CHROME_PX,
            Self::DropChannel => ROW_CHROME_PX - CHANNEL_COL_PX,
            Self::DropChannelAndPill => ROW_CHROME_PX - CHANNEL_COL_PX - PILL_COL_PX,
        };
        (panel_width - chrome).max(0.0)
    }

    /// 要不要画执行通道列。
    pub fn shows_channel(self) -> bool {
        self == Self::Full
    }

    /// 要不要画小标列。
    pub fn shows_pill(self) -> bool {
        !matches!(self, Self::DropChannelAndPill)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 档位边界与实测对齐：按 `layout_probe` 的量法，410 宽刚够全列（名字 120）。
    #[test]
    fn tiers_flip_where_the_measurement_says() {
        assert_eq!(RowTier::for_width(0.0), RowTier::Full, "没量到就按全列画");
        assert_eq!(RowTier::for_width(680.0), RowTier::Full);
        assert_eq!(RowTier::for_width(410.0), RowTier::Full);
        assert_eq!(RowTier::for_width(409.0), RowTier::DropChannel);
        assert_eq!(
            RowTier::for_width(318.0),
            RowTier::DropChannel,
            "318 宽刚好够 120px 名字"
        );
        assert_eq!(RowTier::for_width(317.0), RowTier::DropChannelAndPill);
        assert_eq!(RowTier::for_width(200.0), RowTier::DropChannelAndPill);
    }

    /// 名字可用宽 = 面板宽 − 该档的固定开销：这条等式就是「能不能放得下」的判据。
    #[test]
    fn name_space_is_panel_minus_chrome() {
        assert_eq!(RowTier::Full.name_space(380.0), 380.0 - ROW_CHROME_PX);
        assert_eq!(
            RowTier::DropChannel.name_space(380.0),
            380.0 - ROW_CHROME_PX + CHANNEL_COL_PX,
            "丢通道后名字必须吃回「列 + 它的间距」"
        );
        // 面板窄到比开销还小时不能反号（负宽会让 ellipsis 画出怪东西）
        assert_eq!(RowTier::DropChannelAndPill.name_space(100.0), 0.0);
    }

    /// 丢列顺序不能反：小标是身份判据，绝不能在还有通道列时就先丢它。
    #[test]
    fn pill_is_the_last_column_to_go() {
        let tier = RowTier::for_width(340.0);
        assert!(!tier.shows_channel(), "340 宽先丢通道");
        assert!(tier.shows_pill(), "小标必须留到最后一档");

        let tight = RowTier::for_width(300.0);
        assert!(!tight.shows_channel() && !tight.shows_pill());
    }
}
