# RdataStation-v2 AI 协作约定

## GPUI-kit 查询必须用本地 MCP 索引（禁止联网）

本工作区对 gpui-kit（`GPUI-kit 0.8.x`）的组件/API 查询有**强制约束**：

1. **查询 gpui-kit 一律走本地 MCP 索引**，不得使用 `fetch` / 浏览器 / 任何联网方式查询 gpui-kit 组件用法。GPUI-kit 全部源码（0.7.1/0.7.2/0.7.3/0.8.0）已本地索引，权威性高于任何在线文档。
2. 可用的 MCP 工具（按场景选用）：
   - `gpuikit_search_graph` / `gpuikit_search_code`：按名字/关键词找符号
   - `gpuikit_get_code_snippet`：读符号源码（含邻居/大纲）
   - `gpuikit_get_file_outline`：看某文件的结构
   - `gpuikit_get_architecture`：模块/依赖/入口点总览
   - `gpuikit_query_graph`：多跳/聚合查询（如"谁调用了某组件"）
   - `gpuikit_list_projects` / `gpuikit_index_status`：确认索引在线与覆盖
   - `gpuikit_get_graph_schema`：图结构说明
   - 如会话中工具以 `mcp:GPUIKIT:*` 命名空间暴露，则使用对应 `mcp:GPUIKIT:*` 形式。
3. 写/改 GPUI 视图代码前，**必须**先读 `.agents/skills/gpui-kit-dev/SKILL.md`（GPUI-kit 视图开发规范：依赖只向下、主题取色、组件选型禁手搓、动效、API 查证路径；0.7.1 实测细节见 skill 内标注，0.8.0 升级差异见 `docs/architecture/ui/gpui-kit-0.8.0-upgrade-report.md`）。该 skill 的查证路径即本地索引/本地源码，与第 1 条一致。
4. 遇到 API 不确定时：先在索引里确认存在与签名（`get_code_snippet` 读源码），**再**决定写法；不得凭记忆或在线资料猜测。

## 过程产物约定

过程产物（验证日志、一次性脚本、命令输出）落盘位置遵循 `.agents/AGENTS.md`：一律 `.rds/tmp/` 或 `tools/_archive/`，不入库、不留当前工作目录。

## 文档组织约定

- 仓库根目录不放置文档（README / LICENSE / AGENTS 等仓库元文件除外）；文档一律进 `docs/` 对应主题目录（architecture / migration / reports）。
- 竞品研究、竞品分析与对标建议类文档统一放 `docs/analysis/`（该目录已 gitignore，不入库，属过程性研究资料）。
- 临时产物（日志、截图、一次性脚本、嵌套仓库）不进 `docs/`，一律走 `.rds/tmp/` 或 `tools/_archive/`。
