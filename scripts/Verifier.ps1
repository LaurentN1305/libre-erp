. (Join-Path $PSScriptRoot 'Commun.ps1')
Write-Host 'Verification des outils de compilation' -ForegroundColor Cyan
foreach ($taskName in @('node','cargo','rustc','perl','nasm')) {
    $taskTool = Get-Command $taskName -ErrorAction SilentlyContinue
    if ($taskTool) { Write-Host "$taskName : $($taskTool.Source)" -ForegroundColor Green }
    else { Write-Host "$taskName : absent du PATH" -ForegroundColor Yellow }
}
$taskVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (Test-Path -LiteralPath $taskVswhere) {
    & $taskVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
} else { Write-Host 'Visual Studio Build Tools : non detecte' -ForegroundColor Yellow }
Write-Host 'Les logiciels installes chez les utilisateurs ne necessitent aucun de ces outils.'
