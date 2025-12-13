param([switch]$Offline)
$ErrorActionPreference = 'Stop'
$taskRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location -LiteralPath $taskRoot
try {
    $hostInfo = & rustc -vV
    if ($LASTEXITCODE -ne 0 -or -not ($hostInfo -contains 'host: x86_64-pc-windows-msvc')) {
        throw 'Packaging currently requires the x86_64-pc-windows-msvc host toolchain.'
    }
    $cargoArgs = @('build', '--locked', '--release', '--jobs', '2', '--target-dir', (Join-Path $taskRoot 'target'))
    if ($Offline) { $cargoArgs += '--offline' }
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
    $metadata = (& cargo metadata --offline --no-deps --format-version 1 | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read package version.' }
    $version = ($metadata.packages | Where-Object name -eq 'print-helper').version
    if (-not $version) { throw 'PrintHelper package was not found.' }
    $dist = Join-Path $taskRoot 'dist'
    New-Item -ItemType Directory -Force -Path $dist | Out-Null
    $name = "PrintHelper-v$version.exe"
    $destination = Join-Path $dist $name
    Copy-Item -LiteralPath (Join-Path $taskRoot 'target\release\print-helper.exe') -Destination $destination
    $versionInfo = (Get-Item -LiteralPath $destination).VersionInfo
    if ($versionInfo.ProductVersion -ne $version -or $versionInfo.FileVersion -ne $version -or
        $versionInfo.OriginalFilename -ne $name) {
        throw 'Executable version resources do not match the Cargo version and asset name.'
    }
    & python (Join-Path $PSScriptRoot 'inspect-pe.py') $destination
    if ($LASTEXITCODE -ne 0) { throw 'Packaged executable inspection failed.' }
    $digest = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
    "$digest  $name" | Set-Content -LiteralPath (Join-Path $dist 'SHA256SUMS.txt') -Encoding ascii
    Copy-Item -LiteralPath (Join-Path $taskRoot 'LICENSE') -Destination (Join-Path $dist 'LICENSE.txt')
    Copy-Item -LiteralPath (Join-Path $taskRoot 'THIRD_PARTY_NOTICES.md') -Destination $dist
    Write-Output "Packaged $destination"
    Write-Output "Verified product/file version $version and original filename $name"
    Write-Output "SHA256 $digest"
} finally { Pop-Location }
