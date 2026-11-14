@echo off
title VOIDWRAP RUN - browser mode
cd /d "%~dp0"
echo.
echo   VOIDWRAP RUN is starting in your web browser.
echo   Use this only if the normal "VOIDWRAP RUN" program window does not work on this computer.
echo   KEEP THIS WINDOW OPEN while you play. Close it to stop the game.
echo.
set VOIDWRAP_OPEN_BROWSER=1
"VOIDWRAP RUN.exe" --browser
echo.
echo   The game has stopped. Press any key to close this window.
pause >nul
