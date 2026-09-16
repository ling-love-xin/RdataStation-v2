"""在 target/verify-tree 里拼出「HEAD + 本次改动」的树，用于编译与真机验证。

为什么需要它：工作区里同时有另一个会话的在途重构（workbench_shell / insight /
analytics_resource 正在搬东西，时有编不过的中间态），直接 `cargo check` 会拿到
他们的错、看不到我的。副本里他们的文件回到 HEAD，我的文件用本次改动。

**每次改完代码要同步 `MINE` 清单**，否则副本里跑的是旧代码——会得出假结论。

构建配置（`.cargo/config.toml` / `Cargo.toml` / `Cargo.lock`）刻意用**工作区版本**：
那套配置是动态链接 DuckDB，编译只要几十秒（HEAD 的 bundled 要编 C++ 内核十几分钟），
而它与本次改动的代码无关。

用法：`python verify_tree.py`（跑完在副本里 `cargo check/test`，用完 `rm -rf target/verify-tree`）
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
    "crates/engine/src/driver/utils.rs",
    "crates/engine/src/driver/native/mysql.rs",
    "crates/engine/src/driver/native/mysql_native.rs",
    "crates/engine/src/driver/native/postgres.rs",
    "crates/engine/src/driver/native/postgres_native.rs",
    "crates/engine/src/driver/native/sqlite.rs",
    "crates/engine/src/driver/native/duckdb.rs",
    "crates/engine/src/services/sql_service.rs",
    "crates/engine/src/connection_manager.rs",
    "crates/shared/src/error.rs",
    # 编辑器：结果区（B5）与错误回填（B6）
    "crates/editor/src/diagnostics.rs",
    "crates/editor/src/execution.rs",
    "crates/editor/src/lib.rs",
    "crates/editor/src/shared.rs",
    "crates/editor/src/store.rs",
    "crates/editor/src/ui.rs",
    "crates/editor/src/view/host.rs",
    "crates/editor/src/view/tests.rs",
    "crates/editor/src/view/widgets/result_grid.rs",
    "crates/editor/src/view/widgets/result_sets.rs",
    "crates/editor/src/view/widgets/status_bar.rs",
    # 宿主侧与真机探针
    "crates/workbench/src/services/editor_exec.rs",
    "crates/workbench/tests/editor_exec_real.rs",
    "crates/workbench/tests/ui_contract.rs",
    # 文档
    "docs/architecture/editor/editor-dev-plan.md",
    "docs/architecture/editor/editor-prototype-design.md",
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

    # 断言副本里我的改动在（拿一个已知点对一下）
    with open(os.path.join(VERIFY, "crates/workbench/src/services/editor_exec.rs"), encoding="utf-8") as handle:
        assert "fn cancel(&self" in handle.read(), "副本里缺我的中断实现"
    with open(os.path.join(VERIFY, "crates/editor/src/diagnostics.rs"), encoding="utf-8") as handle:
        assert "fn site_in_document" in handle.read(), "副本里缺我的错误定位实现"
    print("自检通过：副本 = HEAD + 本次改动")


if __name__ == "__main__":
    main()
