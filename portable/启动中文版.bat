@echo off
setlocal
cd /d "%~dp0"
if not exist "%~dp0data" mkdir "%~dp0data"
set "AICHAT_CONFIG_DIR=%~dp0data"
if not exist "%~dp0aichat.exe" (
  echo [错误] 未找到 aichat.exe，请确认程序文件与本启动器位于同一目录。
  pause
  exit /b 1
)
"%~dp0aichat.exe" %*
endlocal
