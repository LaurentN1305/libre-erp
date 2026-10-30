param([switch]$Publication, [switch]$Essai)
. (Join-Path $PSScriptRoot 'Commun.ps1')
Assert-BuildTools
if ($Publication -and $Essai) { throw 'La publication doit utiliser le profil release.' }
Write-Host '1/4 - Dependances' -ForegroundColor Cyan
Invoke-PackageManager @('install','--frozen-lockfile')
Write-Host '2/4 - Verification de l interface' -ForegroundColor Cyan
Invoke-PackageManager @('run','build')
Invoke-PackageManager @('test')
Write-Host '3/4 - Tests du moteur et du chiffrement' -ForegroundColor Cyan
Invoke-Checked 'cargo' @('test','--locked','--manifest-path','src-tauri/Cargo.toml')
Write-Host '4/4 - Fabrication des installateurs Windows' -ForegroundColor Cyan
$taskProfile = if ($Essai) { 'debug' } else { 'release' }
$taskRecoveryArgs = @('build','--locked','--manifest-path','src-tauri/Cargo.toml','--bin','securite-erp-recovery')
if ($Essai) { $taskRecoveryArgs = @('build','--locked','--manifest-path','src-tauri/Cargo.toml','--bins','--features','custom-protocol','-j','1') }
if (-not $Essai) { $taskRecoveryArgs += '--release' }
Invoke-Checked 'cargo' $taskRecoveryArgs
Copy-Item -LiteralPath "src-tauri\target\$taskProfile\securite-erp-recovery.exe" -Destination 'src-tauri\resources\securite-erp-recovery.exe'
$taskArgs = @('exec','tauri','build','--bundles','nsis,msi','--config','src-tauri/tauri.distribution.json')
if ($Essai) { $taskArgs = @('exec','tauri','bundle','--debug','--features','custom-protocol','--bundles','nsis,msi','--config','src-tauri/tauri.distribution.json','--ci') }
if ($Publication) {
    $taskConfig = Join-Path $taskRoot 'src-tauri\tauri.release.json'
    if (-not (Test-Path -LiteralPath $taskConfig)) { throw 'Configuration de publication absente : src-tauri/tauri.release.json.' }
    $taskText = Get-Content -LiteralPath $taskConfig -Raw
    if ($taskText -match 'REPLACE_|example\.invalid|REPLACE_WITH') { throw 'La configuration contient encore des valeurs a renseigner.' }
    foreach ($taskName in @('TAURI_SIGNING_PRIVATE_KEY','ERP_MANIFEST_PUBLIC_KEY','ERP_UPDATE_BASE_URL')) {
        if (-not [Environment]::GetEnvironmentVariable($taskName)) { throw "Configuration de publication manquante : $taskName" }
    }
    $taskRelease = $taskText | ConvertFrom-Json
    if (-not $taskRelease.bundle.createUpdaterArtifacts) { throw 'La publication doit activer createUpdaterArtifacts.' }
    if (-not $taskRelease.bundle.windows.certificateThumbprint) { throw 'Certificat de signature Windows absent.' }
    $taskSigner = Get-Command signtool -ErrorAction SilentlyContinue
    if (-not $taskSigner) { throw 'signtool doit etre disponible pour signer outil de recuperation.' }
    Invoke-Checked 'signtool' @('sign','/sha1',$taskRelease.bundle.windows.certificateThumbprint,'/fd','SHA256','/tr',$taskRelease.bundle.windows.timestampUrl,'/td','SHA256','src-tauri/resources/securite-erp-recovery.exe')
    $taskArgs += @('--config','src-tauri/tauri.release.json')
} else {
    Write-Host 'Version de test : installateurs non signes, mises a jour non publiees.' -ForegroundColor Yellow
}
Invoke-PackageManager $taskArgs
$taskOutput = Join-Path $taskRoot "src-tauri\target\$taskProfile\bundle"
if ($Publication) {
    foreach ($taskInstaller in Get-ChildItem -LiteralPath $taskOutput -Recurse -File | Where-Object { $_.Extension -in @('.exe','.msi') }) {
        $taskSignature = Get-AuthenticodeSignature -LiteralPath $taskInstaller.FullName
        if ($taskSignature.Status -ne 'Valid') { throw "Signature Windows invalide : $($taskInstaller.Name)" }
    }
}
Write-Host "Termine. Les installateurs se trouvent dans : $taskOutput" -ForegroundColor Green
Invoke-Item -LiteralPath $taskOutput
