@echo off
title VOIDWRAP RUN
cd /d "%~dp0"
echo.
echo   VOIDWRAP RUN is starting...
echo   Your browser will open the game in a few seconds.
echo   If it does not, type this address into your browser yourself:  http://localhost:7878
echo.
echo   KEEP THIS WINDOW OPEN while you play. Close it to stop the game.
echo.
set VOIDWRAP_OPEN_BROWSER=1
voidwrap-server.exe
echo.
echo   The game has stopped. Press any key to close this window.
pause >nul
