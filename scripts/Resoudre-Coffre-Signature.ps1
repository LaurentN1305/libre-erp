# Locate the existing encrypted vault, including Windows MSIX AppData redirection.
# This helper never reads private key contents, creates keys, or moves the vault.
function Resolve-LibreErpSigningVault {
    param([string]$Dossier,[string]$RacineLocale=$env:LOCALAPPDATA)
    $taskStandard=[IO.Path]::GetFullPath((Join-Path $RacineLocale 'LibreERP-Editeur'))
    $taskRequested=if($Dossier){[IO.Path]::GetFullPath($Dossier)}else{$taskStandard}
    if(Test-Path -LiteralPath (Join-Path $taskRequested 'signatures.dpapi') -PathType Leaf){return $taskRequested}
    # A missing explicitly chosen custom folder must not select another publisher's vault.
    if(-not [string]::Equals($taskRequested.TrimEnd('\'),$taskStandard.TrimEnd('\'),[StringComparison]::OrdinalIgnoreCase)){return $null}
    $taskPackages=Join-Path $RacineLocale 'Packages'
    $taskFound=@()
    if(Test-Path -LiteralPath $taskPackages -PathType Container){
        foreach($taskPackage in Get-ChildItem -LiteralPath $taskPackages -Directory -Filter 'OpenAI.Codex_*' -ErrorAction SilentlyContinue){
            $taskCandidate=Join-Path $taskPackage.FullName 'LocalCache\Local\LibreERP-Editeur'
            if(Test-Path -LiteralPath (Join-Path $taskCandidate 'signatures.dpapi') -PathType Leaf){$taskFound+= $taskCandidate}
        }
    }
    if($taskFound.Count -gt 1){throw 'Plusieurs coffres de signature existent. Aucun choix automatique : indiquez le dossier du coffre correspondant aux cles publiques publiees.'}
    if($taskFound.Count -eq 1){return $taskFound[0]}
    return $null
}
