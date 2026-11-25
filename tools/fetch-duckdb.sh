#!/usr/bin/env bash
# 取 DuckDB 预编译库（动态链接用）——跑一次即可，库落在仓库内 third_party/duckdb/<版本>/。
#
# 设计 / 升级 / 排错：docs/architecture/dependencies/duckdb-linking.md
#   为什么不让构建期自动下：① 构建期联网不可控（本机 GitHub 直连不通，走镜像）；
#   ② `target/` 会被清理，而库不该跟着丢；③ 版本与 crate 版本必须成对，放仓库里看得见。
#
# 用法：
#   tools/fetch-duckdb.sh                 # 取默认版本（与 Cargo.toml 的 duckdb 版本对齐）
#   tools/fetch-duckdb.sh 1.5.6           # 显式给版本
#   GH_PROXY= tools/fetch-duckdb.sh       # 不走镜像（能直连 GitHub 时）
#
# 平台：按当前系统选资产（Windows amd64 / Linux amd64 / macOS universal）。
set -euo pipefail

# ⚠️ 变量后面**紧接中文（多字节字符）**时必须写成 `${VAR}`：macOS 的 `/bin/bash` 还是 3.2，
# 在 UTF-8 语言环境下会把多字节字节吞进变量名，报 `ASSET: unbound variable` 这种莫名其妙的错；
# Windows / Linux 的 bash 5.x 不受影响（CI 的 macOS 作业钤过这个坑，见 release-pipeline.md §7）。

VERSION="${1:-2.0.1}"
# 镜像前缀：本机实测 GitHub 直连不通，镜像可达；能直连时用 GH_PROXY= 关掉
GH_PROXY="${GH_PROXY-https://ghproxy.net/}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/third_party/duckdb/$VERSION"

# 资产命名分界线：**2.0 起**发布资产改成 `*-shared-libs-*`（实测 v2.0.1 的
# `duckdb-shared-libs-windows-amd64.tar.gz` 同时含 dll / lib / h 三类，19.2MB）；
# 1.5.x 及以前是 `libduckdb-<平台>.zip`（含 dll / lib / h）。
MAJOR="${VERSION%%.*}"
if [ "$MAJOR" -ge 2 ] 2>/dev/null; then
  NEW_ASSETS=1
else
  NEW_ASSETS=0
fi

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*)
    if [ "$NEW_ASSETS" = "1" ]; then ASSET="duckdb-shared-libs-windows-amd64.tar.gz"; else ASSET="libduckdb-windows-amd64.zip"; fi
    LIB="duckdb.dll" ;;
  Linux)
    if [ "$NEW_ASSETS" = "1" ]; then ASSET="duckdb-shared-libs-linux-amd64.tar.gz"; else ASSET="libduckdb-linux-amd64.zip"; fi
    LIB="libduckdb.so" ;;
  Darwin)
    if [ "$NEW_ASSETS" = "1" ]; then ASSET="duckdb-shared-libs-osx-universal.tar.gz"; else ASSET="libduckdb-osx-universal.zip"; fi
    LIB="libduckdb.dylib" ;;
  *) echo "不认识的平台：$(uname -s)；请手工取 2.0 的 duckdb-shared-libs-<平台> 放进 $DEST" >&2; exit 1 ;;
esac

URL="${GH_PROXY}https://github.com/duckdb/duckdb/releases/download/v${VERSION}/${ASSET}"
mkdir -p "$DEST"

if [ -f "$DEST/$LIB" ] && [ "${FORCE:-0}" != "1" ]; then
  echo "已在位：$DEST/${LIB}（要重取加 FORCE=1）"
  exit 0
fi

echo "下载 ${ASSET}（DuckDB v${VERSION}）"
echo "  $URL"
curl -fL --retry 3 --max-time 900 -o "$DEST/$ASSET" "$URL"

echo "解压到 $DEST"
case "$ASSET" in
  *.tar.gz) tar -xzf "$DEST/$ASSET" -C "$DEST" ;;
  *.zip)    unzip -o -q "$DEST/$ASSET" -d "$DEST" ;;
  *) echo "不认识的资产类型：$ASSET" >&2; exit 1 ;;
esac

# 头文件（duckdb.h）必须和库同目录：libduckdb-sys 从 DUCKDB_LIB_DIR 里找它
[ -f "$DEST/$LIB" ] || { echo "解压后没看到 ${LIB}，资产可能变了" >&2; exit 1; }
[ -f "$DEST/duckdb.h" ] || { echo "解压后没看到 duckdb.h（bindings 需要）" >&2; exit 1; }

echo "就位：$DEST"
echo "crate 版本必须成对：Cargo.toml 的 duckdb = \"1.10<每段两位>\" ↔ 这里的 v$VERSION"
echo "若换了版本，记得同步 .cargo/config.toml 的 DUCKDB_LIB_DIR 路径。"
