# 发布流水线：打标签 → 云端多平台构建 → Release Assets

> 状态：**已落地（2026-09-21）** · 触发方式 `git push origin v*` · 关联：`runtime/data-paths.md`（资源与数据落点）、`dependencies/duckdb-linking.md`（动态链接的由来）、`dependencies/dependency-strategy.md`（`--locked` 的底气 = 版本唯一入口）

## 1. 一句话

**本地不打包、不上传**：推一个 `v*` 标签，GitHub Actions 在四套 runner 上各编一次 release，
把「可分发目录」压成归档 + 校验和，直接挂到该标签的 Release Assets 下。

```
git tag v0.1.0 && git push origin v0.1.0
        │
        ├─ Windows x86_64 ─┐
        ├─ Linux  x86_64  ─┼─ 4 个 job 并行：取 DuckDB 库 → cargo build --release → 打包 → 传 Assets
        ├─ macOS  arm64   ─┤   （任一平台失败不影响其余：fail-fast: false）
        └─ macOS  x86_64  ─┘
```

## 2. 触发与产物

| 触发 | 行为 |
| --- | --- |
| `push` 标签 `v*` | 建/追加该标签的 Release，产物进 **Release Assets** |
| Actions → Release → Run workflow | 只出**工作流附件**（`upload-artifact`），**不建 Release**——用来试跑矩阵，不污染版本页 |

| 平台 | runner / target | 产物 | 归档内容 |
| --- | --- | --- | --- |
| Windows x86_64 | `windows-latest` / `x86_64-pc-windows-msvc` | `rds-app-<版本>-windows-x86_64.zip` | exe + `duckdb.dll` + `assets/` + 许可 |
| Linux x86_64 | `ubuntu-22.04` / `x86_64-unknown-linux-gnu` | `rds-app-<版本>-linux-x86_64.tar.gz` | 可执行文件 + `libduckdb.so` + `assets/` + 许可 |
| macOS arm64 | `macos-14` / `aarch64-apple-darwin` | `rds-app-<版本>-macos-arm64.tar.gz` | `RdataStation.app`（内含可执行文件 + `libduckdb.dylib` + `assets/`） |
| macOS x86_64 | `macos-14` / `x86_64-apple-darwin` | `rds-app-<版本>-macos-x86_64.tar.gz` | 同上（在 arm64 runner 上交叉编译） |

每个归档另有一份 `<归档名>.sha256` 一起上传。

**每个归档里必须有的三样**（`tools/package-release.sh` 自检，缺一样就红掉那一步）：
可执行文件、DuckDB 动态库、`assets/themes/product-tokens.json`。

## 3. 包内结构与「谁会去读它」

| 文件 | 读它的人 | 备注 |
| --- | --- | --- |
| `rds-app(.exe)` | — | Windows 下应用图标在编译期嵌进 exe（`crates/app/build.rs`） |
| `duckdb.dll` / `libduckdb.so` / `libduckdb.dylib` | 动态加载器 | Windows 按「exe 目录」搜索；Linux / macOS 靠**构建期写进二进制的 RPATH**（`$ORIGIN` / `@executable_path`） |
| `assets/themes/`、`assets/icons/` | `paths::assets_dir()` | 与可执行文件**同级**；主题与产品语义 token 在里面，缺了会静默回落 gpui-kit 默认配色 |
| `LICENSE` / `NOTICE` / `README.md` | 人 | 分发义务与上手说明 |
| `RdataStation.app/Contents/Resources/icon.icns`（仅 macOS） | Finder / Dock | 图标；`Info.plist` 由打包脚本生成 |

**不带**：`assets/public/`（README 的截图与品牌素材，运行时不读）、`v1/`（历史参考）、迁移 SQL（已 `include_dir!` 编进二进制）。

数据落点不变（见 `runtime/data-paths.md`）：软件生成的一切默认落在**可执行文件所在目录**；
该目录不可写（如装在 `/Applications`、`Program Files`）时回退平台本地数据目录。

## 4. 维护者操作步骤

```bash
# 1. 版本号对齐（不一致只会 warning，但产物按 tag 命名，别让它长期漂）
#    改根 Cargo.toml 的 [workspace.package] version
# 2. 提交
git commit -am "Bump version to 0.1.1"
# 3. 打标签推送（这一步就是发布动作）
git tag v0.1.1
git push origin main --tags
```

发布后校对三件事：Actions 四个 job 全绿 → Release 页有 8 个资产（4 归档 + 4 校验和）→
下载 Windows 包解压直接跑（不改 PATH、不设环境变量）。

## 5. 平台细节与已知限制

| 平台 | 细节 | 限制（有意为之 / 后续） |
| --- | --- | --- |
| Windows | MSVC 工具链 + Windows SDK（runner 自带，`winresource` 嵌图标要 `rc.exe`）；库由加载器按 exe 目录找到 | **未签名**：首次运行会有 SmartScreen 提示（「仍要运行」可过）；签名要代码签名证书 |
| Linux | 钉 `ubuntu-22.04`（glibc 2.35）作兼容下限；`RUSTFLAGS` 写入 `RPATH=$ORIGIN`；依赖 wayland / xkbcommon / xcb / fontconfig / libclang / OpenSSL | 只有 tar.gz，没有 AppImage / deb / rpm；运行时仍需系统装了图形栈（桌面发行版自带） |
| macOS | 包成 `RdataStation.app`（裸二进制从访达启动会被当命令行脚本）；arm64 与 x86_64 各一份，不做 universal 合并 | **未签名未公证**：首次打开被 Gatekeeper 拦，见 §7；正式分发需 Apple 开发者账号 |
| 全平台 | `--locked`（用仓库里的 `Cargo.lock`，不重新解析版本）；`-j 2`（与 README 同口径，防 OOM） | 未在 CI 上跑测试；测试仍是本机 `cargo test-all`（见 §6） |

## 6. 设计取舍

| 决定 | 为什么 |
| --- | --- |
| 只认标签触发，不认分支 | 发布是**明确动作**；分支推送跑 4 平台全量 release 构建纯属浪费 |
| 打包逻辑放脚本（`tools/package-release.sh`），不放 YAML | CI 与本地跑同一份逻辑；本地想验一次包，直接 `bash tools/package-release.sh <版本> <triple>` |
| DuckDB 动态库随包发，不静态链接 | 沿用既有决策（`duckdb-linking.md`：内核编译数分钟 + 并发静态链接耗尽内存）；代价是包里多 36 MB |
| `paths::assets_dir()` 认「exe 同级 → 仓库」 | 云端构建时编译期路径指**构建机**的检出目录，用户机上不存在；只认它会让主题与图标静默丢失 |
| 矩阵各 job 各自上传 Assets（`softprops/action-gh-release`） | 免去「先等 Release 建好再传」的编排与竞态（第一次上传建 Release，其余追加） |
| 缓存只存 registry（`cache-targets: false`） | 本工作区一份 `target/` 数 GB，会挤爆仓库 10 GB 缓存预算，上传比省下的编译时间更贵 |
| 不做 universal macOS 二进制 | 两份独立产物更简单，用户按芯片选；`lipo` 合并需要同一 runner 上编两遍再合并 |
| 不在发布流水线里跑测试 | 4 个平台各跑一遍 2000+ 项测试成本翻倍且与产物无关；测试仍是本机 `cargo test-all`（口径见 `module-status.md`），PR 上的门禁只做编译（见 §9） |

## 7. 故障排查

| 症状 | 原因 | 处理 |
| --- | --- | --- |
| 启动后配色是 gpui-kit 默认（不是 RDS） | 包里少了 `assets/`，或它不在可执行文件同级 | 看 stderr 的 `[paths] 未找到随包资源目录`；用 `tools/package-release.sh` 重打包 |
| Linux：`error while loading shared libraries: libduckdb.so` | 二进制里没有 `$ORIGIN` RPATH（本地手工构建常见） | `LD_LIBRARY_PATH=. ./rds-app`，或用流水线同样的 `RUSTFLAGS` 重编 |
| 库没随包（或名字对不上） | 加载器找的是 **SONAME**，可能不是 `libduckdb.so` | 打包脚本已把 `libduckdb.*` 连同版本化文件名一起拷；用 `readelf -d` / `otool -L` 核对实际依赖名 |
| macOS：`Library not loaded: ...libduckdb.dylib` | 该 dylib 的 install name 是绝对路径（不是 `@rpath/…`） | `install_name_tool -change <原路径> @rpath/libduckdb.dylib RdataStation.app/Contents/MacOS/rds-app`，再重打 tar |
| macOS：双击提示「无法验证开发者」 | 未签名未公证 | `xattr -dr com.apple.quarantine RdataStation.app`，或右键 → 打开 |
| Windows：SmartScreen「Windows 已保护你的电脑」 | 未签名 | 「更多信息 → 仍要运行」；要根治得买证书做签名 |
| 构建报 `duckdb.h` / `-lduckdb` 找不到 | 取库步骤没跑，或 `.cargo/config.toml` 的 `DUCKDB_LIB_DIR` 指向的版本与 crate 不配对 | 跑 `tools/fetch-duckdb.sh`；版本映射见 `duckdb-linking.md` |
| Windows 构建报 `LNK1104: cannot open file '...\.rds\tmp\lnk{GUID}.tmp'` | `.cargo/config.toml` 把 `TEMP` 钉在仓库内 `.rds/tmp`，而全新检出里没这个目录（MSVC 链接器不自己建） | `mkdir -p .rds/tmp` 后重试；两个工作流里已有「预建 TEMP 目录」一步。**这是全新克隆就会踩的坑，不只是 CI** |
| Linux 编译报缺头文件 / `-lxxx` | 少系统包 | 按报错的库名补进 workflow 的 apt 列表（当前清单见 `release.yml`） |
| 构建被 OOM 杀掉 | 并发链接重型 crate | 保持 `-j 2`；不要为提高速度去掉它 |
| 手动 Run workflow 之后 Release 页没东西 | 手动触发**不建 Release**（设计如此） | 产物在该次运行的 Artifacts 里；要发布请推标签 |
| 版本号与标签不一致的 warning | `[workspace.package] version` 没跟着改 | 改版本号后重打标签 |

## 8. 明确不做 / 后续可加

- 代码签名与公证（Windows 证书 / Apple 开发者账号）：**要钱要账号**，与「个人开源项目」现状不符
- AppImage / deb / rpm / MSI 安装包：当前只发「解压即用」归档
- macOS universal 二进制、Linux arm64、Windows arm64
- 自动分类的 changelog（现在用 GitHub 自动生成的 release notes）
- **已完成**：`push` / PR 上的编译门禁（见 §9，之前列在「后续可加」里）

## 9. 检查工作流（PR 与 main 上的编译门禁）

`.github/workflows/ci.yml`：**在打标签之前**就发现「Windows 能编、Linux 编不过」这类问题，
而不是等发布流水线红了才知道。发布流水线只负责构建与打包，不重复跑检查。

| 项 | 取值 | 为什么 |
| --- | --- | --- |
| 触发 | `pull_request` · `push` 到 `main` · 手动 | PR 上频繁推送，`concurrency` 会把同一分支的旧一轮取消掉 |
| 矩阵 | `ubuntu-22.04` · `windows-latest` | 两端都要过：Linux 是发布目标之一，Windows 是开发基线 |
| 跑什么 | `cargo clippy-all -j 2` | 它已包含 `check-all` 的全量类型检查，只多一遍 lint |
| 缓存 | rust-cache **开** target（key `ci`）+ `third_party/duckdb` 按版本 | 与发布流水线相反：PR 反复跑同一份代码，增量命中收益最大；DuckDB 库按内核版本缓存，免去每次下载 |

**有意不做的三件事**：

| 不做 | 原因 | 想加的话 |
| --- | --- | --- |
| 测试 | GPUI 窗口测试要图形栈（Linux runner 无显示服务、Windows runner 只有软件适配器） | 先在 Linux 上加 `mesa-vulkan-drivers` + `xvfb-run`，再按模块分批接；权威口径仍是本机 `cargo test-all`（`module-status.md`） |
| `cargo fmt --check` | 存量文件有 rustfmt 漂移（本仓约定只格式化本轮 hunk），一刀切会全红 | 只对改动文件格式化：`git diff --name-only origin/$BASE...HEAD -- '*.rs'` 再逐个 `rustfmt --check` |
| 告警硬门禁 | 存量告警未清零时 `-- -D warnings` 会直接红掉 | 清零后在 clippy 步骤加 `-- -D warnings`（一行） |

Linux 系统依赖只有**一份清单**（`tools/install-linux-deps.sh`），CI 与 Release 两个工作流共用，
补包只改那一处。

## 10. 实现位置映射表

| 设计决策 | 实现位置 |
| --- | --- |
| 标签触发 + 矩阵构建 + 传 Assets | `.github/workflows/release.yml` |
| PR / main 编译门禁 | `.github/workflows/ci.yml` |
| Linux 系统依赖（两份工作流共用） | `tools/install-linux-deps.sh` |
| 打包（可分发目录 / 归档 / 自检 / 校验和） | `tools/package-release.sh` |
| Windows 归档（无 `zip` 时用 .NET 写正斜杠条目名） | `tools/zip-dir.ps1` |
| DuckDB 预编译库获取 | `tools/fetch-duckdb.sh`（CI 以 `GH_PROXY=` 直连 GitHub） |
| 随包资源目录解析 | `crates/paths/src/lib.rs`（`assets_dir`） |
| 主题目录消费 | `crates/app/src/main.rs`（`paths::assets_dir()/themes`） |
| 标题栏图标消费 | `crates/workbench/src/view.rs`（`paths::assets_dir()/icons/32x32.png`） |
| Windows 图标嵌入 exe | `crates/app/build.rs`（`winresource`） |
| Windows 开发期拷贝 `duckdb.dll` | `crates/engine/build.rs` |
| 打包产物不入库 | `.gitignore`（`/dist/`） |
