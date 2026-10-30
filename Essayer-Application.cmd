@echo off
chcp 65001 >nul
if exist "%~dp0src-tauri\target\debug\securite-erp.exe" (
  start "Securite ERP" "%~dp0src-tauri\target\debug\securite-erp.exe"
) else (
  call "%~dp0Lancer-Application.cmd"
)
