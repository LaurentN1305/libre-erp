param([string]$DossierCles=(Join-Path $env:LOCALAPPDATA 'LibreERP-Editeur'))
. (Join-Path $PSScriptRoot 'Commun.ps1')
try{
    . (Join-Path $PSScriptRoot 'Charger-Signatures.ps1') -Dossier $DossierCles
    $taskRelease=[ordered]@{bundle=@{createUpdaterArtifacts=$true};plugins=@{updater=@{pubkey=$taskEditorKeys.tauri_public;requireSignedVersion=$true;endpoints=@('https://github.com/LaurentN1305/libre-erp/releases/download/stable/windows-x86_64-{{bundle_type}}.json')}}}
    [IO.File]::WriteAllText((Join-Path $taskRoot 'src-tauri/tauri.release.json'),($taskRelease | ConvertTo-Json -Depth 6),(New-Object Text.UTF8Encoding($false)))
    & (Join-Path $PSScriptRoot 'Compiler.ps1') -Publication
}finally{
    foreach($taskVar in @('TAURI_SIGNING_PRIVATE_KEY','TAURI_SIGNING_PRIVATE_KEY_PASSWORD','ERP_MANIFEST_KEY_PASSWORD')){[Environment]::SetEnvironmentVariable($taskVar,$null,'Process')}
    $taskEditorKeys=$null
}
