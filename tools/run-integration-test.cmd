@echo off
rem ============================================================
rem  RdataStation 全量集成测试（cargo test --workspace）
rem  可复用工具：本机 Rust 工具链不在 PATH，需绕 rustup shim。
rem  用法：cmd /c tools\run-integration-test.cmd
rem  日志：<仓库根>\.rds\tmp\rds-integration-<日期>.log（gitignore）
rem ============================================================
setlocal
set "ROOT=%~dp0.."
for /f %%d in ('powershell -NoProfile -Command "Get-Date -Format yyyy-MM-dd"') do set "DATE=%%d"

rem 本机 Rust 工具链（非 shim）路径；换机器需改这里
set "RUSTC=D:/Dev/Rust/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustc.exe"
set "RUSTDOC=D:/Dev/Rust/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustdoc.exe"

"D:/Dev/Rust/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/cargo.exe" test --workspace -j 2 --no-fail-fast > "%ROOT%\.rds\tmp\rds-integration-%DATE%.log" 2>&1
set "EXITCODE=%ERRORLEVEL%"
echo EXIT_CODE=%EXITCODE%
exit /b %EXITCODE%
