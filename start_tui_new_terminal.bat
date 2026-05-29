@echo off
chcp 65001 >nul
title Goat TUI Launcher
echo 正在在新窗口中启动 Goat TUI...
start "Goat TUI" cmd /k python tui_main.py --inplace %*
echo.
echo TUI 已在新终端窗口中启动。
echo 关闭此窗口不会影响 TUI 运行。
echo.
pause