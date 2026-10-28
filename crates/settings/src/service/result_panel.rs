//! 结果集面板设置项的写入处理（`apply_slot`，由 `SettingsService::apply_by_key` 分发）与读写访问器。
use gpui_kit::App;

use crate::model::{FactStoragePref, Settings};
use crate::persistence::persist;
use crate::registry::{SettingValue, Slot};


/// 本域 slot 的标量写入处理；返回 `false` 表示拒绝（值形态不符 / 复合值 / 无法解析）。
pub(crate) fn apply_slot(
    slot: Slot,
    value: SettingValue,
    _window: Option<&mut gpui_kit::Window>,
    cx: &mut App,
) -> bool {
    match slot {
            Slot::ResultPanelFactStorage => {
                let Some(text) = value.as_text() else {
                    return false;
                };
                // 落盘值只认登记表枚举三档；未知值拒绝（设置文件被手改坏时不让写入）。
                let Some(pref) = FactStoragePref::parse(text) else {
                    return false;
                };
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.result_panel.fact_storage = pref;
                }
                persist(&cx.global::<Settings>().clone());
                // NextUse：下次触发物化路径读取；渲染与文本导出不读本偏好，无需通知重绘。
            }
            Slot::ResultPanelShowColumnComments => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.result_panel.show_column_comments = on;
                }
                persist(&cx.global::<Settings>().clone());
                // Immediate：面板在下次 sync 时重读偏好并重绘列头（渲染路径读本偏好）。
            }
            Slot::ResultPanelShowTypeBadge => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.result_panel.show_type_badge = on;
                }
                persist(&cx.global::<Settings>().clone());
            }
            Slot::ResultPanelLoopSelection => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.result_panel.loop_selection = on;
                }
                persist(&cx.global::<Settings>().clone());
            }
            Slot::ResultPanelSelectOnModifierClick => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.result_panel.select_on_modifier_click = on;
                }
                persist(&cx.global::<Settings>().clone());
            }
            Slot::ResultPanelPersistColumnWidths => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.result_panel.persist_column_widths = on;
                }
                persist(&cx.global::<Settings>().clone());
            }
            Slot::ResultPanelNumberFormat => {
                let Some(text) = value.as_text() else {
                    return false;
                };
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.result_panel.number_format =
                        workbench_shell::result_style::NumberFormatPref::parse(text);
                }
                persist(&cx.global::<Settings>().clone());
            }
            Slot::ResultPanelNullPlaceholder => {
                let Some(text) = value.as_text() else {
                    return false;
                };
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.result_panel.null_placeholder =
                        workbench_shell::result_style::NullPlaceholderPref::parse(text);
                }
                persist(&cx.global::<Settings>().clone());
            }
            Slot::ResultPanelBoolDisplay => {
                let Some(text) = value.as_text() else {
                    return false;
                };
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.result_panel.bool_display =
                        workbench_shell::result_style::BoolDisplayPref::parse(text);
                }
                persist(&cx.global::<Settings>().clone());
            }
            Slot::ResultPanelAlignNumeric => {
                let Some(text) = value.as_text() else {
                    return false;
                };
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.result_panel.align_numeric =
                        workbench_shell::result_style::AlignPref::parse(text);
                }
                persist(&cx.global::<Settings>().clone());
            }
            Slot::ResultPanelAlignText => {
                let Some(text) = value.as_text() else {
                    return false;
                };
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.result_panel.align_text =
                        workbench_shell::result_style::AlignPref::parse(text);
                }
                persist(&cx.global::<Settings>().clone());
            }
            Slot::ResultPanelAlignTemporal => {
                let Some(text) = value.as_text() else {
                    return false;
                };
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.result_panel.align_temporal =
                        workbench_shell::result_style::AlignPref::parse(text);
                }
                persist(&cx.global::<Settings>().clone());
            }
            Slot::ResultPanelAlignBool => {
                let Some(text) = value.as_text() else {
                    return false;
                };
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.result_panel.align_bool =
                        workbench_shell::result_style::AlignPref::parse(text);
                }
                persist(&cx.global::<Settings>().clone());
            }
            Slot::ResultPanelAlignBlob => {
                let Some(text) = value.as_text() else {
                    return false;
                };
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.result_panel.align_blob =
                        workbench_shell::result_style::AlignPref::parse(text);
                }
                persist(&cx.global::<Settings>().clone());
            }
            Slot::ResultPanelColorNumeric => {
                let Some(text) = value.as_text() else {
                    return false;
                };
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.result_panel.color_numeric =
                        workbench_shell::result_style::ColorRolePref::parse(text);
                }
                persist(&cx.global::<Settings>().clone());
            }
            Slot::ResultPanelColorText => {
                let Some(text) = value.as_text() else {
                    return false;
                };
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.result_panel.color_text =
                        workbench_shell::result_style::ColorRolePref::parse(text);
                }
                persist(&cx.global::<Settings>().clone());
            }
            Slot::ResultPanelColorTemporal => {
                let Some(text) = value.as_text() else {
                    return false;
                };
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.result_panel.color_temporal =
                        workbench_shell::result_style::ColorRolePref::parse(text);
                }
                persist(&cx.global::<Settings>().clone());
            }
            Slot::ResultPanelColorBool => {
                let Some(text) = value.as_text() else {
                    return false;
                };
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.result_panel.color_bool =
                        workbench_shell::result_style::ColorRolePref::parse(text);
                }
                persist(&cx.global::<Settings>().clone());
            }
            Slot::ResultPanelColorBlob => {
                let Some(text) = value.as_text() else {
                    return false;
                };
                {
                    let settings = cx.global_mut::<Settings>();
                    settings.result_panel.color_blob =
                        workbench_shell::result_style::ColorRolePref::parse(text);
                }
                persist(&cx.global::<Settings>().clone());
            }
        _ => return false,
    }
    true
}
