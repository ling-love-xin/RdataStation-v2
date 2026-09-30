"""在 target/verify-tree 里拼出「HEAD + 本次改动」的树，用于编译与真机验证。

为什么需要它：工作区里同时有另一个会话的在途重构（workbench_shell / insight /
analytics_resource 正在搬东西，时有编不过的中间态），直接 `cargo check` 会拿到
他们的错、看不到我的。副本里他们的文件回到 HEAD，我的文件用本次改动。

**每次改完代码要同步 `MINE` 清单**，否则副本里跑的是旧代码——会得出假结论。

构建配置（`.cargo/config.toml` / `Cargo.toml` / `Cargo.lock`）刻意用**工作区版本**：
那套配置是动态链接 DuckDB，编译只要几十秒（HEAD 的 bundled 要编 C++ 内核十几分钟），
而它与本次改动的代码无关。

**不要给副本指定 `CARGO_TARGET_DIR=<主 target>`**：同包名同 target 名在不同路径下会算出同一个
fingerprint 目录，两边互相顶掉产物与 dep-info——实测跑出过 235 项旧测试（新测试一个没进），
用完还得删本才能恢复。副本用**自己的 target**（副本目录里的 `target/`），代价是首轮要全量编。

用法：`python verify_tree.py` → `cd target/verify-tree && cargo check/test …`（**不设** `CARGO_TARGET_DIR`）
→ 用完 `rm -rf target/verify-tree`
"""

import os
import shutil
import subprocess

ENV = dict(
    os.environ,
    # 用**绝对路径**：`checkout-index` 带 `--work-tree` 时，相对路径的 index 会被按
    # 工作树去解析（“is not in the cache”就是这么来的）
    GIT_INDEX_FILE=os.path.abspath(os.path.join(".git", "index.b3")),
)
VERIFY = os.path.abspath(os.path.join("target", "verify-tree"))

# 本次改动涉及的文件（**每次改完要同步**）
MINE = [
    # 引擎：影响行数（B5-1）与错误位置（B6）
    "crates/engine/tests/duckdb_extensions_probe.rs",
    "crates/engine/tests/federation_probe.rs",
    "crates/engine/src/driver/utils.rs",
    "crates/engine/src/driver/native/mysql.rs",
    "crates/engine/src/driver/native/mysql_native.rs",
    "crates/engine/src/driver/native/postgres.rs",
    "crates/engine/src/driver/native/postgres_native.rs",
    "crates/engine/src/driver/native/sqlite.rs",
    "crates/engine/src/driver/native/duckdb.rs",
    "crates/engine/src/services/sql_service.rs",
    "crates/engine/src/persistence/history_store.rs",
    "crates/engine/src/sql/filter.rs",
    "crates/engine/src/sql/formatter.rs",
    "crates/engine/src/sql/engine.rs",
    "crates/engine/src/sql/explain.rs",
    "crates/engine/src/sql/script.rs",
    "crates/engine/src/sql/transpiler.rs",
    "crates/engine/src/sql/mod.rs",
    "crates/engine/src/connection_manager.rs",
    "crates/engine/src/duckdb/accel.rs",
    "crates/engine/src/duckdb/mod.rs",
    "crates/engine/src/duckdb/federation/mod.rs",
    "crates/engine/src/duckdb/federation/legacy.rs",
    "crates/engine/src/duckdb/federation/registry.rs",
    "crates/engine/src/duckdb/federation/session.rs",
    "crates/engine/src/duckdb/federation/bridge.rs",
    "crates/engine/src/duckdb/manager.rs",
    "crates/engine/src/duckdb/mod.rs",
    "crates/engine/src/persistence/workbench_context_store.rs",
    "crates/engine/src/services/execution_service.rs",
    "crates/engine/tests/duckdb_accel_probe.rs",
    "crates/engine/tests/transaction_affinity.rs",
    "crates/insight/src/schema_analyzer.rs",
    "crates/insight/src/service/persistence.rs",
    "crates/insight/src/table_profile_service.rs",
    "crates/shared/src/error.rs",
    # 编辑器：结果区（B5）与错误回填（B6）
    "crates/editor/Cargo.toml",
    "crates/editor/src/format.rs",
    "crates/editor/src/translate.rs",
    "crates/editor/src/store.rs",
    "crates/editor/src/view/results/sets.rs",
    "crates/editor/src/commands.rs",
    "crates/editor/src/connection.rs",
    "crates/editor/src/channel.rs",
    "crates/editor/src/diagnostics.rs",
    "crates/editor/src/execution.rs",
    "crates/editor/src/export.rs",
    "crates/editor/src/history.rs",
    "crates/editor/src/lib.rs",
    "crates/editor/src/service.rs",
    "crates/editor/src/session.rs",
    "crates/editor/src/shared.rs",
    "crates/editor/src/store.rs",
    "crates/editor/src/ui.rs",
    "crates/editor/src/view/host.rs",
    "crates/editor/src/view/history.rs",
    "crates/editor/src/view/mod.rs",
    "crates/editor/src/view/results/error_card.rs",
    "crates/editor/src/view/results/grid.rs",
    "crates/editor/src/view/results/mod.rs",
    "crates/editor/src/view/results/sets.rs",
    "crates/editor/src/view/tests.rs",
    "crates/editor/src/view/widgets/mod.rs",
    "crates/editor/src/view/widgets/status_bar.rs",
    # 宿主侧与真机探针（view.rs 不搬：现在装着另一个会话的 quick_open 在途改动，副本回退 HEAD）
    "crates/workbench/src/services/editor_channels.rs",
    "crates/workbench/src/services/editor_connections.rs",
    "crates/workbench/src/services/editor_exec.rs",
    "crates/workbench/src/services/editor_files.rs",
    "crates/workbench/src/services/editor_session.rs",
    "crates/workbench/src/services/mod.rs",
    "crates/workbench/src/panels/right.rs",
    "crates/app/src/main.rs",
    "crates/workbench/tests/editor_exec_real.rs",
    # ui_contract.rs 不搬：它正被另一个会话改（quick_open 的视图登记），副本用 HEAD 版正好自洽
    # 文档
    "docs/architecture/editor/README.md",
    "docs/architecture/editor/editor-architecture.md",
    "docs/architecture/editor/editor-dev-plan.md",
    "docs/architecture/editor/editor-prototype-design.md",
    # 联邦（设计定稿 + 目录落位）
    "docs/architecture/federation/README.md",
    "docs/architecture/federation/federation-architecture.md",
    "docs/architecture/federation/federation-prototype-design.md",
    "docs/architecture/federation/federation-dev-plan.md",
]

# 编译需要的路径（不搬 docs / v1，省时间与空间）
# **不搬 `.cargo/`**：副本嵌在仓库里，cargo 会把仓库根的 `.cargo/config.toml` 合并进来，
# 两份一起给 `DUCKDB_LIB_DIR` 会报 “failed to merge key env”；只留根那一份正好指向
# 正确的预编译库目录（`relative = true` 相对根配置解析）。
PATHS = ["crates", "assets", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml"]

# 用工作区的构建配置覆盖副本（动态 DuckDB：快）
CONFIG = ["Cargo.toml", "Cargo.lock"]


def main():
    if os.path.isdir(VERIFY):
        shutil.rmtree(VERIFY)
    os.makedirs(VERIFY)

    subprocess.run(["git", "read-tree", "HEAD"], env=ENV, check=True)
    subprocess.run(["git", "add", "--", *MINE], env=ENV, check=True)

    listed = subprocess.run(
        ["git", "ls-files", "-z", "--", *PATHS], capture_output=True, env=ENV, check=True
    ).stdout
    files = [name.decode("utf-8") for name in listed.split(b"\0") if name]
    # 分批给：命令行长度有上限（Windows 约 32K），一次几百个路径会超
    for start in range(0, len(files), 100):
        chunk = files[start : start + 100]
        done = subprocess.run(
            ["git", f"--work-tree={VERIFY}", "checkout-index", "-f", "--", *chunk],
            capture_output=True,
            env=ENV,
        )
        if done.returncode != 0:
            raise SystemExit(f"checkout-index 失败：{done.stderr.decode('utf-8', 'replace')}")
    print(f"副本已写出 {len(files)} 个文件 → {VERIFY}")

    for name in CONFIG:
        shutil.copy2(name, os.path.join(VERIFY, name))
        print(f"覆盖构建配置 {name}")

    # 断言副本里我的改动在（拿几个已知点对一下）
    with open(os.path.join(VERIFY, "crates/workbench/src/services/editor_exec.rs"), encoding="utf-8") as handle:
        assert "fn cancel(&self" in handle.read(), "副本里缺我的中断实现"
    with open(os.path.join(VERIFY, "crates/editor/src/diagnostics.rs"), encoding="utf-8") as handle:
        assert "fn site_in_document" in handle.read(), "副本里缺我的错误定位实现"
    with open(os.path.join(VERIFY, "crates/editor/src/export.rs"), encoding="utf-8") as handle:
        assert "pub fn menu_items" in handle.read(), "副本里缺导出实现（MINE 没同步？）"
    with open(os.path.join(VERIFY, "crates/engine/src/services/sql_service.rs"), encoding="utf-8") as handle:
        assert "fn unwrap_segment_error" in handle.read(), "副本里缺包装错误的还原实现"
    with open(os.path.join(VERIFY, "crates/editor/src/format.rs"), encoding="utf-8") as handle:
        assert "pub fn plan" in handle.read(), "副本里缺格式化计划实现（MINE 没同步？）"
    with open(os.path.join(VERIFY, "crates/engine/src/sql/formatter.rs"), encoding="utf-8") as handle:
        assert "format_with_report" in handle.read(), "副本里缺格式化报告实现"
    with open(os.path.join(VERIFY, "crates/editor/src/translate.rs"), encoding="utf-8") as handle:
        assert "pub fn targets_for" in handle.read(), "副本里缺转译目标表（MINE 没同步？）"
    with open(os.path.join(VERIFY, "crates/engine/src/sql/explain.rs"), encoding="utf-8") as handle:
        assert "pub fn explain_sql" in handle.read(), "副本里缺执行计划前缀实现"
    with open(os.path.join(VERIFY, "crates/engine/src/duckdb/federation/mod.rs"), encoding="utf-8") as handle:
        assert "pub mod session;" in handle.read(), "副本里缺联邦目录骨架（MINE 没同步？）"
    print("自检通过：副本 = HEAD + 本次改动")
    print(f"下一步：cd {os.path.relpath(VERIFY)} && cargo check -p rds-workbench — **不要**设 CARGO_TARGET_DIR")


if __name__ == "__main__":
    main()
