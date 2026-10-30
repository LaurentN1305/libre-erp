param([Parameter(Mandatory=$true)][string]$Installateur, [Parameter(Mandatory=$true)][string]$EmpreinteEditeur)
$ErrorActionPreference = 'Stop'
$taskPath = (Resolve-Path -LiteralPath $Installateur).Path
if ([IO.Path]::GetExtension($taskPath) -notin @('.exe','.msi')) { throw 'Format installeur inconnu.' }
$taskSignature = Get-AuthenticodeSignature -LiteralPath $taskPath
if ($taskSignature.Status -ne 'Valid') { throw 'La signature Windows de cet installeur est invalide.' }
if ($taskSignature.SignerCertificate.Thumbprint -ne $EmpreinteEditeur.Replace(' ','')) { throw 'Le certificat ne correspond pas a celui de votre editeur.' }
Write-Host 'Fermez application et outil de recuperation avant de continuer.'
$taskConfirm = Read-Host 'Saisissez RETOUR pour lancer cet ancien installeur signe'
if ($taskConfirm -ne 'RETOUR') { throw 'Operation annulee.' }
if ([IO.Path]::GetExtension($taskPath) -eq '.msi') {
    Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i',('"' + $taskPath + '"')) -WindowStyle Hidden -Wait
} else {
    Start-Process -FilePath $taskPath -WindowStyle Hidden -Wait
}
Write-Host 'Utilisez ensuite outil de recuperation pour restaurer la sauvegarde compatible si necessaire.'
