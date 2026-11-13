@echo off
title VOIDWRAP RUN - hosting for friends
cd /d "%~dp0"
echo.
echo   VOIDWRAP RUN - HOSTING FOR FRIENDS ON YOUR NETWORK
echo   -------------------------------------------------
echo   Friends on the SAME Wi-Fi or network type one of these addresses into their browser:
echo.
powershell -NoProfile -Command "Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.IPAddress -notlike '127.*' -and $_.IPAddress -notlike '169.254.*' } | ForEach-Object { '       http://' + $_.IPAddress + ':7878' }"
echo.
echo   If Windows asks whether to allow access, click "Allow access" - otherwise friends cannot join.
echo   KEEP THIS WINDOW OPEN while you play. Close it to stop the game.
echo.
set VOIDWRAP_HOST=0.0.0.0
set VOIDWRAP_OPEN_BROWSER=1
voidwrap-server.exe
echo.
echo   The game has stopped. Press any key to close this window.
pause >nul
