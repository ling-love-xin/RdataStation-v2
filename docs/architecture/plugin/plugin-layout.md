# 插件目录布局约定

> 状态：2026-10-16 定稿并已接入代码（`crates/plugin/src/layout.rs`）。
> 本文是「一个插件一个目录」的**唯一**目录口径；`plugin-dev-plan.md` §4.3 / §4.7
> 与 `plugin-user-guide.md` §2.1 的示例都以本文为准（后者的清单键名已于
> 2026-11-07 按 `manifest.rs` 重写，见文末 §9）。

## 1. 为什么要有这份约定

插件系统从第一天就是「目录即插件」：`scan_and_diff_plugins` 只认
`<安装目录>/<id>/plugin.toml`（没有 `plugin.toml` 的目录跳过）。但**目录里面怎么摆**
一直没有写死，于是出现了三种互不兼容的形态：

| 示例包 | 入口位置 | 依赖位置 |
| --- | --- | --- |
| `tools/plugin-drivers/oracle-jdbc` | `lib/rds-jdbc-shell.jar` | `lib/*.jar` |
| `tools/plugin-drivers/postgres-py` | 包根 `rds_dbapi_shell.py` | 无 |
| wasm 渲染（`tools/plugin-render-mermaid`） | 包根 `render.wasm` | 无 |

代码只靠「清单给相对路径」工作，所以三种都能跑；但对作者与安装器来说，
入口在根 / `lib/` / 子目录全靠猜，打包时还会把 `src/`、`target/` 一起塞进安装目录。
这份约定把位置收敛成一套可校验的默认值。

## 2. 三个外层目录（已由 `crates/paths` 定死，不再设计）

| 目录 | 内容 | 生命周期 |
| --- | --- | --- |
| `<RDS_HOME>/plugins/<id>/` | 插件本体，**只读安装产物** | 卸载删目录 |
| `plugin-data/<id>/` | 插件持久数据（可能含连接/凭据） | 卸载**保留** |
| `plugin-cache/<id>/` | 可清缓存（wasmtime 编译缓存 / sidecar 日志） | 随时可删 |
| `tmp/sidecar/<id>/` | 子进程 `current_dir` | 进程退出即失效 |

`plugins/.registry` 以点开头，扫描天然跳过。

## 3. 插件目录内部布局

```
<RDS_HOME>/plugins/<id>/
├── plugin.toml                  # 唯一契约，必须在根
├── README.md                    # 给人看的说明（可选）
├── LICENSE                      # 可选
├── bin/                         # 进程轨入口（sidecar / script）
│   ├── win-x64/agent.exe        # 多平台：按平台分目录
│   ├── linux-x64/agent
│   ├── darwin-arm64/agent
│   └── agent.py                 # 单文件 / 跨平台时直接放 bin/
├── lib/                         # 进程轨依赖：jar / dll / so / 私有包
├── wasm/                        # wasm 轨入口（纯计算 / 导出）
│   └── render.wasm
├── dist/                        # 前端轨入口（QuickJS，无 DOM）
│   └── extension.js
├── schemas/                     # 声明式资产（connection_schema 等）
├── media/                       # 展示资产：icon.svg / 截图 / 示例
└── checksums.json               # 随包完整性（sha256，可选；格式见下）
```

`checksums.json` 的形态与校验时机：

```json
{
  "algorithm": "sha256",
  "files": {
    "plugin.toml": "<64 位十六进制>",
    "bin/agent": "<64 位十六进制>"
  }
}
```

- **时机**：安装期（`installer::validate_plugin` → `crates/plugin/src/checksums.rs::verify_package`），
  对解压产物 / 源目录校验，**复制目录之前**；装进去的字节和校验过的是同一份。
- **覆盖率**（防"删一行绕过"）：`plugin.toml`、清单声明的入口（多平台包按**解析后**的
  `bin/<platform>/agent` 算）、声明了且真实存在的 `plugin.icon` / `connection_schema`
  必须列出；其余依赖（`lib/*.jar` 等）建议全列。
- 键与清单同一套相对路径白名单（POSIX `/`、禁 `..`/盘符/绝对路径），且不许把
  `checksums.json` 自己列进去（哈希无法自指）。
- 算法目前只认 `sha256`；`signature.json`（验"谁签发"）属 P5，未实现。
- 列在开发残留目录（`target/`、`.git` 等，见 §5 规则 4）里的文件会被拒绝——安装时不会复制，
  列进校验只会给完整性一个假承诺。
- **缺失放行**：老包与开发目录（热插拔直接指源码目录）不带该文件，安装照常成功；
  发布包应在打包流程里生成——不带它等于放弃完整性校验。

各目录的职责边界：

- **`bin/`**：进程轨**可执行入口**。原生二进制按平台分目录（见 §4）；脚本（`.py` / `.js`）
  直接放这里即可，不要混进 `lib/`。
- **`lib/`**：进程轨**依赖**，不是入口。Java 的 jar（壳 + 驱动）、原生 `dll`/`so`、
  私有 Python 包都放这里。已有示例 `oracle-jdbc` 的 `lib/*.jar` 保持不动。
- **`wasm/`**：wasm 轨模块。一个包可以有多个 `.wasm`，由 `capabilities.wasm.entry` 指向。
- **`dist/`**：前端轨模块（VS Code 习惯命名）。带 webview 资源时同级放 `webview/` 之类子目录。
- **`schemas/`**：声明式资产，比如驱动的 `connection_schema`。
- **`media/`**：图标、截图、示例图；不参与执行，缺失不阻断安装。
- 根目录**只放契约与说明**（`plugin.toml` / README / LICENSE），运行产物一律进子目录。

## 4. 入口解析规则（清单指向，不靠位置猜）

入口位置一律由清单声明，宿主不在固定路径上"碰运气"：

| 轨 | 清单字段 | 约定位置 |
| --- | --- | --- |
| 进程（sidecar / script） | `backend.executable` | `bin/agent` / `bin/agent.py` |
| wasm | `capabilities.wasm.entry` | `wasm/render.wasm` |
| 前端 | `capabilities.frontend.entry` | `dist/extension.js` |

**多平台二进制**：`platforms` 是矩阵、`executable` 是单值，所以用目录表达平台差异：

1. `bin/<platform>/agent` 优先（`<platform>` 取值 `win-x64` / `darwin-arm64` / `linux-x64`，
   与清单 `platforms` 同构）；
2. 回退 `bin/agent`（单文件 / 跨平台包）；
3. Windows 上若清单没写后缀且 `bin/agent.exe` 存在，补 `.exe`（老清单行为不变）。

清单**不需要**为平台分叉，也不需要新增字段。解析实现见
`crates/plugin/src/layout.rs::resolve_executable`。

## 5. 硬规则（清单与包必须同时满足）

1. **目录名 = `plugin.plugin.id`**。扫描按目录名建 key、加载与卸载按清单 id 建 key；
   不一致会出现「加载成功但热插拔 diff 认不出、卸载删不到目录」。加载入口会直接拒绝
   （`layout::validate_dir_name`）。
2. **清单里所有"指文件"的字段**必须：相对插件目录、POSIX `/`、不含 `..` / 盘符 /
   绝对路径，且解析后仍在插件目录内。当前覆盖：
   `backend.executable` / `capabilities.wasm.entry` / `capabilities.frontend.entry` /
   `plugin.icon` / `contributes.drivers[].connection_schema`（`PluginManifest::validate`
   统一过闸，实现 `layout::validate_relative_path`）。
3. **入口必须真实存在**。安装期检查（`layout::check_declared_entries`）：
   声明的入口缺失就拒绝安装并逐个点名，而不是「装成功、激活时才报错」。
   图标 / schema 这类展示与声明式资产只校验路径形态，缺失不阻断安装。
4. **包里只放运行产物**。安装器整目录复制，但跳过开发残留——目录
   `.git` / `.svn` / `.hg` / `.rds` / `.idea` / `.vscode` / `target` / `node_modules` /
   `__pycache__` / `.pytest_cache` / `.mypy_cache`，文件 `.DS_Store` / `Thumbs.db`
   （`layout::DEV_RESIDUE_DIRS` / `is_dev_residue`）。`src/`、`lib/` 属于包内容，照常安装
   （脚本类插件的入口就可能写在 `src/` 里）。清单声明的入口若落在被跳过的目录
   （如 `executable = "target/agent"`），安装期直接拒绝，不会「校验通过却被复制丢掉」。
5. **压缩包根 = `plugin.toml` 所在层**。zip / tar.gz 里不要再套一层 `<id>/`：
   解压根找不到 `plugin.toml` 会直接判为「不是插件包」。
6. **运行期不写安装目录**。安装目录是只读产物：写进去会参与热插拔 hash 判定之外的变化，
   卸载时一起被删。持久数据走 `plugin-data/<id>`，缓存走 `plugin-cache/<id>`，
   子进程工作目录走 `tmp/sidecar/<id>`。
7. **发布包带 `checksums.json`**。安装期逐文件校验 sha256 并检查覆盖率（见 §3 末）；
   缺失不阻断安装（兼容老包与开发目录），但发布包不带它就没有"字节是否仍是作者签发的"这一层。

## 6. 版本不进目录名

目录按 `id` 唯一（`plugins/<id>/`），版本只存在清单与 DB：

- 卸载、热插拔 diff、DB `install_path` 全按 id；
- 更新语义 = 清单 hash 变化 → 置 `needs_reload` → `reload_plugins` 生效（不热更运行中插件）；
- 若要 VS Code 式 `<id>-<version>/`，需要同时改卸载路径、diff key、DB 安装路径，
  并新增旧版本清理，代价大于收益。

更新时的写入顺序：**先停进程**（`deactivate_plugin` 的 Sidecar 分支会先停），
再整体替换目录；不要在运行中覆盖正在被 `current_dir` 占用的文件（Windows 上文件锁会失败）。

## 7. 校验矩阵（当前代码落点）

| 检查 | 时机 | 实现 |
| --- | --- | --- |
| 相对路径形态（五字段） | 解析清单时 | `manifest::PluginManifest::validate_asset_paths` |
| 目录名 = id | 加载时 | `manager::load_plugin_from_dir` → `layout::validate_dir_name` |
| 入口存在性 | 安装时（复制前，对解压产物） | `installer::validate_plugin` → `layout::check_declared_entries` |
| 随包哈希 + 覆盖率 | 安装时（复制前） | `installer::validate_plugin` → `checksums::verify_package` |
| 开发残留不落盘 | 安装时（复制） | `installer::copy_dir` → `layout::is_dev_residue`；入口落其中则 `validate_plugin` 拒绝 |
| wasm 入口解析 | 激活时 | `manager::activate_wasm` → `layout::resolve_asset` |
| 多平台可执行解析 | 部署时 | `manifest::PluginBackend::resolve_executable` → `layout::resolve_executable` |

**未覆盖（诚实列出）**：

- `contributes.commands[].icon` / `contributes.panels[].icon` 语义未定（可能是宿主内置图标名），
  本轮不校验、不解析；定案前按字符串原样传递。
- `plugin.icon` 目前**没有消费方**：驱动描述符的 `icon` 是字符徽标（内置族是 emoji），
  塞插件资源路径只会显示文件名字面量，所以 `sidecar::factory` 已停止传递（2026-10-16）。
  插件列表/详情 UI（P4）要展示插件图标时，再补宿主侧 icon resolver。
- 路径校验是**逐段白名单**（拒绝 `..` / 盘符 / 根），不做 symlink 解析后的二次确认；
  插件包里的符号链接按平台语义处理（P5 随包签名时再收紧）。
- `checksums.json` **只在安装期**校验：运行期与热插拔（开发者目录）不校验，
  `signature.json`（签名与签发者信任链）未实现。安装目录的只读性靠文件系统权限与卸载流程
  保证，不靠每次启动重算哈希。

## 8. 既有示例包的对齐建议

| 包 | 现状 | 建议 |
| --- | --- | --- |
| `tools/plugin-drivers/oracle-jdbc` | 入口 `lib/rds-jdbc-shell.jar` | 壳是"可执行入口"、其余 jar 是依赖；迁到 `bin/rds-jdbc-shell.jar` 更贴约定，但**不必现在改**（清单为准，代码已兼容） |
| `tools/plugin-drivers/postgres-py` | 入口 `rds_dbapi_shell.py`（包根） | 新包按 `bin/` 摆；老包不动 |
| wasm 渲染 | 入口 `render.wasm`（包根） | 打包时放进 `wasm/render.wasm` 并改 `entry` |

约定是**新包的默认值**，不是对存量包的强制迁移：入口位置由清单指向，代码两边都认。

## 9. 原型稿与现行清单的差异（已归档）

早期 `plugin-user-guide.md` §2.1 的最小插件用 `schema_version` + `[extension]` + `[main]` 与
`[capabilities]` 权限表；现行 `crates/plugin/src/manifest.rs` 里 `schema_version` 零命中，
实际是 `[plugin]` + `[backend]` + `[permissions]` + `[capabilities.frontend/wasm/driver]`，
不存在 `[extension]` / `[main]`。2026-11-07 已把该节（连同 §2.2 字段表、§2.4 权限口径、
§1.1–§1.3 安装/启停/卸载流程）按代码重写。

**仍然要记住的坑**：`PluginManifest` 及其子结构没有 `deny_unknown_fields`，旧原型键写进清单
会**解析通过但不生效**（例：`contributes.views` / `insightRules` / `mockGenerators` /
`exporters` 没有对应结构体）。不要照抄旧样例；字段以 §2.2 表为准。
