@echo off
rem ============================================================
rem  RdataStation 全量 check（cargo check --workspace --all-targets）
rem  可复用工具：零告警闸门。用法：cmd /c tools\run-check.cmd
rem  日志：<仓库根>\.rds\tmp\rds-check-<日期>.log（gitignore）
rem ============================================================
setlocal
set "ROOT=%~dp0.."
for /f %%d in ('powershell -NoProfile -Command "Get-Date -Format yyyy-MM-dd"') do set "DATE=%%d"

rem 本机 Rust 工具链（非 shim）路径；换机器需改这里
set "RUSTC=D:/Dev/Rust/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustc.exe"
set "RUSTDOC=D:/Dev/Rust/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustdoc.exe"

"D:/Dev/Rust/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/cargo.exe" check --workspace --all-targets -j 2 > "%ROOT%\.rds\tmp\rds-check-%DATE%.log" 2>&1
set "EXITCODE=%ERRORLEVEL%"
echo CHECK_EXIT_CODE=%EXITCODE%
exit /b %EXITCODE%
