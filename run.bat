@echo off
title Cache Advisor Launcher
echo ===================================================
echo               CACHE ADVISOR LAUNCHER
echo ===================================================
echo.
echo Setting up environment variables...
set LIBCLANG_PATH=D:\LLVM19\bin
echo.
echo Starting application in Release Mode (for smooth 60 FPS)...
cargo run -p cache-advisor --release --features ai
if %ERRORLEVEL% neq 0 (
    echo.
    echo Application exited with an error or cargo run failed.
    echo Running in Debug/Dev mode as fallback...
    cargo run -p cache-advisor --features ai
)
pause
