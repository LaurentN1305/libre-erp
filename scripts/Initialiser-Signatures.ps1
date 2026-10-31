param([string]$Dossier=(Join-Path $env:LOCALAPPDATA 'LibreERP-Editeur'))
. (Join-Path $PSScriptRoot 'Commun.ps1')
Add-Type -AssemblyName System.Security
. (Join-Path $PSScriptRoot 'Resoudre-Coffre-Signature.ps1')
$taskExistingVault=Resolve-LibreErpSigningVault -Dossier $Dossier
if($taskExistingVault){throw 'Un coffre existe deja, y compris dans les donnees de Codex. Utilisez Sauvegarder-Cles-Signature.cmd : aucune cle ne sera remplacee.'}
$taskVault=[IO.Path]::GetFullPath($Dossier)
if($taskVault.StartsWith($taskRoot+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'Le coffre doit rester hors du projet.'}
$taskVaultFile=Join-Path $taskVault 'signatures.dpapi'
if(Test-Path -LiteralPath $taskVaultFile){throw 'Un coffre existe deja. Il ne sera pas remplace.'}
New-Item -ItemType Directory -Path $taskVault -Force | Out-Null
$taskAcl=New-Object Security.AccessControl.DirectorySecurity
$taskAcl.SetAccessRuleProtection($true,$false)
foreach($taskSid in @([Security.Principal.WindowsIdentity]::GetCurrent().User,(New-Object Security.Principal.SecurityIdentifier 'S-1-5-18'))){$taskRule=New-Object Security.AccessControl.FileSystemAccessRule($taskSid,'FullControl','ContainerInherit,ObjectInherit','None','Allow');$taskAcl.AddAccessRule($taskRule)}
Set-Acl -LiteralPath $taskVault -AclObject $taskAcl
$taskTempKey=Join-Path $taskVault ('temp-'+[guid]::NewGuid().ToString()+'.key')
$taskPrivateBytes=$null
try{
    $taskCliOutput=& node (Join-Path $taskRoot 'node_modules/@tauri-apps/cli/tauri.js') signer generate --ci --write-keys $taskTempKey 2>&1
    if($LASTEXITCODE -ne 0){throw 'Generation Tauri impossible ; aucune sortie privee affichee.'}
    $taskSecretJson=& node (Join-Path $PSScriptRoot 'Materiel-Signature.mjs') $taskTempKey
    if($LASTEXITCODE -ne 0){throw 'Generation Ed25519 impossible.'}
    $taskPrivateBytes=[Text.Encoding]::UTF8.GetBytes([string]$taskSecretJson)
    $taskEntropy=[Text.Encoding]::UTF8.GetBytes('LibreERP-Editeur-v1')
    $taskProtected=[Security.Cryptography.ProtectedData]::Protect($taskPrivateBytes,$taskEntropy,[Security.Cryptography.DataProtectionScope]::CurrentUser)
    $taskStream=[IO.File]::Open($taskVaultFile,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
    try{$taskStream.Write($taskProtected,0,$taskProtected.Length);$taskStream.Flush($true)}finally{$taskStream.Dispose()}
    $taskKeys=$taskSecretJson | ConvertFrom-Json
    $taskPublicJson=[pscustomobject]@{format=1;tauri=$taskKeys.tauri_public;manifest=$taskKeys.manifest_public} | ConvertTo-Json
    [IO.File]::WriteAllText((Join-Path $taskVault 'publiques.json'),$taskPublicJson,(New-Object Text.UTF8Encoding($false)))
    Write-Host "Coffre de signature cree : $taskVault. Les cles privees restent chiffrees pour votre compte Windows."
}finally{
    if($taskPrivateBytes){[Array]::Clear($taskPrivateBytes,0,$taskPrivateBytes.Length)}
    $taskSecretJson=$null;$taskKeys=$null;$taskCliOutput=$null
    foreach($taskScratch in @($taskTempKey,($taskTempKey+'.pub'))){
        if(Test-Path -LiteralPath $taskScratch){$taskResolved=(Resolve-Path -LiteralPath $taskScratch).Path;if(-not $taskResolved.StartsWith($taskVault+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'Nettoyage temporaire hors du coffre refuse.'};Remove-Item -LiteralPath $taskResolved -Force}
    }
}
