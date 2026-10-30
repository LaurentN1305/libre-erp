param([string]$Dossier=(Join-Path $env:LOCALAPPDATA 'LibreERP-Editeur'))
Add-Type -AssemblyName System.Security
$taskCipher=[IO.File]::ReadAllBytes((Join-Path $Dossier 'signatures.dpapi'))
$taskPlain=[Security.Cryptography.ProtectedData]::Unprotect($taskCipher,[Text.Encoding]::UTF8.GetBytes('LibreERP-Editeur-v1'),[Security.Cryptography.DataProtectionScope]::CurrentUser)
try{
    $taskEditorKeys=[Text.Encoding]::UTF8.GetString($taskPlain) | ConvertFrom-Json
    if($taskEditorKeys.format -ne 1){throw 'Format de coffre inconnu.'}
    $env:TAURI_SIGNING_PRIVATE_KEY=$taskEditorKeys.tauri_private
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD=''
    $env:ERP_MANIFEST_PUBLIC_KEY=$taskEditorKeys.manifest_public
    $env:ERP_UPDATE_BASE_URL='https://github.com/LaurentN1305/libre-erp/releases/download'
    $env:ERP_MANIFEST_KEY_PASSWORD=$taskEditorKeys.manifest_password
}finally{[Array]::Clear($taskPlain,0,$taskPlain.Length)}
