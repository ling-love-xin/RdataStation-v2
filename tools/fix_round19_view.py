import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\view.rs')
t = p.read_text(encoding='utf-8')

t = t.replace(
    'Button::new(cx)\n                    .icon(icon)\n                    .size(px(28.))\n                    .ghost()\n                    .toggled(self.active_tool == tool)\n                    .id(format!("activity-{}", tool.label()))',
    'Button::new(format!("activity-{}", tool.label()))\n                    .icon(icon)\n                    .size(px(28.))\n                    .ghost()\n                    .toggled(self.active_tool == tool)',
)
t = t.replace('Button::new(cx)\n                            .id("sidebar-toggle")', 'Button::new("sidebar-toggle")')
t = t.replace('Button::new(cx)\n                    .id("new-connection")', 'Button::new("new-connection")')
t = t.replace('app.update(entity.clone(), |this, cx| {', 'entity.clone().update(app, |this, cx| {')
t = t.replace('name: "本地 MySQL 分析库",', 'name: "本地 MySQL 分析库".into(),')
t = t.replace('name: "生产 PostgreSQL 主库",', 'name: "生产 PostgreSQL 主库".into(),')
t = t.replace('name: "分析数仓 DuckDB",', 'name: "分析数仓 DuckDB".into(),')
t = t.replace('name: "沙箱 SQLite 测试库",', 'name: "沙箱 SQLite 测试库".into(),')

p.write_text(t, encoding='utf-8')
print('activity-new:', t.count('Button::new(format!'))
print('sidebar-new:', t.count('Button::new("sidebar'))
print('newconn-new:', t.count('Button::new("new-connection'))
print('entity-update:', t.count('entity.clone().update(app'))
print('app-update-remain:', t.count('app.update('))
