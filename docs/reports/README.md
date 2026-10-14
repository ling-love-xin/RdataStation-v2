# docs/reports —— 验证 / 集成测试 / 交接文档

> 约定（2026-10-14 用户明确）：**报告、说明、交接文档不放在 `.rds/tmp/` 或 `docs/tmp/`**（这两处会被 `tools/clean-temp.sh` 等清理误删）。凡属"给人读 / 给后续会话读"的文档一律入库到本目录。

## 目录内容

| 文件 | 内容 |
| --- | --- |
| `integration-test-report-<日期>.md` | 整体联动集成测试全量报告（含逐包对照、修复记录、缺口清单） |
| `integration-test-report-incremental-<日期>.md` | 增量复验报告（相对上一份的差异） |
| `integration-task-next.md` | 下一会话独立任务说明（自包含：前置状态 / 环境要点 / 执行步骤 / 验收口径） |

## 命名约定

- 报告：`integration-test-report-<yyyy-MM-dd>.md`，增量轮用 `integration-test-report-incremental-<yyyy-MM-dd>.md`。
- 历史轮次保留：`integration-test-report-2026-10-14.md` 是首轮基线（含逐包 2026-09-19 对照），增量报告与之并列保留，供追溯。

## 相关约定

- 可复用工具脚本 → `tools/`（入库）；一次性过程脚本 → `tools/_archive/`（不入库）。
- 验证/构建/回归**日志**仍落 `.rds/tmp/` 或 `docs/tmp/`（gitignore）。权威出处：`docs/architecture/runtime/data-paths.md` §6.3。
