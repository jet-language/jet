@echo off
mode con >nul 2>nul || exit /b 41
echo ready
set /p line=
echo input:%line%
echo [31mcontrol[0m
mode con
