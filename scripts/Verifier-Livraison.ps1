param([ValidateSet('P0','M1','M2','M4','M3','M5','M6','M7','M8')][string]$Module='M4', [string]$Revision='')
. (Join-Path $PSScriptRoot 'Commun.ps1')
$taskFolderName="Livraison-$Module"
if ($Revision) { if ($Revision -notmatch '^\d+\.\d+\.\d+$') { throw 'Revision invalide.' }; $taskFolderName+="-$Revision" }
$taskFolder = Join-Path $taskRoot $taskFolderName
$taskPrefix=if($Module -in @('M5','M6','M7','M8')){'LibreERP'}else{'SecuriteERP'}
$taskResults = @()
foreach ($taskName in @("$taskPrefix-$Module-essai-setup.exe","$taskPrefix-$Module-essai.msi","$taskPrefix-$Module-recuperation.exe")) {
    $taskPath = Join-Path $taskFolder $taskName
    $taskFile = Get-Item -LiteralPath $taskPath
    $taskSignature = Get-AuthenticodeSignature -LiteralPath $taskPath
    $taskResults += [pscustomobject]@{ fichier=$taskName; octets=$taskFile.Length; sha256=(Get-FileHash -LiteralPath $taskPath -Algorithm SHA256).Hash; signature=$taskSignature.Status.ToString() }
}
$taskInstaller = New-Object -ComObject WindowsInstaller.Installer
$taskMsi = Join-Path $taskFolder "$taskPrefix-$Module-essai.msi"
$taskDatabase = $taskInstaller.OpenDatabase($taskMsi,0)
$taskView = $taskDatabase.OpenView('SELECT `Value` FROM `Property` WHERE `Property` = ''ProductVersion''')
$taskView.Execute()
$taskRecord = $taskView.Fetch()
$taskVersion = $taskRecord.StringData(1)
$taskView.Close()
$taskReport = [pscustomobject]@{ version_msi=$taskVersion; profil='essai-debug'; fichiers=$taskResults }
$taskReport | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $taskFolder 'verification.json') -Encoding utf8
$taskReport | ConvertTo-Json -Depth 5
