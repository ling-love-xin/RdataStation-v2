#!/usr/bin/env bash
# 装「编译本仓所需的 Linux 系统包」——CI 的两个工作流（CI / Release）共用这一份。
#
# 为什么是脚本而不是各写进 YAML：清单会随依赖变化（gpui 的 Linux 后端、bindgen、native-tls…），
# 两处各抄一份必然一处更新一处漏；补包也只改这一处。
#
# 用法（需要 sudo；CI runner 上是免密 sudo）：
#   bash tools/install-linux-deps.sh
#
# 包与理由（按依赖树推出来的，实测报错就按报错的库名补在后面）：
#   pkg-config · clang · libclang-dev      gpui-pre 的 bindgen（生成绑定要用 libclang）
#   libssl-dev                             native-tls → openssl-sys
#   libfontconfig1-dev · libfreetype6-dev  zed-font-kit（字体枚举与光栅化）
#   libwayland-dev                         gpui 的 Wayland 后端（wayland-sys）
#   libxkbcommon-dev · libxkbcommon-x11-dev  键盘映射（xkbcommon）
#   libxcb1-dev · -randr0-dev · -xkb-dev · -shm0-dev
#                                          X11 侧连接与窗口（as-raw-xcb-connection、x11rb）
#   libx11-dev · libx11-xcb-dev            X11 头文件（zed-scap / XIM）
#   libvulkan1 · libvulkan-dev             wgpu 的 Vulkan 后端（构建要 loader 头与库）
set -euo pipefail

if [ "$(uname -s)" != "Linux" ]; then
  echo "只用于 Linux（当前：$(uname -s)）；Windows / macOS 的系统依赖由工具链自带" >&2
  exit 1
fi

SUDO=""
if [ "$(id -u)" != "0" ]; then
  SUDO="sudo"
fi

PACKAGES=(
  pkg-config clang libclang-dev libssl-dev
  libfontconfig1-dev libfreetype6-dev
  libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev
  libxcb1-dev libxcb-randr0-dev libxcb-xkb-dev libxcb-shm0-dev
  libx11-dev libx11-xcb-dev
  libvulkan1 libvulkan-dev
)

# shellcheck disable=SC2086  # SUDO 为空时就是「不带前缀」，这里要的正是分词
$SUDO apt-get update
$SUDO apt-get install -y --no-install-recommends "${PACKAGES[@]}"

echo "Linux 系统依赖就位：${PACKAGES[*]}"
