#!/usr/bin/env bash
# 取 DuckDB 预编译库（动态链接用）——跑一次即可，库落在仓库内 third_party/duckdb/<版本>/。
#
# 设计 / 升级 / 排错：docs/architecture/dependencies/duckdb-linking.md
#   为什么不让构建期自动下：① 构建期联网不可控（本机 GitHub 直连不通，走镜像）；
#   ② `target/` 会被清理，而库不该跟着丢；③ 版本与 crate 版本必须成对，放仓库里看得见。
#
# 用法：
#   tools/fetch-duckdb.sh                 # 取默认版本（与 Cargo.toml 的 duckdb 版本对齐）
#   tools/fetch-duckdb.sh 1.5.5           # 显式给版本
#   GH_PROXY= tools/fetch-duckdb.sh       # 不走镜像（能直连 GitHub 时）
#
# 平台：按当前系统选资产（Windows amd64 / Linux amd64 / macOS universal）。
set -euo pipefail

VERSION="${1:-1.5.5}"
# 镜像前缀：本机实测 GitHub 直连不通，镜像可达；能直连时用 GH_PROXY= 关掉
GH_PROXY="${GH_PROXY-https://ghproxy.net/}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/third_party/duckdb/$VERSION"

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) ASSET="libduckdb-windows-amd64.zip"; LIB="duckdb.dll" ;;
  Linux)                ASSET="libduckdb-linux-amd64.zip";   LIB="libduckdb.so" ;;
  Darwin)               ASSET="libduckdb-osx-universal.zip"; LIB="libduckdb.dylib" ;;
  *) echo "不认识的平台：$(uname -s)；请手工取 $ASSET 放进 $DEST" >&2; exit 1 ;;
esac

URL="${GH_PROXY}https://github.com/duckdb/duckdb/releases/download/v${VERSION}/${ASSET}"
mkdir -p "$DEST"

if [ -f "$DEST/$LIB" ] && [ "${FORCE:-0}" != "1" ]; then
  echo "已在位：$DEST/$LIB（要重取加 FORCE=1）"
  exit 0
fi

echo "下载 $ASSET（DuckDB v$VERSION）"
echo "  $URL"
curl -fL --retry 3 --max-time 900 -o "$DEST/$ASSET" "$URL"

echo "解压到 $DEST"
unzip -o -q "$DEST/$ASSET" -d "$DEST"

# 头文件（duckdb.h）必须和库同目录：libduckdb-sys 从 DUCKDB_LIB_DIR 里找它
[ -f "$DEST/$LIB" ] || { echo "解压后没看到 $LIB，资产可能变了" >&2; exit 1; }
[ -f "$DEST/duckdb.h" ] || { echo "解压后没看到 duckdb.h（bindings 需要）" >&2; exit 1; }

echo "就位：$DEST"
echo "crate 版本必须成对：Cargo.toml 的 duckdb = \"1.10<每段两位>\" ↔ 这里的 v$VERSION"
echo "若换了版本，记得同步 .cargo/config.toml 的 DUCKDB_LIB_DIR 路径。"
