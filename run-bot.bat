@echo off
REM Telegram Bot Auto-restart Script for Windows
REM Automatically restarts the bot if it crashes

echo ========================================
echo  Telegram Bot Auto-restart Script
echo ========================================
echo.

:restart
echo [%date% %time%] Starting telegram-bot...
cargo run -p telegram-bot

echo.
echo [%date% %time%] Bot exited with code: %ERRORLEVEL%
echo [%date% %time%] Restarting in 3 seconds...
echo Press Ctrl+C to stop
echo.

timeout /t 3 /nobreak > nul
goto restart
