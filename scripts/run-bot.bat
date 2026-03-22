@echo off
chcp 65001 >nul
setlocal enabledelayedexpansion

title Telegram Bot - Auto Restart

echo ========================================
 echo  Telegram Bot Auto-Restart Script
 echo  Safe Restart on Crash
 echo ========================================
echo.
 echo Bot will automatically restart if it crashes.
 echo Press Ctrl+C to stop.
echo.

:restart
REM Stop any existing telegram-bot process before starting
echo [%date% %time%] Stopping existing telegram-bot processes...
taskkill /F /IM telegram-bot.exe >nul 2>&1
timeout /t 1 /nobreak >nul

echo [%date% %time%] Starting telegram-bot...
echo.

target\release\telegram-bot.exe

echo.
echo [%date% %time%] Bot exited with code: %ERRORLEVEL%
echo [%date% %time%] Restarting in 3 seconds...
 echo (Press Ctrl+C to cancel)
echo.

timeout /t 3 /nobreak >nul
goto restart
