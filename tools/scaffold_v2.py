# -*- coding: utf-8 -*-
"""RdataStation v2 脚手架：建目录、复制 v1 源码、生成占位 crate。"""
import os
import shutil
from pathlib import Path

V1 = Path(r"D:\RdataStation\RDS\RdataStation")
V2 = Path(r"D:\RdataStation\RDS\RdataStation-v2")

IGNORE_DIRS = {".git", "node_modules", "target", "__pycache__", ".venv", "venv"}

# ---------------------------------------------------------------- crates 定义
# (crate 名, package 名, 中文说明, 模块文件列表)
CRATES = [
    ("app", "rds-app", "App Shell：组合窗口与 Feature 装配，不承载业务逻辑", ["main.rs", "lib.rs"]),
    ("workbench", "rds-workbench", "工作台 Feature：Dock 布局、活动栏、命令面板、编辑器工作台", ["lib.rs", "model.rs", "commands.rs", "workbench_view.rs"]),
    ("project", "rds-project", "M1 双层数据架构：项目模型、系统级/项目级、promote、版本快照", ["lib.rs", "model.rs", "commands.rs", "project_view.rs", "promote_dialog.rs", "snapshot_dialog.rs"]),
    ("engine", "rds-engine", "M2 双引擎基础设施：SQLite 元数据仓储 + DuckDB 分析引擎 + 驱动/缓存/迁移/日志", ["lib.rs", "model.rs", "engine.rs"]),
    ("connection", "rds-connection", "M3 数据源连接：原生连接 + DuckDB Secret 加速通道", ["lib.rs", "model.rs", "commands.rs", "connection_view.rs", "connection_dialog.rs", "secret.rs"]),
    ("database", "rds-database", "M4 数据库导航：元数据浏览器、对象属性面板", ["lib.rs", "model.rs", "commands.rs", "database_view.rs", "property_panel.rs"]),
    ("scratchpad", "rds-scratchpad", "M5 草稿箱：草稿模型、文件树、自动保存", ["lib.rs", "model.rs", "commands.rs", "scratchpad_view.rs"]),
    ("analytics_resource", "rds-analytics-resource", "M6 资源分析：资源目录、版本、标签、回收站", ["lib.rs", "model.rs", "commands.rs", "resource_view.rs", "recycle_bin_dialog.rs"]),
    ("mock", "rds-mock", "M7 Mock：元数据驱动测试数据生成（只写分析引擎，不回写源库）", ["lib.rs", "model.rs", "commands.rs", "mock_view.rs", "generator.rs"]),
    ("insight", "rds-insight", "M8 洞察：库/表/列画像、规则引擎", ["lib.rs", "model.rs", "commands.rs", "insight_view.rs", "rule.rs"]),
    ("plugin", "rds-plugin", "M9 插件宿主：WASM / Sidecar / gpui-shell", ["lib.rs", "model.rs", "commands.rs", "plugin_view.rs", "host.rs"]),
    ("shared", "rds-shared", "跨 Feature 稳定能力（≥2 个真实使用方时才放入）", ["lib.rs"]),
]

MODULE_DOC = {
    "model": "领域模型（占位）。TODO(migration): 从 v1 对应模块迁入后填充。",
    "commands": "命令与 Action（占位）。TODO(migration): v1 的 Tauri 命令将演化为服务方法或 GPUI Action，按编码规范「事件、Action 与焦点」落地。",
    "view": "视图（占位）。TODO(migration): 按编码规范实现：Entity<T> 承载跨 frame 状态、render_* 拆分区域、theme token 取色、稳定 ElementId。",
    "dialog": "对话框（占位）。TODO(migration): 按编码规范「选择正确的组成单元」实现。",
}


def ignore(src, names):
    return {n for n in names if n in IGNORE_DIRS}


def write(path: Path, text: str):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def main():
    if V2.exists():
        print("V2 已存在，跳过创建：", V2)
    else:
        V2.mkdir(parents=True)

    # ------------------------------------------------------------ 复制 v1 源码
    copy_map = [
        ("src-tauri", "v1/backend"),
        ("src", "v1/frontend"),
        ("docs", "v1/docs"),
        ("tests", "v1/tests"),
        ("prototype", "v1/prototype"),
        ("public", "v1/public"),
        ("scripts", "v1/scripts"),
        (".trae", "v1/.trae"),
    ]
    for src_rel, dst_rel in copy_map:
        src = V1 / src_rel
        dst = V2 / dst_rel
        if src.exists():
            shutil.copytree(src, dst, ignore=ignore, dirs_exist_ok=True)
            print("copied:", src_rel, "->", dst_rel)
        else:
            print("skip(not exist):", src_rel)

    # 根目录文件（排除 .git）
    for item in sorted(V1.iterdir()):
        if item.name == ".git":
            continue
        if item.is_file():
            shutil.copy2(item, V2 / "v1" / item.name)
    print("root files copied to v1/")

    # ------------------------------------------------------------ 工作区文件
    write(V2 / "rust-toolchain.toml", '[toolchain]\nchannel = "stable"\ncomponents = ["rustfmt", "clippy"]\n')
    write(V2 / ".gitignore", "/target\n**/*.rs.bk\nnode_modules/\nv1/frontend/node_modules/\n")
    members = "\n".join(f'    "crates/{c[0]}",' for c in CRATES)
    write(
        V2 / "Cargo.toml",
        f"""[workspace]
resolver = "2"
members = [
{members}
]

[workspace.package]
version = "0.1.0"
edition = "2021"

[workspace.dependencies]
# 迁移时统一在此锁定版本；当前骨架不引入外部依赖。
# gpui-kit = "0.6"
# rusqlite = {{ version = "0.32", features = ["bundled"] }}
# duckdb = {{ version = "1", features = ["bundled"] }}
# tokio = {{ version = "1", features = ["rt-multi-thread"] }}
""",
    )

    # ------------------------------------------------------------ crate 占位
    for name, pkg, desc, files in CRATES:
        crate_dir = V2 / "crates" / name
        src_dir = crate_dir / "src"
        src_dir.mkdir(parents=True, exist_ok=True)

        write(
            crate_dir / "Cargo.toml",
            f"""[package]
name = "{pkg}"
version.workspace = true
edition.workspace = true
description = "{desc}"

[dependencies]
# 迁移时按需添加，并在 workspace.dependencies 统一锁定版本。
""",
        )

        for f in files:
            if f == "main.rs":
                write(
                    src_dir / "main.rs",
                    """//! RdataStation v2 应用入口（占位）。
//!
//! TODO(migration): 按 GPUI-kit 编码规范「初始化与 Root 所有权」实现：
//! 在 `app.run` 中先 `gpui_kit::init(cx)`，再 `open_window`，
//! 窗口第一层为 `Root::new(workspace, window, cx)`；
//! App Shell 只组合各 Feature crate，不承载业务逻辑。
//! 参考: https://gpui-kit.com/zh-CN/docs/coding-guides/#初始化与-root-所有权

fn main() {
    // 骨架阶段暂无实现；接入 gpui-kit 后在此装配各 Feature。
}
""",
                )
                continue
            if f == "lib.rs":
                mods = [m[:-3] for m in files if m.endswith(".rs") and m not in ("lib.rs", "main.rs")]
                mod_lines = "\n".join(f"pub mod {m};" for m in mods)
                write(
                    src_dir / "lib.rs",
                    f"""//! {pkg} — {desc}。
//!
//! 规范说明：本 crate 是 v2 骨架占位，按 GPUI-kit 编码规范组织：
//! 同一业务能力的 model / service / view / command / dialog / workflow 放在一起。
//! 迁移源与逐项映射见 `docs/migration/v1-to-v2-mapping.md`。
#![allow(dead_code)]

{mod_lines}
""",
                )
                continue
            # 普通模块文件
            key = None
            for k in ("model", "commands", "dialog"):
                if f.startswith(k) or k in f:
                    key = k
                    break
            if f.endswith("_view.rs"):
                key = "view"
            doc = MODULE_DOC.get(key, "模块占位。TODO(migration): 迁移时填充。")
            write(
                src_dir / f,
                f"""//! {pkg} — {f[:-3]}。
//!
//! {doc}
""",
            )

    print("\nDONE. 结构已生成于:", V2)


if __name__ == "__main__":
    main()
