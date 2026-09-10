# -*- coding: utf-8 -*-
"""临时打点：WorkbenchView::render + EditorPanel::render 加计数器打印。"""
import pathlib

v = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\view.rs')
t = v.read_text(encoding='utf-8')
old = '''impl Render for WorkbenchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.area.is_none() {'''
new = '''impl Render for WorkbenchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static N: AtomicUsize = AtomicUsize::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            eprintln!("[dbg] WorkbenchView::render #{n}");
        }
        if self.area.is_none() {'''
assert t.count(old) == 1
v.write_text(t.replace(old, new), encoding='utf-8')

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t2 = p.read_text(encoding='utf-8')
old2 = '''impl Render for EditorPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Round 23：受控输入懒创建（render 首次初始化，需要 window）。'''
new2 = '''impl Render for EditorPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static N: AtomicUsize = AtomicUsize::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            eprintln!("[dbg] EditorPanel::render #{n}");
        }
        // Round 23：受控输入懒创建（render 首次初始化，需要 window）。'''
assert t2.count(old2) == 1
p.write_text(t2.replace(old2, new2), encoding='utf-8')
print('dbg points added')
