@echo off
chcp 65001 >nul
title Goat TUI
echo 正在启动 Goat TUI...
echo.
python tui_main.py --inplace %*
pause