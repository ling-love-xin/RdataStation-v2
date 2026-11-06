//! 分组：折叠、默认分组与拖拽落组。

use super::*;

impl ResourcesPanel {
    /// 折叠 / 展开一个分组（分组头点击；`GROUP_ALL` / `GROUP_UNGROUPED` 也是合法的 key）。
    ///
    /// 折叠态不是纯视图状态：它要被记住（见 `remember_collapsed`），否则每次打开项目
    /// 都得重新折一遍。
    pub fn toggle_group_collapse(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.collapsed.insert(key.to_string()) {
            self.collapsed.remove(key);
        }
        self.refresh_view_items(cx);
        let keys = self.collapsed_keys();
        self.host.remember_collapsed(&keys, cx);
        cx.notify();
    }
    /// 折叠集合（**排序后**的：写进设置里的东西不该每次换个顺序）。
    pub fn collapsed_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.collapsed.iter().cloned().collect();
        keys.sort();
        keys
    }
    /// 把一批行拖到分组头（原型 §3.2）。
    ///
    /// 拖拽是「移动到分组」的**第二条入口**，不是第二套实现：落点解成目标分组后，
    /// 走的仍是宿主的 `request_move_to_group`（面板不碰库），连「拖到原分组 = 无操作」
    /// 也与菜单里当前项置灰同一口径（`dnd::rows_to_move`）。
    ///
    /// 无落点（「全部分组」聚合头）或没有行真的需要改时**不发请求**：拖着鼠标扫过一行
    /// 分组头不应该产生一次写库 + 一条回执。
    pub fn drop_rows_onto_group(
        &mut self,
        ids: &[String],
        target: GroupDrop,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.snapshot.read_only {
            return;
        }
        let moving = dnd::rows_to_move(&self.view_rows, ids, &target);
        if moving.is_empty() {
            return;
        }
        self.host
            .request_move_to_group(&moving, dnd::drop_folder(&target), window, cx);
        cx.notify();
    }
    /// `F2` / 「重命名…」的**目标**：单选语义 + 只读项目不给改（判据在 [`can_rename`]）。
    ///
    /// 与菜单项同一判据：改名没有“批量”的意义（改哪一条都是猜），只管焦点行。
    pub fn rename_target(&self) -> Option<String> {
        can_rename(self.multi.len(), self.snapshot.read_only)
            .then(|| self.selected.clone())
            .flatten()
    }
    /// 归档默认分组（当前值；`None` = 没设）。
    pub fn default_group(&self) -> Option<&str> {
        self.default_group.as_deref()
    }
    /// 注入默认分组（宿主构造期从设置读；**不回调宿主**——否则注入会被当成用户动作回写）。
    pub fn set_default_group(&mut self, folder_id: Option<String>, cx: &mut Context<Self>) {
        self.default_group = folder_id;
        // 标记画在分组头上，而头由**列表委托**渲染：得把新值推给它（否则要等下一次快照推送才更新）。
        self.push_rows_to_list(cx);
        cx.notify();
    }
    /// 设为 / 取消「归档默认分组」（分组头右键，原型 §4.1 的预选来源）。
    ///
    /// 与 [`toggle_group_collapse`](Self::toggle_group_collapse) 同一形态：先改本地（标记立刻
    /// 对上），再把结果交宿主落盘。它**不碰**已有存档的归属——只影响下一次归档的预选值。
    pub fn toggle_default_group(&mut self, folder_id: &str, cx: &mut Context<Self>) {
        let next =
            (self.default_group.as_deref() != Some(folder_id)).then(|| folder_id.to_string());
        self.default_group = next.clone();
        self.push_rows_to_list(cx);
        self.host.remember_default_group(next.as_deref(), cx);
        cx.notify();
    }
    /// 注入折叠集合（宿主在构造期从设置里读出来；不回调宿主——否则默认值会被当成用户动作回写）。
    pub fn set_collapsed(&mut self, keys: &[String], cx: &mut Context<Self>) {
        self.collapsed = keys.iter().cloned().collect();
        self.refresh_view_items(cx);
        cx.notify();
    }
}
