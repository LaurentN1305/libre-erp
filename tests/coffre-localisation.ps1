$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot '../scripts/Resoudre-Coffre-Signature.ps1')
$taskTestRoot=Join-Path ([IO.Path]::GetTempPath()) ('libreerp-vault-location-'+[guid]::NewGuid())
try{
 New-Item -ItemType Directory -Path $taskTestRoot | Out-Null
 if(Resolve-LibreErpSigningVault -RacineLocale $taskTestRoot){throw 'Un coffre inexistant ne doit pas etre selectionne'}
 $taskFallback=Join-Path $taskTestRoot 'Packages/OpenAI.Codex_test/LocalCache/Local/LibreERP-Editeur'
 New-Item -ItemType Directory -Path $taskFallback -Force | Out-Null
 [IO.File]::WriteAllBytes((Join-Path $taskFallback 'signatures.dpapi'),[byte[]]@(1,2,3))
 if((Resolve-LibreErpSigningVault -RacineLocale $taskTestRoot) -ne $taskFallback){throw 'Le coffre MSIX doit etre retrouve'}
 if(Resolve-LibreErpSigningVault -RacineLocale $taskTestRoot -Dossier (Join-Path $taskTestRoot 'personnalise')){throw 'Pas de repli pour un dossier personnalise'}
 $taskStandard=Join-Path $taskTestRoot 'LibreERP-Editeur'
 New-Item -ItemType Directory -Path $taskStandard | Out-Null
 [IO.File]::WriteAllBytes((Join-Path $taskStandard 'signatures.dpapi'),[byte[]]@(4,5,6))
 if((Resolve-LibreErpSigningVault -RacineLocale $taskTestRoot) -ne $taskStandard){throw 'Le dossier standard doit rester prioritaire'}
 Write-Output 'Localisation : absence, coffre Codex, dossier personnalise et priorite standard : OK'
}finally{
 $taskResolved=[IO.Path]::GetFullPath($taskTestRoot)
 if($taskResolved.StartsWith([IO.Path]::GetFullPath([IO.Path]::GetTempPath()),[StringComparison]::OrdinalIgnoreCase) -and (Split-Path $taskResolved -Leaf).StartsWith('libreerp-vault-location-')){Remove-Item -LiteralPath $taskResolved -Recurse -Force}
}
