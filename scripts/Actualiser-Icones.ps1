param([Parameter(Mandatory=$true)][string]$Application,[Parameter(Mandatory=$true)][string]$Icon,[string[]]$Folders=@())
$ErrorActionPreference='Stop'
$taskTarget=[IO.Path]::GetFullPath($Application)
if(-not (Test-Path -LiteralPath $Icon -PathType Leaf)){throw 'Icône absente'}
$taskShell=New-Object -ComObject WScript.Shell
$taskFolders=if($Folders.Count){$Folders}else{@([Environment]::GetFolderPath('Desktop'),[Environment]::GetFolderPath('StartMenu'),(Join-Path $env:APPDATA 'Microsoft/Internet Explorer/Quick Launch/User Pinned/TaskBar'))}
$taskCount=0
foreach($taskFolder in $taskFolders){
 if(-not $taskFolder -or -not (Test-Path -LiteralPath $taskFolder)){continue}
 foreach($taskFile in (Get-ChildItem -LiteralPath $taskFolder -Recurse -Filter '*.lnk' -File -ErrorAction SilentlyContinue)){
  try{$taskLink=$taskShell.CreateShortcut($taskFile.FullName);if($taskLink.TargetPath -and [string]::Equals([IO.Path]::GetFullPath($taskLink.TargetPath),$taskTarget,[StringComparison]::OrdinalIgnoreCase)){$taskLink.IconLocation=$Icon+',0';$taskLink.Save();$taskCount++
    if($taskFile.Name -in @('Sécurité ERP.lnk','Securite ERP.lnk','SecuriteERP.lnk')){
     $taskRenamed=Join-Path $taskFile.DirectoryName 'Libre ERP.lnk'
     if(-not(Test-Path -LiteralPath $taskRenamed)){Rename-Item -LiteralPath $taskFile.FullName -NewName 'Libre ERP.lnk'}
     else{$taskExisting=$taskShell.CreateShortcut($taskRenamed);if($taskExisting.TargetPath -and [string]::Equals([IO.Path]::GetFullPath($taskExisting.TargetPath),$taskTarget,[StringComparison]::OrdinalIgnoreCase)){Remove-Item -LiteralPath $taskFile.FullName}}
    }}}catch{continue}
 }
}
$taskRefresh=Join-Path $env:SystemRoot 'System32/ie4uinit.exe'
if(-not $Folders.Count -and (Test-Path -LiteralPath $taskRefresh)){& $taskRefresh -show | Out-Null}
Write-Output $taskCount
