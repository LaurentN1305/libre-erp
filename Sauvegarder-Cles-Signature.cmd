@echo off
chcp 65001 >nul
powershell.exe -NoProfile -STA -ExecutionPolicy Bypass -File "%~dp0scripts\Sauvegarder-Signatures.ps1"
pause
