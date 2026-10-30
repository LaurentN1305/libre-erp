$ErrorActionPreference = 'Stop'
# Évite les verrous et liens durs du cache incrémental sur les volumes synchronisés.
if (-not $env:CARGO_INCREMENTAL) { $env:CARGO_INCREMENTAL = '0' }
$taskRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $taskRoot
$taskPerl = Join-Path $taskRoot '.tools\strawberry\perl\bin'
if (Test-Path -LiteralPath $taskPerl) { $env:PATH = $taskPerl + ';' + $env:PATH }
$taskOpenSsl = Join-Path $taskRoot '.tools\openssl'
$taskOpenSslLocation = Join-Path $taskRoot '.tools\openssl-location.txt'
if (Test-Path -LiteralPath $taskOpenSslLocation) {
    $taskRecorded = (Get-Content -LiteralPath $taskOpenSslLocation -Raw).Trim()
    if (Test-Path -LiteralPath $taskRecorded) { $taskOpenSsl = $taskRecorded }
}
if ((Test-Path -LiteralPath (Join-Path $taskOpenSsl 'lib\libcrypto.lib')) -and (Test-Path -LiteralPath (Join-Path $taskOpenSsl 'include\openssl\ssl.h'))) {
    $env:OPENSSL_DIR = $taskOpenSsl
    $env:OPENSSL_STATIC = '1'
    $env:OPENSSL_NO_VENDOR = '1'
}
$taskCargo = Join-Path $taskRoot '.tools\cargo\bin'
if (Test-Path -LiteralPath $taskCargo) {
    $env:CARGO_HOME = Join-Path $taskRoot '.tools\cargo'
    $env:RUSTUP_HOME = Join-Path $taskRoot '.tools\rustup'
    $env:PATH = $taskCargo + ';' + $env:PATH
} elseif (Test-Path -LiteralPath (Join-Path $env:USERPROFILE '.cargo\bin')) {
    $env:PATH = (Join-Path $env:USERPROFILE '.cargo\bin') + ';' + $env:PATH
}
$taskRuntime = Join-Path $env:USERPROFILE '.cache\codex-runtimes\codex-primary-runtime\dependencies'
if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
    $taskBundledNode = Join-Path $taskRuntime 'node\bin'
    if (Test-Path -LiteralPath (Join-Path $taskBundledNode 'node.exe')) { $env:PATH = $taskBundledNode + ';' + $env:PATH }
}
function Invoke-Checked {
    param([string]$File, [string[]]$Arguments)
    & $File @Arguments
    if ($LASTEXITCODE -ne 0) { throw "La commande a echoue (code $LASTEXITCODE) : $File" }
}
function Invoke-PackageManager {
    param([string[]]$Arguments)
    $taskPnpm = Get-Command pnpm -ErrorAction SilentlyContinue
    $taskBundledPnpm = Join-Path $taskRuntime 'bin\fallback\pnpm.cmd'
    if ($taskPnpm) { Invoke-Checked $taskPnpm.Source $Arguments }
    elseif (Test-Path -LiteralPath $taskBundledPnpm) { Invoke-Checked $taskBundledPnpm $Arguments }
    elseif (Get-Command npx -ErrorAction SilentlyContinue) { Invoke-Checked 'npx.cmd' (@('--yes', 'pnpm@11.25.0') + $Arguments) }
    else { throw 'Node.js LTS avec npm est necessaire. Ouvrez LISEZ-MOI.md pour les liens officiels.' }
}
function Assert-BuildTools {
    if (-not (Get-Command node -ErrorAction SilentlyContinue)) { throw 'Node.js LTS est absent. Voir LISEZ-MOI.md.' }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'Rust est absent. Voir LISEZ-MOI.md.' }
}
