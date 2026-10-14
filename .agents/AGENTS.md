# RdataStation-v2 工作区约定（供 AI 助手 / 各会话遵守）

本文件是开发期**过程产物去向**的会话级约定。权威文档：`docs/architecture/runtime/data-paths.md` §6.3。

## 你的输出该放哪（三句话）

1. **验证 / 构建 / 回归日志** → `docs/tmp/` 或 `.rds/tmp/`（两者均已 gitignore；**绝不**写在仓库根、`crates/`、`tools/`）
2. **一次性过程脚本**（fix_/migrate_/inspect_ 这类跑完就扔的）→ `tools/_archive/`（不入库；可复用工具才放 `tools/`）
3. **手动命令输出**（diff、检查、反汇编、截图、列表）→ 统一重定向到 `.rds/tmp/<名字>`，**绝不**落在当前工作目录

**报告 / 说明 / 交接文档**（给人读、给后续会话读的）：**不放 `.rds/tmp/` 或 `docs/tmp/`**（会被 `clean-temp` 误删），一律入 `docs/reports/`（入库）——用户 2026-10-14 明确。可复用工具脚本入 `tools/`（入库）。

## 硬约束（gitignore 已挡，无需再确认）

- 根目录 `*.log`、`*.asm`、`*.bak`、`tools/*.exe/.pdb`、`examples/instance-project/` 生成物一律不入库
- `.rds/`（运行时数据 + 临时）整体不入库；`.rds/tmp/` 可随时清空（`tools/clean-temp.sh`）
- 编译产物、测试残留、DuckDB/global.db 快照**从不**提交

## 提交前自查

```bash
tools/hygiene-check.sh          # 报告三处规划外条目（根 / .rds 顶层 / tools）
tools/hygiene-check.sh --yes    # 把偏离项移入 .rds/tmp（两阶段删除）
```

发现偏离先归位再提交。不要改白名单脚本绕过检查——要改先改 data-paths.md §6.3 再同步脚本。
