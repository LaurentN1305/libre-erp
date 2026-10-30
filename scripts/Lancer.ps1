. (Join-Path $PSScriptRoot 'Commun.ps1')
Assert-BuildTools
Write-Host 'Preparation de Securite ERP...' -ForegroundColor Cyan
if (-not (Test-Path -LiteralPath 'node_modules')) { Invoke-PackageManager @('install','--frozen-lockfile') }
Invoke-PackageManager @('exec','tauri','dev')
