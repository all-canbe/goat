@echo off
chcp 65001 >nul
title Goat TUI
REM =======================================================
REM [DEPRECATED] TUI 方式已废弃，仅作本地参考。
REM 请使用 CLI 方式：  python main.py
REM 或 Web 方式：      python main.py web
REM 该文件已加入 .gitignore，不再跟踪。
REM =======================================================
echo [WARNING] TUI 方式已废弃，请使用 CLI 或 Web 方式启动。
echo.
echo 使用 CLI: python main.py
echo 使用 Web: python main.py web
echo.
python tui_main.py --inplace %*
pause