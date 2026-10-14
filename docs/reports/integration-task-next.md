# RdataStation v2 集成测试——下一会话独立任务说明

> 用途：交给另一个会话独立执行。本说明自包含：前置状态（含本轮已完成项）、验收口径、执行步骤、过程产物约定、插件口径。执行者不需要回看主会话。
> 生成时间：2026-10-14（晚）· 生成者：主会话（本轮已完成「清理 + 全量复验 + 0 告警确认 + 增量报告」）
> 后续维护：本文件已归位到 `docs/reports/`（入库，不再放 `.rds/tmp/`，避免误删）；可复用脚本已从 `.rds/tmp/` 迁到 `tools/`（见 §6）。

---

## 0. 任务目标（一句话）

在**主会话已验证全绿**的基础上，对 RdataStation v2 工作区继续做整体联动集成维护：维持 `cargo check --workspace --all-targets` 零告警 + `cargo test --workspace` 全绿，处理后续出现的任何告警/回归并提交，产出增量报告。**插件（M9）不接入**（用户明确「先不用管插件模式，目前是先不接入，后面差不多后再接入」）。

## 1. 前置状态（已验证，直接采信）

| 项 | 状态 |
| --- | --- |
| Git 分支 | `main`，工作树**干净**（`git status` 无输出），领先 `origin/main` **8 笔**提交，未 push |
| 最近提交 | `589973c4` docs(plugin) §14 场景示例 · `0a54049b` docs(plugin) 落地状态 · `ca7cc12b` fix(scratchpad) chrome.rs 乱码 · `65a5d856` Fix 103 compiler warnings |
| 全量集成测试（本轮复验） | **116 目标 ↔ 116 结果全配对，2775 通过 / 0 失败 / 44 忽略**，逐目标与上轮 **0 差异**（日志 `.rds/tmp/rds-integration-final-2026-10-14.log`） |
| 全量 check（本轮） | `Finished dev profile in 51.40s`，**0 warning / 0 error**（日志 `.rds/tmp/rds-check-final-2026-10-14.log`） |
| 项目过程信息 | 已清理：删 795 个测试残留目录 + 27 个测试垃圾文件（`.rds/tmp/` 下），保留 101 个日志/脚本过程产物 |
| 增量报告 | `docs/reports/integration-test-report-incremental-2026-10-14.md`（本轮，已入库） |

### 环境要点（必须遵守，否则编译失败）

- **cargo 不在 PATH**，必须全路径 + 绕 rustup shim：
  ```
  set RUSTC=D:/Dev/Rust/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustc.exe
  set RUSTDOC=D:/Dev/Rust/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustdoc.exe
  D:/Dev/Rust/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/cargo.exe <子命令> -j 2
  ```
  裸调 `cargo` 报 `os error 448（不受信任的装入点）`；用 `cmd /c "set RUSTC=... && set RUSTDOC=... && cargo.exe ..."` 包一层即可（已实测）。
- `.cargo/config.toml` 强制 `TEMP/TMP/TMPDIR=.rds/tmp`、`RDS_HOME=.rds`，**不要改**。
- 长编译（workbench 目标）超过工具超时：**用分离进程**。参考 `.rds/tmp/launch_workspace_test_final.ps1`（`Start-Process cmd /c wrapper.cmd -WindowStyle Hidden -PassThru`）+ wrapper（`.rds/tmp/run_integration_final.cmd`，内部重定向日志 + `echo EXIT_CODE=%ERRORLEVEL%`）+ 轮询（`.rds/tmp/poll_test_final.ps1`）。
- **不用 `head`/`tail`/`findstr` 截断**：工具自带 `head_lines`/`tail_lines` 参数；windows shell 里 `tail` 不存在。PowerShell 脚本里 `$` 变量会被 bash 吞，**一律写成 `.ps1` 文件用 `-File` 执行**。
- 中文提交信息用 `git commit -F <消息文件>`（`cmd /c "set GIT_EDITOR=true && git commit"` 写法在主会话也验证可用）。

## 2. 插件（M9）口径——本轮用户明确

- **不接入**：`crates/plugin` 整包**无任何 crate 依赖它**，此状态保持不变；**不要**把 plugin 加进任何 workspace 依赖 / 模块接线 / 测试目标。
- **只保留文档层推进**：`plugin-prototype-design.md` / `plugin-dev-plan.md` 等文档可继续补，但必须纯文档、不进构建。
- 若后续用户说「接入」，那才是单独立项，不在本说明范围。

## 3. 执行步骤（按需触发，非一次性全做）

### 步骤 A：零告警闸门（每次改动后必跑）

```bat
cmd /c "set RUSTC=D:/Dev/Rust/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustc.exe && set RUSTDOC=D:/Dev/Rust/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustdoc.exe && D:/Dev/Rust/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/cargo.exe check --workspace --all-targets -j 2 > .rds/tmp/rds-check-<date>.log 2>&1 && echo EXIT_OK || echo EXIT_FAIL"
```

- 通过标准：出现 `Finished`，无 `error`、无 `warning:` 行。
- 告警处置：删未用 / 补下划线前缀 / `#[allow]` 局部化（只用于确有设计意图的预留 API）；修复后重跑到 0。
- 主会话 wrapper/轮询模板（可复制改名）：`run_check_final.cmd` / `launch_check_final.ps1` / `poll_check_final.ps1`。

### 步骤 B：全量集成测试（改动涉及多包/收尾时）

```bat
set RUSTC=...&& set RUSTDOC=...&& cargo.exe test --workspace -j 2 --no-fail-fast > .rds/tmp/rds-integration-<date>.log 2>&1
```

- 通过标准（与报告同口径）：
  - `Running` / `Doc-tests` 目标数 ↔ `test result:` 行数**全部配对**（用 `.rds/tmp/verify_integration_final.ps1` 校验）；
  - **0 个 `FAILED`**、0 个 `error[`；
  - 尾部为 cargo 最终输出（`all doctests ran` / `Finished`）。
- 目标数变动：比 116 多是新增（正常）；少则核对漏跑。
- 单包复跑：`cargo test -p rds-<name> -j 2`。

### 步骤 C：产出增量报告

- 复制 `docs/reports/integration-test-report-incremental-2026-10-14.md` 为 `docs/reports/integration-test-report-incremental-<date>.md`，更新摘要/逐目标差异/告警/缺口。
- 报告**入库**到 `docs/reports/`（不落 `.rds/tmp/`）。

### 步骤 D：提交

- 只提交本次改动的代码/文档；不碰无关改动；不 push。
- 项目提交风格：`fix(...)`/`feat(...)`/`docs(...)` 前缀，subject ≤50 字符，正文 72 折行，中文可用。用 `git commit -F` 消息文件避免转义。
- 提交前 `git diff` 核对内容归属（本仓库存在并发会话/用户改动的先例——上轮 `plugin-prototype-design.md` 就是并发提交的 `589973c4`）。

## 4. 报告/说明文档与工具脚本的去向（2026-10-14 用户新约定）

- **报告 / 说明 / 交接文档**（本文件、集成报告等）→ `docs/reports/`（**入库**，不放在 `.rds/tmp/` 或 `docs/tmp/`，用户明确这两处会被误删）。
- **可复用工具脚本** → `tools/`（入库，见 §6 清单）。
- **一次性过程脚本** → `tools/_archive/`（不入库）。
- 验证/构建/回归**日志**仍按 `.agents/AGENTS.md` 落 `.rds/tmp/` 或 `docs/tmp/`。

## 5. 回归处置口径

1. **复现**：单包复跑定位（`cargo test -p <pkg>` / `cargo check -p <pkg>`），读日志尾部 `error[E....]` 与源码行。
2. **根因优先**：先定位根因再改；不删断言/弱化断言硬过。
3. **最小改动面**：只改出问题的那一个文件；无关的既有问题只记录不修（写进报告缺口）。
4. **逐级放行**：单包 → 相关包 → 全量。
5. **历史教训（勿回退）**：
   - `mock_generator.rs` 曾因 `parse_data_type` 签名改 `Result` 未跟进而 E0308——涉及签名改动要全局搜调用方。
   - chrome.rs 曾有 648 处 U+FFFD 注释乱码——**任何新改动不得引入 FFFD**（`py .rds/tmp/scan_repo_fffd.py` 复查）。

## 6. 已知联动缺口（非本次范围，报告中照录即可）

对照 `docs/architecture/module-status.md` §3/§5（状态不变，**不处理**）：
- M9 plugin 未接 workbench（本轮用户明确先不接入）。
- engine L2 缓存零生产调用；缓存身份指纹（`meta_{fp}.sqlite`）未接线；增量同步/快照表不补。
- 联邦查询 L3 桥接 / SQL Server 真机待验；M6 引用档 Phase 5 未接 UI；M4 虚拟列表未落地；Quick Open Phase 2 未做；settings `effect` 无人消费。

## 7. 过程产物约定

- **日志/临时文件**落 `.rds/tmp/`（gitignore，不入库），不留工作目录；**报告/说明文档**入 `docs/reports/`。
- **可复用脚本（已迁到 `tools/`，入库）**：
  - 全量测试：`tools/run-integration-test.cmd` + `tools/launch-integration-test.ps1` + `tools/poll-integration-test.ps1`
  - 全量 check：`tools/run-check.cmd` + `tools/launch-check.ps1` + `tools/poll-check.ps1`
  - 配对校验 `tools/verify-integration-log.ps1`（`-Log <文件>`）、逐目标对照 `tools/diff-test-runs.ps1`（`-LogA/-LogB`）、告警计数 `tools/count-warnings.ps1`（`-Log <文件>`）、乱码扫描 `tools/scan-fffd.py`、清理 `tools/cleanup-test-residue.ps1`
- 测试残留目录会随每次 `cargo test` 重新产生（`rds-test-root-*`/`rds_*`/`rd_duckdb_test_*` 等），占空间时可再用 `tools/cleanup-test-residue.ps1` 清理；`.rds` 根下非 tmp 内容（global.db/设置）**勿删**。

## 8. 完成判据

- [ ] 步骤 A：`check --workspace --all-targets` Finished + 0 warning + 0 error
- [ ] 步骤 B（如触发）：全量测试 0 FAILED、目标/结果行配对、2775 通过 / 0 失败 / 44 忽略（或按新增目标数相应增长）
- [ ] 步骤 C：增量报告已产出到 `docs/reports/integration-test-report-<date>.md`（入库，不落 tmp）
- [ ] 步骤 D：本次修复已提交，工作树干净
- [ ] 插件 M9：未做任何代码接入（保持 0 调用方）
