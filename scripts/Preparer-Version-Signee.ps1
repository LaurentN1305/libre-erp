param([Parameter(Mandatory=$true)][ValidateSet('stable','beta')][string]$Canal,[Parameter(Mandatory=$true)][long]$Sequence,[Parameter(Mandatory=$true)][string]$Sortie,[string]$Nouveautes=(Join-Path $PSScriptRoot '../publication/NOUVEAUTES-0.11.0.txt'),[string]$DossierCles=(Join-Path $env:LOCALAPPDATA 'LibreERP-Editeur'))
. (Join-Path $PSScriptRoot 'Commun.ps1')
try{
    . (Join-Path $PSScriptRoot 'Charger-Signatures.ps1') -Dossier $DossierCles
    $env:ERP_MANIFEST_PRIVATE_KEY=$taskEditorKeys.manifest_private
    $taskVersion=(Get-Content -LiteralPath (Join-Path $taskRoot 'package.json') -Raw | ConvertFrom-Json).version
    $taskExe=Join-Path $taskRoot ('src-tauri/target/release/bundle/nsis/Libre ERP_'+$taskVersion+'_x64-setup.exe')
    $taskMsi=Join-Path $taskRoot ('src-tauri/target/release/bundle/msi/Libre ERP_'+$taskVersion+'_x64_fr-FR.msi')
    Invoke-Checked 'node' @((Join-Path $PSScriptRoot 'Preparer-Publication.mjs'),'--channel',$Canal,'--sequence',([string]$Sequence),'--nsis',$taskExe,'--msi',$taskMsi,'--output',$Sortie,'--notes-file',$Nouveautes)
}finally{
    foreach($taskVar in @('TAURI_SIGNING_PRIVATE_KEY','TAURI_SIGNING_PRIVATE_KEY_PASSWORD','ERP_MANIFEST_KEY_PASSWORD','ERP_MANIFEST_PRIVATE_KEY')){[Environment]::SetEnvironmentVariable($taskVar,$null,'Process')}
    $taskEditorKeys=$null
}
