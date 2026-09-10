# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\README.md")
t = p.read_text(encoding="utf-8")
# 删除损坏段落（从 "## 资源目录" 到末尾）
idx = t.find("## 资源目录")
if idx != -1:
    t = t[:idx].rstrip() + "\n"
# 重新追加
BS = chr(92)  # backslash
add = (
    "\n## 资源目录（assets）\n\n"
    "| 目录 | 来源 | 内容 |\n"
    "| --- | --- | --- |\n"
    "| `assets" + BS + "icons` | v1 `src-tauri" + BS + "icons`（53 文件 7.1MB） | 应用图标：128/32/64 px PNG、android mipmap 系列、app-icon-coral-clean.png |\n"
    "| `assets" + BS + "public` | v1 `public`（7 文件 5.7MB） | 品牌主视觉（brand 3D story）、rds-icon-dark/light、popout.html 等 |\n"
)
t += add
p.write_text(t, encoding="utf-8")
print("README fixed")
