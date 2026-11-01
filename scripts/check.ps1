param([switch]$Offline)
$ErrorActionPreference = 'Stop'
$taskRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location -LiteralPath $taskRoot
try {
    & cargo fmt --all -- --check
    if ($LASTEXITCODE -ne 0) { throw 'Formatting failed.' }
    & python -m unittest discover -s (Join-Path $PSScriptRoot '.') -p test_generate_icon.py
    if ($LASTEXITCODE -ne 0) { throw 'Icon verification regressions failed.' }
    & python (Join-Path $PSScriptRoot 'generate-icon.py') --check
    if ($LASTEXITCODE -ne 0) { throw 'Icon assets are stale.' }
    & python (Join-Path $PSScriptRoot 'check-software-backend.py')
    if ($LASTEXITCODE -ne 0) { throw 'Software backend dependency check failed.' }
    $cargoArgs = @('--locked', '--jobs', '2', '--target-dir', (Join-Path $taskRoot 'target'))
    if ($Offline) { $cargoArgs += '--offline' }
    & cargo clippy @cargoArgs --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Clippy failed.' }
    & cargo test @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw 'Tests failed.' }
} finally { Pop-Location }
