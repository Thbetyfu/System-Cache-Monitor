@echo off
title Cache Advisor Launcher
echo ===================================================
echo               CACHE ADVISOR LAUNCHER
echo ===================================================
echo.
echo Setting up environment variables...
:: Auto-detect libclang from Visual Studio 2022 bundled LLVM (preferred)
:: Falls back to standalone LLVM install if VS is not found.
set LIBCLANG_PATH=
if exist "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\Llvm\x64\bin\libclang.dll" (
    set LIBCLANG_PATH=C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\Llvm\x64\bin
) else if exist "C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Tools\Llvm\x64\bin\libclang.dll" (
    set LIBCLANG_PATH=C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Tools\Llvm\x64\bin
) else if exist "C:\Program Files\LLVM\bin\libclang.dll" (
    set LIBCLANG_PATH=C:\Program Files\LLVM\bin
) else if exist "D:\LLVM19\bin\libclang.dll" (
    set LIBCLANG_PATH=D:\LLVM19\bin
)

if "%LIBCLANG_PATH%"=="" (
    echo [WARNING] libclang.dll tidak ditemukan. Build AI feature mungkin gagal.
    echo           Install LLVM dari https://releases.llvm.org/ atau via Visual Studio Installer.
) else (
    echo [OK] LIBCLANG_PATH = %LIBCLANG_PATH%
)

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
