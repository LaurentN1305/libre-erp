. (Join-Path $PSScriptRoot 'Commun.ps1')
Assert-BuildTools
Write-Host 'Preparation de Securite ERP...' -ForegroundColor Cyan
if (-not (Test-Path -LiteralPath 'node_modules')) { Invoke-PackageManager @('install','--frozen-lockfile') }
# Embarque l'interface compilée pour démarrer sans serveur web local.
Invoke-Checked 'node' @('scripts/frontend.mjs','build')
Invoke-Checked 'cargo' @('run','--manifest-path','src-tauri/Cargo.toml','--features','custom-protocol','--bin','securite-erp')
