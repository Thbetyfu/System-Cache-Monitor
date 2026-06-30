@echo off
title Cache Advisor Launcher
echo ===================================================
echo               CACHE ADVISOR LAUNCHER
echo ===================================================
echo.
echo Setting up environment variables...
set LIBCLANG_PATH=D:\LLVM19\bin

:: Check if precompiled release binary exists
set BIN_PATH=
if exist "D:\.cargo-target\release\cache-advisor.exe" (
    set BIN_PATH=D:\.cargo-target\release\cache-advisor.exe
) else if exist "target\release\cache-advisor.exe" (
    set BIN_PATH=target\release\cache-advisor.exe
)

if not "%BIN_PATH%"=="" (
    echo.
    echo Found precompiled Release binary.
    echo [1] Run precompiled Release binary (Instantly)
    echo [2] Recompile and Run (Takes several minutes due to LTO)
    echo.
    set /p choice="Select option (default is 1): "
    if "%choice%"=="" set choice=1
    if "%choice%"=="1" (
        echo.
        echo Launching application...
        start "" "%BIN_PATH%"
        exit /b
    )
)

echo.
echo Compiling and starting application in Release Mode...
cargo run -p cache-advisor --release --features ai
if %ERRORLEVEL% neq 0 (
    echo.
    echo Application exited with an error or cargo run failed.
    echo Running in Debug/Dev mode as fallback...
    cargo run -p cache-advisor --features ai
)
pause
