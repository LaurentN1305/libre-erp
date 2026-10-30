param([Parameter(Mandatory=$true)][string]$Application, [string]$Heure = '18:00')
$ErrorActionPreference = 'Stop'
$taskExe = (Resolve-Path -LiteralPath $Application).Path
if ([IO.Path]::GetExtension($taskExe) -ne '.exe') { throw 'Indiquez le fichier executable de l application installee.' }
$taskTime = [datetime]::ParseExact($Heure,'HH:mm',[Globalization.CultureInfo]::InvariantCulture)
$taskAction = New-ScheduledTaskAction -Execute $taskExe -Argument '--backup-only'
$taskTrigger = New-ScheduledTaskTrigger -Daily -At $taskTime
$taskPrincipal = New-ScheduledTaskPrincipal -UserId ([Security.Principal.WindowsIdentity]::GetCurrent().Name) -LogonType Interactive -RunLevel Limited
$taskSettings = New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -MultipleInstances IgnoreNew
$taskName = 'SecuriteERP-Sauvegarde-' + [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
Register-ScheduledTask -TaskName $taskName -Action $taskAction -Trigger $taskTrigger -Principal $taskPrincipal -Settings $taskSettings -Description 'Sauvegarde locale chiffree de Securite ERP, selon la configuration de l application.' -Force | Out-Null
Write-Host 'Tache creee pour le compte Windows actuel. Activez aussi la sauvegarde dans les parametres de l application.'
