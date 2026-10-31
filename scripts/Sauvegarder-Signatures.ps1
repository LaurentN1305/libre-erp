param([string]$Destination,[string]$DossierCles=(Join-Path $env:LOCALAPPDATA 'LibreERP-Editeur'))
. (Join-Path $PSScriptRoot 'Commun.ps1')
Add-Type -AssemblyName System.Windows.Forms
. (Join-Path $PSScriptRoot 'Resoudre-Coffre-Signature.ps1')
$taskExistingVault=Resolve-LibreErpSigningVault -Dossier $DossierCles
if(-not $taskExistingVault){throw 'Aucun coffre de signature retrouve. Ne regenerez pas les cles du logiciel deja publie. Utilisez Restaurer-Cles-Signature.cmd avec une sauvegarde .erp-cles.'}
$DossierCles=$taskExistingVault
Write-Host 'Coffre de signature existant retrouve. Cette operation sauvegarde les cles deja utilisees par Libre ERP.' -ForegroundColor Cyan
if(-not $Destination){$taskDialog=New-Object System.Windows.Forms.SaveFileDialog;$taskDialog.Title='Sauvegarder les cles de signature Libre ERP';$taskDialog.Filter='Coffre de signature (*.erp-cles)|*.erp-cles';$taskDialog.FileName='LibreERP-cles-signature.erp-cles';if($taskDialog.ShowDialog() -ne [Windows.Forms.DialogResult]::OK){return};$Destination=$taskDialog.FileName}
if(Test-Path -LiteralPath $Destination){throw 'Choisissez un nouveau fichier, sans remplacer une sauvegarde existante.'}
$taskFirst=Read-Host 'Phrase pour proteger cette sauvegarde (12 caracteres minimum)' -AsSecureString
$taskSecond=Read-Host 'Confirmez la phrase' -AsSecureString
$taskPtr1=[Runtime.InteropServices.Marshal]::SecureStringToBSTR($taskFirst);$taskPtr2=[Runtime.InteropServices.Marshal]::SecureStringToBSTR($taskSecond)
try{
    $taskPass1=[Runtime.InteropServices.Marshal]::PtrToStringBSTR($taskPtr1);$taskPass2=[Runtime.InteropServices.Marshal]::PtrToStringBSTR($taskPtr2)
    if($taskPass1.Length -lt 12 -or $taskPass1 -cne $taskPass2){throw 'Phrases differentes ou trop courtes.'}
    . (Join-Path $PSScriptRoot 'Charger-Signatures.ps1') -Dossier $DossierCles
    $env:ERP_KEY_BACKUP_PASSWORD=$taskPass1
    $taskEditorKeys | ConvertTo-Json -Compress | & node (Join-Path $PSScriptRoot 'coffre-portable.mjs') sauvegarder $Destination
    if($LASTEXITCODE -ne 0){throw 'Sauvegarde des cles echouee.'}
    Write-Host 'Copiez ce coffre chiffre sur un support separe et conservez la phrase a part. Ne publiez pas ce fichier sur GitHub.'
}finally{
    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($taskPtr1);[Runtime.InteropServices.Marshal]::ZeroFreeBSTR($taskPtr2)
    $taskPass1=$null;$taskPass2=$null;$taskEditorKeys=$null
    foreach($taskVar in @('TAURI_SIGNING_PRIVATE_KEY','TAURI_SIGNING_PRIVATE_KEY_PASSWORD','ERP_MANIFEST_KEY_PASSWORD','ERP_KEY_BACKUP_PASSWORD')){[Environment]::SetEnvironmentVariable($taskVar,$null,'Process')}
}
