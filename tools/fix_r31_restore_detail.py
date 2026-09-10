# -*- coding: utf-8 -*-
"""恢复详情卡 + 子节点级打点。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

# 1. 移除 SKIP 标记（恢复详情卡代码）
old = '''        // [SKIP] 详情卡临时跳过
        eprintln!("[dbg] EP-detail-done");'''
new = '''        eprintln!("[dbg] EP-detail-card-build");
            content = content.child(
                div()
                    .v_flex()
                    .gap_2()
                    .w_full()
                    .rounded_md()'''
# 上面恢复方式不对（原代码被整段删除，需要重新插入）
print('need manual restore')
