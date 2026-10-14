@echo off
rem 重建 rds-app（含 LSP 模块）供 UI 手动测试
setlocal
set "ROOT=%~dp0.."
set "RUSTC=D:/Dev/Rust/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustc.exe"
set "RUSTDOC=D:/Dev/Rust/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustdoc.exe"
"D:/Dev/Rust/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/cargo.exe" build -p rds-app -j 2 > "%ROOT%\.rds\tmp\rds-build-app-2-2026-10-14.log" 2>&1
set "EXITCODE=%ERRORLEVEL%"
echo BUILD_EXIT_CODE=%EXITCODE% >> "%ROOT%\.rds\tmp\rds-build-app-2-2026-10-14.log"
exit /b %EXITCODE%
