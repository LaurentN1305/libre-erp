param([string]$Dossier=(Join-Path $env:LOCALAPPDATA 'LibreERP-Editeur'))
Add-Type -AssemblyName System.Security
. (Join-Path $PSScriptRoot 'Resoudre-Coffre-Signature.ps1')
$taskResolvedVault=Resolve-LibreErpSigningVault -Dossier $Dossier
if(-not $taskResolvedVault){throw 'Coffre de signature introuvable. Restaurez une sauvegarde .erp-cles avec Restaurer-Cles-Signature.cmd. Si des versions sont deja publiees, ne creez pas de nouvelles cles.'}
$taskCipher=[IO.File]::ReadAllBytes((Join-Path $taskResolvedVault 'signatures.dpapi'))
$taskPlain=[Security.Cryptography.ProtectedData]::Unprotect($taskCipher,[Text.Encoding]::UTF8.GetBytes('LibreERP-Editeur-v1'),[Security.Cryptography.DataProtectionScope]::CurrentUser)
try{
    $taskEditorKeys=[Text.Encoding]::UTF8.GetString($taskPlain) | ConvertFrom-Json
    if($taskEditorKeys.format -ne 1){throw 'Format de coffre inconnu.'}
    $taskPublishedKeysPath=Join-Path $PSScriptRoot '../publication/cles-publiques.json'
    if(Test-Path -LiteralPath $taskPublishedKeysPath){
        $taskPublishedKeys=Get-Content -LiteralPath $taskPublishedKeysPath -Raw | ConvertFrom-Json
        if($taskPublishedKeys.tauri -cne $taskEditorKeys.tauri_public -or $taskPublishedKeys.manifest -cne $taskEditorKeys.manifest_public){
            $taskEditorKeys=$null
            throw 'Ce coffre ne correspond pas aux cles publiques du logiciel publie. Aucune signature ne sera chargee.'
        }
    }
    $env:TAURI_SIGNING_PRIVATE_KEY=$taskEditorKeys.tauri_private
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD=''
    $env:ERP_MANIFEST_PUBLIC_KEY=$taskEditorKeys.manifest_public
    $env:ERP_UPDATE_BASE_URL='https://github.com/LaurentN1305/libre-erp/releases/download'
    $env:ERP_MANIFEST_KEY_PASSWORD=$taskEditorKeys.manifest_password
}finally{[Array]::Clear($taskPlain,0,$taskPlain.Length)}
