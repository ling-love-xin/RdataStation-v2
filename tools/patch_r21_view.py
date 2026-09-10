# -*- coding: utf-8 -*-
"""view.rs 接入真实连接加载 + 移除 sample 占位。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\view.rs')
t = p.read_text(encoding='utf-8')

old_new = '''    pub fn new() -> Self {
        Self {
            shared: Shared::new(),
            area: None,
            sidebar: None,
            editor: None,
            _subscription: None,
        }
    }'''
new_new = '''    pub fn new() -> Self {
        // Round 21：从全局系统库加载真实连接（失败降级为空列表 + 提示）。
        let (connections, notice) = crate::services::workspace_loader::load_persisted_connections();
        Self {
            shared: Shared::with_connections(connections, notice),
            area: None,
            sidebar: None,
            editor: None,
            _subscription: None,
        }
    }'''
assert old_new in t, 'new() not found'
t = t.replace(old_new, new_new)

# 移除 sample
start = t.find('impl ConnectionItem {')
end = t.find('/// 工作台视图：标题栏')
assert start != -1 and end != -1 and start < end, (start, end)
t = t[:start] + t[end:]
t = t.replace(
    '/// 连接条目（首版占位数据，下一轮由 ConnectionService 提供）。',
    '/// 连接条目（Round 21 起由全局系统库真实数据填充）。',
)
p.write_text(t, encoding='utf-8')
print('view patched ok')
