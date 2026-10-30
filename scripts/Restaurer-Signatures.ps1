param([string]$Fichier,[string]$Dossier=(Join-Path $env:LOCALAPPDATA 'LibreERP-Editeur'))
. (Join-Path $PSScriptRoot 'Commun.ps1')
Add-Type -AssemblyName System.Security
Add-Type -AssemblyName System.Windows.Forms
$taskVault=[IO.Path]::GetFullPath($Dossier)
if($taskVault.StartsWith($taskRoot+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'Le coffre doit rester hors du projet.'}
if(Test-Path -LiteralPath (Join-Path $taskVault 'signatures.dpapi')){throw 'Un coffre existe deja : aucune cle ne sera remplacee.'}
if(-not $Fichier){$taskDialog=New-Object Windows.Forms.OpenFileDialog;$taskDialog.Title='Restaurer les cles de signature Libre ERP';$taskDialog.Filter='Coffre de signature (*.erp-cles)|*.erp-cles';if($taskDialog.ShowDialog() -ne [Windows.Forms.DialogResult]::OK){return};$Fichier=$taskDialog.FileName}
$taskPhrase=Read-Host 'Phrase de la sauvegarde des cles' -AsSecureString
$taskPtr=[Runtime.InteropServices.Marshal]::SecureStringToBSTR($taskPhrase)
$taskPlain=$null
try{
    $env:ERP_KEY_BACKUP_PASSWORD=[Runtime.InteropServices.Marshal]::PtrToStringBSTR($taskPtr)
    $taskJson=& node (Join-Path $PSScriptRoot 'coffre-portable.mjs') restaurer $Fichier
    if($LASTEXITCODE -ne 0){throw 'Phrase incorrecte ou sauvegarde modifiee. Aucun coffre restaure.'}
    $taskRestored=$taskJson | ConvertFrom-Json
    if($taskRestored.format -ne 1 -or -not $taskRestored.tauri_private -or -not $taskRestored.manifest_private -or -not $taskRestored.manifest_password -or -not $taskRestored.tauri_public -or -not $taskRestored.manifest_public){throw 'Contenu du coffre incomplet.'}
    New-Item -ItemType Directory -Path $taskVault -Force | Out-Null
    $taskAcl=New-Object Security.AccessControl.DirectorySecurity;$taskAcl.SetAccessRuleProtection($true,$false)
    foreach($taskSid in @([Security.Principal.WindowsIdentity]::GetCurrent().User,(New-Object Security.Principal.SecurityIdentifier 'S-1-5-18'))){$taskAcl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($taskSid,'FullControl','ContainerInherit,ObjectInherit','None','Allow')))}
    Set-Acl -LiteralPath $taskVault -AclObject $taskAcl
    $taskPlain=[Text.Encoding]::UTF8.GetBytes([string]$taskJson)
    $taskProtected=[Security.Cryptography.ProtectedData]::Protect($taskPlain,[Text.Encoding]::UTF8.GetBytes('LibreERP-Editeur-v1'),[Security.Cryptography.DataProtectionScope]::CurrentUser)
    $taskStream=[IO.File]::Open((Join-Path $taskVault 'signatures.dpapi'),[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
    try{$taskStream.Write($taskProtected,0,$taskProtected.Length);$taskStream.Flush($true)}finally{$taskStream.Dispose()}
    $taskPublic=[pscustomobject]@{format=1;tauri=$taskRestored.tauri_public;manifest=$taskRestored.manifest_public}|ConvertTo-Json
    [IO.File]::WriteAllText((Join-Path $taskVault 'publiques.json'),$taskPublic,(New-Object Text.UTF8Encoding($false)))
    Write-Host 'Coffre restaure et chiffre pour ce compte Windows. Les cles publiques restent identiques.'
}finally{[Runtime.InteropServices.Marshal]::ZeroFreeBSTR($taskPtr);if($taskPlain){[Array]::Clear($taskPlain,0,$taskPlain.Length)};$taskJson=$null;$taskRestored=$null;[Environment]::SetEnvironmentVariable('ERP_KEY_BACKUP_PASSWORD',$null,'Process')}
