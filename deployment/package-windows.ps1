[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Release',
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
function Get-KovaSourceFingerprint {
    $paths = & git -c core.quotepath=false ls-files
    if ($LASTEXITCODE -ne 0) { throw 'Cannot inventory tracked build sources.' }
    @($paths | ForEach-Object {
        if (Test-Path -LiteralPath $_ -PathType Leaf) {
            '{0}  {1}' -f (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash, $_
        }
    }) -join "`n"
}
$workspace = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $workspace 'dist'
}
$outputRoot = if ([IO.Path]::IsPathRooted($OutputDirectory)) {
    [IO.Path]::GetFullPath($OutputDirectory)
} else {
    [IO.Path]::GetFullPath((Join-Path $workspace $OutputDirectory))
}
New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
Push-Location $workspace
try {
    $metadataJson = & cargo metadata --locked --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed.' }
    $metadata = $metadataJson | ConvertFrom-Json
    $targetDirectory = $metadata.target_directory
    if ([string]::IsNullOrWhiteSpace($targetDirectory)) {
        throw 'Cargo metadata did not provide target_directory.'
    }

    $buildArguments = @('build', '-p', 'kova-desktop', '-p', 'kova-client-core')
    if ($Configuration -eq 'Release') { $buildArguments += '--release' }
    $buildArguments += '--locked'
    $buildSourceFingerprint = Get-KovaSourceFingerprint
    & cargo @buildArguments
    if ($LASTEXITCODE -ne 0) { throw 'Windows build failed.' }

    $output = Join-Path $outputRoot ('windows-development-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $output -Force | Out-Null
    $complianceDirectory = Join-Path $output 'compliance'
    & python (Join-Path $PSScriptRoot 'compliance.py') --output $complianceDirectory
    if ($LASTEXITCODE -ne 0) { throw 'Distribution compliance material generation failed.' }
    if (-not (Test-Path -LiteralPath $complianceDirectory -PathType Container)) {
        throw 'Distribution compliance material was not generated.'
    }
    if ($buildSourceFingerprint -ne (Get-KovaSourceFingerprint)) {
        throw 'Tracked sources changed during build/source assembly; rebuild before packaging.'
    }

    $artifactDirectory = Join-Path $targetDirectory $Configuration.ToLowerInvariant()
    $artifacts = @(
        'kova-desktop.exe',
        'kova-identity.exe',
        'LICENSE.md',
        'LICENSE-GPL-3.0',
        'LICENSE-AGPL-3.0',
        'LICENSE-APACHE-2.0',
        'apps/desktop/GPUI-LICENSE-APACHE',
        'apps/desktop/THIRD-PARTY-NOTICES.md',
        'docs/windows-spike.md',
        'docs/windows-identity.md',
        'docs/windows-communities.md',
        'docs/windows-chat.md',
        'docs/windows-dm.md',
        'docs/distribution.md'
    )
    try {
        foreach ($artifact in $artifacts) {
            if ($artifact.EndsWith('.exe')) {
                $source = Join-Path $artifactDirectory $artifact
                $destinationName = $artifact
            } else {
                $source = Join-Path $workspace $artifact
                $destinationName = Split-Path -Leaf $artifact
            }
            if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
                throw "Required package file is missing: $source"
            }
            Copy-Item -LiteralPath $source -Destination (Join-Path $output $destinationName)
        }

        Get-ChildItem -LiteralPath $complianceDirectory -Force | Move-Item -Destination $output
        Remove-Item -LiteralPath $complianceDirectory
        $inventoryPath = Join-Path $output 'dependency-inventory.json'
        $inventory = Get-Content -LiteralPath $inventoryPath -Raw | ConvertFrom-Json -AsHashtable
        $inventory['configuration'] = $Configuration
        $inventory['package_kind'] = 'unsigned-development-zip'
        $inventory | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $inventoryPath -Encoding utf8

        $checksumPath = Join-Path $output 'SHA256SUMS'
        $hashes = Get-ChildItem -LiteralPath $output -File -Recurse |
            Where-Object { $_.FullName -ne $checksumPath } |
            Sort-Object { [IO.Path]::GetRelativePath($output, $_.FullName).Replace('\', '/') } |
            ForEach-Object {
                $relativePath = [IO.Path]::GetRelativePath($output, $_.FullName).Replace('\', '/')
                '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $relativePath
        }
        $hashes | Set-Content -LiteralPath $checksumPath -Encoding utf8
        $archive = "$output.zip"
        Compress-Archive -Path (Join-Path $output '*') -DestinationPath $archive
        Write-Output $archive
    } catch {
        throw "Package assembly failed. Partial staging directory: $output. $($_.Exception.Message)"
    }
} finally {
    Pop-Location
}
