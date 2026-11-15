[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Release',
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
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
    & cargo @buildArguments
    if ($LASTEXITCODE -ne 0) { throw 'Windows build failed.' }

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
    $output = Join-Path $outputRoot ('windows-development-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $output -Force | Out-Null
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

        $inventory = [ordered]@{
            configuration = $Configuration
            package_kind = 'unsigned-development-zip'
            generated_at_utc = [DateTime]::UtcNow.ToString('o')
            workspace_packages = @($metadata.workspace_members | ForEach-Object {
                    $workspaceId = $_
                    $metadata.packages | Where-Object { $_.id -eq $workspaceId } | ForEach-Object { $_.name }
                })
            packages = @($metadata.packages | ForEach-Object {
                    [ordered]@{ name = $_.name; version = $_.version; source = $_.source }
                })
        }
        $inventory | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $output 'dependency-inventory.json') -Encoding utf8

        $hashes = Get-ChildItem -LiteralPath $output -File | Sort-Object Name | ForEach-Object {
            '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $_.Name
        }
        $hashes | Set-Content -LiteralPath (Join-Path $output 'SHA256SUMS') -Encoding utf8
        $archive = "$output.zip"
        Compress-Archive -Path (Join-Path $output '*') -DestinationPath $archive
        Write-Output $archive
    } catch {
        throw "Package assembly failed. Partial staging directory: $output. $($_.Exception.Message)"
    }
} finally {
    Pop-Location
}
