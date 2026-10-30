param([string]$Version='0.8.0',[ValidateSet('M4','M3','M5','M6','M7','M8')][string]$Module='M4',[string]$Revision='')
. (Join-Path $PSScriptRoot 'Commun.ps1')
if($Version -notmatch '^\d+\.\d+\.\d+$'){throw 'Version invalide'}
$taskDelivery=Join-Path $taskRoot $(if($Module -in @('M3','M5','M6','M7','M8')){"Livraison-$Module"}else{"Livraison-M4-$Version"})
if($Revision){if($Revision -notmatch '^\d+\.\d+\.\d+$'){throw 'Revision invalide'};$taskDelivery+="-$Revision"}
$taskProductPrefix=if($Module -in @('M5','M6','M7','M8')){'LibreERP'}else{'SecuriteERP'}
$taskPortableFolder=Join-Path $taskDelivery 'Portable'
if(Test-Path -LiteralPath (Join-Path $taskPortableFolder 'Donnees')){throw 'Ce dossier contient des données : choisissez une nouvelle livraison.'}
New-Item -ItemType Directory -Path $taskPortableFolder -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $taskRoot 'src-tauri/target/debug/securite-erp-portable.exe') -Destination (Join-Path $taskPortableFolder "$taskProductPrefix-portable.exe") -Force
Copy-Item -LiteralPath (Join-Path $taskRoot 'src-tauri/target/debug/securite-erp-recovery.exe') -Destination (Join-Path $taskPortableFolder "$taskProductPrefix-recuperation.exe") -Force
Copy-Item -LiteralPath (Join-Path $taskDelivery 'COMMENCER-ICI.md') -Destination (Join-Path $taskPortableFolder 'LIRE-MOI.md') -Force
'{"format":1,"data_folder":"Donnees"}' | Set-Content -LiteralPath (Join-Path $taskPortableFolder 'portable.json') -Encoding UTF8
$taskRuntimeInstaller=Get-ChildItem -LiteralPath (Join-Path $env:LOCALAPPDATA 'tauri/x64') -Recurse -Filter 'MicrosoftEdgeWebView2RuntimeInstallerX64.exe' | Select-Object -First 1
if(-not $taskRuntimeInstaller){throw 'Installeur WebView2 hors ligne absent du cache Tauri. Fabriquez les bundles avant le portable.'}
$taskComponents=Join-Path $taskPortableFolder 'Composants'
New-Item -ItemType Directory -Path $taskComponents -Force | Out-Null
Copy-Item -LiteralPath $taskRuntimeInstaller.FullName -Destination (Join-Path $taskComponents $taskRuntimeInstaller.Name) -Force
$taskReport=Get-ChildItem -LiteralPath $taskPortableFolder -Recurse -File | Where-Object { $_.Name -ne 'empreintes.json' } | ForEach-Object { [pscustomobject]@{ fichier=$_.FullName.Substring($taskPortableFolder.Length+1);sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash;octets=$_.Length } }
$taskReport | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $taskPortableFolder 'empreintes.json') -Encoding UTF8
$taskZip=Join-Path $taskDelivery "$taskProductPrefix-$Version-portable.zip"
Compress-Archive -LiteralPath $taskPortableFolder -DestinationPath $taskZip -Force -CompressionLevel Optimal
Get-Item -LiteralPath $taskZip | Select-Object FullName,Length
Get-FileHash -LiteralPath $taskZip -Algorithm SHA256

