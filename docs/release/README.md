# 发布说明（Release Notes）

GitHub Releases 页面上的正文由仓库**手写**，不用「自动生成的 compare 链接」。
原因：本仓库是单人直推 `main`，GitHub 的自动生成器只汇总 PR 与 issue，这里没有 PR，
生成出来的正文只有一个 `**Full Changelog**` 链接——用户点开看不到任何「这次更新了什么」。

## 位置与命名

```
docs/release/notes/<标签名>.md      例：docs/release/notes/RDS-beta-0.2.8.md
```

文件名 = **标签名原文**（`RDS-beta-0.2.8.md`、`v0.2.9.md`），发布流水线按
`docs/release/notes/${GITHUB_REF_NAME}.md` 取用；找不到就回落到 GitHub 自动生成，
并在 Actions 里留一条 warning。

## 写作口径

读者是**用这个软件的人**，不是看 diff 的人。

- 一条一句话，以「用户能感觉到什么」开头；不复述提交标题、不写文件路径与函数名。
- 只写**这一版相对上一版**的变化；跨版本的功能总览放 README，不在这里重复。
- `docs` / `test` / `chore` / `refactor` 类改动默认不进正文，除非它改变了用户可见行为
  （例：重构顺手修掉一个卡顿，那要写「更流畅」而不是「拆分模块」）。
- 数量级要说清（「支持多列分组」不行，得说「按任意多列分组，并可只看重复行」）。
- 破坏性变更、需要用户动手的事（改设置、重装插件、重新登录）单独列一节，并给操作路径。
- 未验证的东西不写。发布说明里的每条断言，都要能在提交或产物里找到依据。

## 固定骨架

```markdown
# <产品名> <版本>

> <日期> · 相对 <上一标签> 的 N 个提交 · <beta / 定版说明>

## 本次重点        ← 3~5 条，最值得说的
## <功能分区>       ← 按产品区域分（编辑器、结果集、导航、洞察、插件…），按需要出现
## 依赖与构建       ← 内核 / 依赖大版本换代时才有
## 下载与校验       ← 文件表 + 校验命令 + 未签名提示
## 已知问题         ← 这一版明确没做的、需要用户绕的

**完整对比**：<compare 链接>
```

`本次重点` 与 `已知问题` 必写；分区按本版实际改动挑，没有改动的分区整节不出现。

## 草稿脚手架

```powershell
# 从上一个标签到 HEAD 的提交，按中文前缀分组成草稿（放进 .rds/tmp，人工改写后再落 docs/release/notes/）
pwsh tools/release-notes.ps1 -From RDS-beta-0.2.7 -Head HEAD
```

脚本只做分组与骨架，**不直接发布**：截图里那种「Added Group by Source App: …」句子是人改出来的，
机器给的只是素材。改完把正文放进 `docs/release/notes/<标签名>.md` 并提交，再打标签。

## 与流水线的关系

| 环节 | 行为 |
| --- | --- |
| `preflight` job | 校验「标签 ↔ `Cargo.toml` 版本」一致（不一致直接红），并跑 `rds-ui-guard` |
| `build` job | 找不到 notes 文件时 warning + 回落自动生成；找到就用 `body_path` 上传 |
| `RDS-beta-*` 标签 | Release 标成 **prerelease**，不抢 Latest |
| Release 正文 | 手写优先；末尾保留 `**完整对比**` 链接，方便想看 diff 的人 |

历史版本的说明是**补写**的（0.2.8 及更早的 Release 正文只有 compare 链接），补写时把文件放进
`docs/release/notes/` 并用 `gh release edit` 同步到页面；不再回头改已经发布的产物。
