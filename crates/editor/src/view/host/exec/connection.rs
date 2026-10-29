//! channel 域（40-74 行区间）
//!
//! 自 view/host/exec.rs 拆分（2026-10-29 Step 10）：按能力域切分 `impl EditorHostPanel`。
//! 跨域互调经 `pub(crate)` 方法；本文件不得出现新的逻辑。


use crate::channel::{self, ExecChannel};

use super::*;

impl EditorHostPanel {
    /// 本文档绑定的连接 id（`None` = 未绑定，执行跟随当前活动连接）
    pub(crate) fn bound_connection(&self) -> Option<String> {
        self.with_document(|doc| doc.connection().map(str::to_string))
            .flatten()
    }

    /// 本文档当前的执行通道（B13；文档属性，读的是服务层的真值）
    pub fn channel(&self) -> ExecChannel {
        self.with_document(|doc| doc.channel()).unwrap_or_default()
    }

    /// 源库档能不能用：绑定的连接已建连（**未绑定算可用**——那是在跟随当前活动连接，
    /// 它到底连没连不归这里断言，真发错东西了由执行器报可读原因）
    pub(crate) fn source_channel_ready(&self) -> bool {
        match self.bound_connection() {
            None => true,
            Some(id) => self
                .shared
                .connection_options()
                .iter()
                .find(|option| option.id == id)
                .is_some_and(|option| option.connected),
        }
    }

    /// 【B13】「执行位置 ▾」的菜单项（纯函数算的：可用性 + 不可用原因 + 当前项打勾）
    ///
    /// 门控真值来自宿主注入的 `ChannelsPort`；**没注入端口就不给选**（如实报“尚未接入”）。
    pub fn channel_menu(&self) -> Vec<channel::ChannelMenuItem> {
        let availability = self
            .shared
            .channel_availability(self.bound_connection().as_deref());
        channel::menu_items(self.channel(), self.source_channel_ready(), &availability)
    }
}
