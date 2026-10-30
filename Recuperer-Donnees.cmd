@echo off
chcp 65001 >nul
if exist "%~dp0src-tauri\target\release\securite-erp-recovery.exe" (
  "%~dp0src-tauri\target\release\securite-erp-recovery.exe"
) else (
  echo Outil de recuperation indisponible. Fabriquez les installateurs en premier.
  pause
)
