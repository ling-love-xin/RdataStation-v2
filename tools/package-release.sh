#!/usr/bin/env bash
# 打「可分发目录 + 归档」——CI 与本地共用同一份逻辑（发布不靠手工复制文件）。
#
# 设计 / 排错：docs/architecture/release/release-pipeline.md
#
# 用法：
#   bash tools/package-release.sh <版本号> [<target-triple>]
#   bash tools/package-release.sh 0.1.0 x86_64-pc-windows-msvc
#   bash tools/package-release.sh 0.1.0                      # 用本机默认 target
#
# 前置（脚本会校验，缺什么说什么）：
#   1. 已构建：cargo build --release --locked -p rds-app -j 2 --target <triple>
#   2. DuckDB 动态库在位：bash tools/fetch-duckdb.sh
#
# 产物（都在 dist/ 下，已 gitignore）：
#   dist/rds-app-<版本>-<平台>/            可分发目录：可执行文件 + DuckDB 动态库 + assets/
#   dist/rds-app-<版本>-<平台>.zip|.tar.gz  上传到 Release Assets 的那一个文件
#
# 三处约定（与运行时路径设计对齐，见 docs/architecture/runtime/data-paths.md）：
#   - **DuckDB 动态库放在可执行文件同级**：Windows 靠加载器搜索 exe 目录命中；
#     Linux / macOS 靠构建期写进二进制的 RPATH（`$ORIGIN` / `@executable_path`）；
#   - **`assets/` 放在可执行文件同级**：`paths::assets_dir()` 优先认这一份
#     （主题与产品语义 token 在里面，缺了会静默回落 gpui-kit 默认配色）；
#   - 软件生成的数据仍落在**可执行文件所在目录**（`RDS_HOME`，见 paths crate），
#     所以发布包解压到可写目录即可，不必额外安装。
#
# macOS 额外包成 `RdataStation.app`：裸二进制从访达启动会被当成命令行脚本（没有图标、
# 不会进 Dock），包一层才像个应用；`.app` 内的可执行文件仍与 `assets/` 同级。
set -euo pipefail

# ⚠️ 变量后面**紧接中文（多字节字符）**时必须写成 `${VAR}`：macOS 的 `/bin/bash` 还是 3.2，
# 在 UTF-8 语言环境下会把多字节字节吞进变量名并报 `unbound variable`；bash 5.x 不受影响。
# 本脚本由 CI 的 macOS 作业执行，所以这条不是风格问题（见 release-pipeline.md §7）。

VERSION="${1:-}"
TARGET="${2:-}"
if [ -z "$VERSION" ]; then
  echo "用法：bash tools/package-release.sh <版本号> [<target-triple>]" >&2
  exit 2
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STAGING_NAME="rds-app-$VERSION"

# ==================== 平台判定 ====================
# 给了 target 就按 target 判（CI 走这条，交叉编译也准确）；没给就看本机。
case "${TARGET:-$(uname -s)}" in
  *windows*)                  OS=windows; PLATFORM="windows-x86_64"; EXE="rds-app.exe"; DYLIB="duckdb.dll";      ARCHIVE="zip" ;;
  aarch64-apple-darwin)       OS=macos;   PLATFORM="macos-arm64";  EXE="rds-app";     DYLIB="libduckdb.dylib"; ARCHIVE="tar.gz" ;;
  *apple-darwin*)             OS=macos;   PLATFORM="macos-x86_64"; EXE="rds-app";     DYLIB="libduckdb.dylib"; ARCHIVE="tar.gz" ;;
  *linux*)                    OS=linux;   PLATFORM="linux-x86_64"; EXE="rds-app";     DYLIB="libduckdb.so";   ARCHIVE="tar.gz" ;;
  MINGW*|MSYS*|CYGWIN*)       OS=windows; PLATFORM="windows-x86_64"; EXE="rds-app.exe"; DYLIB="duckdb.dll"; ARCHIVE="zip" ;;
  Darwin)                     OS=macos;   PLATFORM="macos-$(uname -m)"; EXE="rds-app"; DYLIB="libduckdb.dylib"; ARCHIVE="tar.gz" ;;
  *)                          OS=linux;   PLATFORM="linux-$(uname -m)"; EXE="rds-app"; DYLIB="libduckdb.so"; ARCHIVE="tar.gz" ;;
esac
STAGING_NAME="$STAGING_NAME-$PLATFORM"

# ==================== 输入校验 ====================
PROFILE_DIR="$ROOT/target${TARGET:+/$TARGET}/release"
BIN="$PROFILE_DIR/$EXE"
if [ ! -f "$BIN" ]; then
  echo "找不到构建产物：$BIN" >&2
  echo "先构建：cargo build --release --locked -p rds-app -j 2${TARGET:+ --target $TARGET}" >&2
  exit 1
fi

# DuckDB 库版本从 .cargo/config.toml 的 DUCKDB_LIB_DIR 反查——**单一来源**，
# 避免升级内核时改了配置却漏改打包脚本（库与 crate 版本必须成对，见 duckdb-linking.md）。
DUCKDB_VERSION="$(grep -o 'third_party/duckdb/[0-9][0-9.]*' "$ROOT/.cargo/config.toml" | head -n1 | sed 's|third_party/duckdb/||')"
DUCKDB_DIR="$ROOT/third_party/duckdb/$DUCKDB_VERSION"
if [ ! -f "$DUCKDB_DIR/$DYLIB" ]; then
  echo "找不到 DuckDB 动态库：$DUCKDB_DIR/$DYLIB" >&2
  echo "先取库：bash tools/fetch-duckdb.sh $DUCKDB_VERSION" >&2
  exit 1
fi

ASSETS_DIR="$ROOT/assets"
for required in assets/themes/product-tokens.json assets/themes/rds-theme.json; do
  if [ ! -f "$ROOT/$required" ]; then
    echo "缺少随包资源：${required}（主题与产品 token 不在包里，发布版会回落默认配色）" >&2
    exit 1
  fi
done

# ==================== 组装可分发目录 ====================
DIST="$ROOT/dist"
STAGING="$DIST/$STAGING_NAME"
rm -rf "$STAGING"
mkdir -p "$STAGING"

# 随包资源：**只带运行时真的要读的两份**（`themes/` 主题与产品 token、`icons/` 应用图标）。
# `assets/public/` 是 README 的截图与品牌素材，运行时不读（见 docs/architecture/release/）。
copy_assets() {
  mkdir -p "$1/assets"
  cp -R "$ASSETS_DIR/themes" "$1/assets/themes"
  cp -R "$ASSETS_DIR/icons" "$1/assets/icons"
}

# 动态库：连同版本化文件名 / 链接一起拷。
# 加载器认的是 **SONAME**（可能是 `libduckdb.so.1.5.5` 而不是 `libduckdb.so`），只拷一个
# 会在用户机上表现成「找不到库」，而构建机上一切正常。
copy_dylib() {
  cp -R "$DUCKDB_DIR"/$DYLIB* "$1/"
}

# macOS：包成 .app，可执行文件在 Contents/MacOS/ 下（assets/ 与动态库跟着它同级）
if [ "$OS" = "macos" ]; then
  APP="$STAGING/RdataStation.app"
  BIN_DIR="$APP/Contents/MacOS"
  RES_DIR="$APP/Contents/Resources"
  mkdir -p "$BIN_DIR" "$RES_DIR"
  cp "$BIN" "$BIN_DIR/rds-app"
  chmod +x "$BIN_DIR/rds-app"
  copy_dylib "$BIN_DIR"
  copy_assets "$BIN_DIR"
  cp "$ASSETS_DIR/icons/icon.icns" "$RES_DIR/icon.icns"
  cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>RdataStation</string>
  <key>CFBundleDisplayName</key><string>RdataStation</string>
  <!-- 标识符若换发布主体，记得同步改这里（本地未签名构建没有公证，见发布文档 §故障排查） -->
  <key>CFBundleIdentifier</key><string>io.github.ling-love-xin.rdatastation</string>
  <key>CFBundleExecutable</key><string>rds-app</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
else
  cp "$BIN" "$STAGING/$EXE"
  copy_dylib "$STAGING"
  copy_assets "$STAGING"
fi

# 许可与说明随包走（GPL/第三方声明的分发义务）
for doc in LICENSE NOTICE README.md; do
  [ -f "$ROOT/$doc" ] && cp "$ROOT/$doc" "$STAGING/"
done

# ==================== 归档 ====================
ARCHIVE_FILE="$DIST/$STAGING_NAME.$ARCHIVE"
rm -f "$ARCHIVE_FILE"
if [ "$ARCHIVE" = "zip" ]; then
  if command -v zip >/dev/null 2>&1; then
    (cd "$DIST" && zip -qr "$STAGING_NAME.zip" "$STAGING_NAME")
  else
    # Git-Bash 常只有 unzip 没有 zip（本机实测如此）：交给 tools/zip-dir.ps1。
    # 不用 `Compress-Archive`：它写出的条目名用反斜杠分隔，在 Linux / macOS 上解出来是怪文件名。
    # ⚠️ `zip-dir.ps1` **必须带 UTF-8 BOM**：Windows PowerShell 5.1 对无 BOM 的文件按 ANSI 解码，
    # 脚本里的中文注释会把字符串字面量解坏（runner 上实测 ParserError，本机因代码页恰好能过）。
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$ROOT/tools/zip-dir.ps1" \
      -Src "$(cygpath -w "$STAGING")" -Dst "$(cygpath -w "$ARCHIVE_FILE")"
  fi
else
  tar -czf "$ARCHIVE_FILE" -C "$DIST" "$STAGING_NAME"
fi

# ==================== 自检 + 汇报 ====================
# 包里必须有这三样：可执行文件、DuckDB 动态库、主题资产。少任何一样都是坏包，
# 与其让用户在机器上发现，不如在这里就报错（CI 上直接红掉这一步）。
REQUIRED_ITEMS=("$EXE" "$DYLIB" "assets/themes/product-tokens.json")
for item in "${REQUIRED_ITEMS[@]}"; do
  if [ ! -e "$STAGING/$item" ]; then
    echo "可分发目录缺少 ${item}：$STAGING" >&2
    exit 1
  fi
done

# 归档再验一遍（tar 哪里都有；zip 只在有 unzip 的环境验，Windows 上未必装）
if [ "$ARCHIVE" = "zip" ]; then
  if command -v unzip >/dev/null 2>&1; then
    LIST="$(unzip -Z1 "$ARCHIVE_FILE")"
    for item in "${REQUIRED_ITEMS[@]}"; do
      printf '%s\n' "$LIST" | grep -q "$item$" || { echo "归档缺少 ${item}：$ARCHIVE_FILE" >&2; exit 1; }
    done
  fi
else
  LIST="$(tar -tzf "$ARCHIVE_FILE")"
  for item in "${REQUIRED_ITEMS[@]}"; do
    printf '%s\n' "$LIST" | grep -q "$item$" || { echo "归档缺少 ${item}：$ARCHIVE_FILE" >&2; exit 1; }
  done
fi

echo "已打包：${ARCHIVE_FILE}（$(du -h "$ARCHIVE_FILE" | cut -f1)）"

# 校验和：下载方一条命令就能核对完整性（与归档一起进 Release Assets）
CHECKSUM_FILE="$ARCHIVE_FILE.sha256"
if command -v sha256sum >/dev/null 2>&1; then
  (cd "$DIST" && sha256sum "$(basename "$ARCHIVE_FILE")" > "$(basename "$CHECKSUM_FILE")")
else
  (cd "$DIST" && shasum -a 256 "$(basename "$ARCHIVE_FILE")" > "$(basename "$CHECKSUM_FILE")")
fi
cat "$CHECKSUM_FILE"

echo "可分发目录：$STAGING"
if [ "$OS" = "linux" ] || [ "$OS" = "macos" ]; then
  echo "提示：动态库靠构建期 RPATH 命中（CI 用 RUSTFLAGS 写入）；本地手工构建若没加同样的"
  echo "      RUSTFLAGS，运行时需要 LD_LIBRARY_PATH（Linux）/ DYLD_LIBRARY_PATH（macOS）指向该目录。"
fi
