[CmdletBinding(SupportsShouldProcess=$true)]
param([Parameter(Mandatory=$true)][string]$Application,[string]$Heure='08:00',[ValidateSet('0','1')][string]$Activer='1')
$ErrorActionPreference='Stop'
$taskExe=(Resolve-Path -LiteralPath $Application).Path
if([IO.Path]::GetExtension($taskExe) -ne '.exe'){throw 'Executable requis'}
$taskTime=[datetime]::ParseExact($Heure,'HH:mm',[Globalization.CultureInfo]::InvariantCulture)
$taskName='LibreERP-Alertes-'+[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
if($Activer -eq '0'){if($PSCmdlet.ShouldProcess($taskName,'Retirer la tache')){Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue | Unregister-ScheduledTask -Confirm:$false};return}
$taskAction=New-ScheduledTaskAction -Execute $taskExe -Argument '--alerts-only'
$taskTrigger=New-ScheduledTaskTrigger -Daily -At $taskTime
$taskPrincipal=New-ScheduledTaskPrincipal -UserId ([Security.Principal.WindowsIdentity]::GetCurrent().Name) -LogonType Interactive -RunLevel Limited
$taskSettings=New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -MultipleInstances IgnoreNew
if($PSCmdlet.ShouldProcess($taskName,'Creer la tache quotidienne')){Register-ScheduledTask -TaskName $taskName -Action $taskAction -Trigger $taskTrigger -Principal $taskPrincipal -Settings $taskSettings -Description 'Verification locale des alertes Libre ERP. Session Windows ouverte necessaire. Aucun secret en argument.' -Force | Out-Null}
