@echo off
chcp 65001 >nul
title Telegram Bot - Development Mode

echo ========================================
echo  Telegram Bot Development Mode
echo  1. Stop old process
echo  2. Build
echo  3. Run
echo ========================================
echo.

:restart
REM Step 1: Stop old process
echo [%date% %time%] Stopping old telegram-bot process...
taskkill /f /im telegram-bot.exe >nul 2>&1
timeout /t 2 /nobreak >nul

REM Step 2: Build
echo [%date% %time%] Building telegram-bot (release)...
echo.

cargo build -p telegram-bot --release
if %ERRORLEVEL% neq 0 (
    echo.
    echo [%date% %time%] Build FAILED with code: %ERRORLEVEL%
    echo [%date% %time%] Fix the error and press any key to retry...
    pause >nul
    goto restart
)

echo.
echo [%date% %time%] Build successful!
echo.

REM Step 3: Run
echo [%date% %time%] Starting bot...
echo.

target\release\telegram-bot.exe

echo.
echo [%date% %time%] Bot exited with code: %ERRORLEVEL%

REM If exit code is 0 (normal exit), ask user
if %ERRORLEVEL% equ 0 (
    echo Bot exited normally.
    choice /c YN /m "Restart bot? (Y/N)"
    if errorlevel 2 goto end
    goto restart
)

REM If crash (non-zero exit), auto restart
echo [%date% %time%] Bot crashed. Restarting in 3 seconds...
echo (Press Ctrl+C to cancel)
timeout /t 3 /nobreak >nul
goto restart

:end
echo.
echo Goodbye!
