param([Parameter(Mandatory=$true)][string]$Installateur,[Parameter(Mandatory=$true)][string]$Preuve,[string]$Verificateur=(Join-Path $PSScriptRoot 'securite-erp-recovery.exe'))
$ErrorActionPreference='Stop'
$taskPath=(Resolve-Path -LiteralPath $Installateur).Path
$taskProof=(Resolve-Path -LiteralPath $Preuve).Path
$taskVerifier=(Resolve-Path -LiteralPath $Verificateur).Path
if([IO.Path]::GetExtension($taskPath) -notin @('.exe','.msi')){throw 'Format installeur inconnu.'}
# Garder le fichier verrouille en lecture pendant controle, confirmation et lancement.
$taskLock=[IO.File]::Open($taskPath,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
try {
    & $taskVerifier '--verifier-installateur' $taskPath $taskProof
    if($LASTEXITCODE -ne 0){throw 'Preuve cryptographique invalide : aucun installeur lance.'}
    Write-Host 'Fermez Libre ERP et conservez une sauvegarde compatible avec cette ancienne version.'
    $taskConfirm=Read-Host 'Saisissez RETOUR pour lancer cet installeur authentifie'
    if($taskConfirm -ne 'RETOUR'){throw 'Operation annulee.'}
    if([IO.Path]::GetExtension($taskPath) -eq '.msi'){
        $taskProcess=Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i',('"'+$taskPath+'"')) -WindowStyle Hidden -Wait -PassThru
    }else{
        $taskProcess=Start-Process -FilePath $taskPath -WindowStyle Hidden -Wait -PassThru
    }
    if($taskProcess.ExitCode -notin @(0,3010)){throw "Installation terminee avec le code $($taskProcess.ExitCode)."}
    Write-Host 'Utilisez ensuite la recuperation pour restaurer la sauvegarde compatible si necessaire.'
} finally {$taskLock.Dispose()}
