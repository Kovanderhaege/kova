$ErrorActionPreference = 'Stop'
$scriptPath = Join-Path (Split-Path -Parent $PSScriptRoot) 'package-windows.ps1'
$workspace = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('kova-package-test-' + [guid]::NewGuid().ToString('N'))
$packageOutput = Join-Path $fixture 'packages'
$failureOutput = Join-Path $fixture 'failure-packages'
$targetDirectory = Join-Path $fixture 'custom-target'
foreach ($profile in @('debug', 'release')) {
    New-Item -ItemType Directory -Path (Join-Path $targetDirectory $profile) -Force | Out-Null
}
$global:kovaPackageBuildArguments = @()
$global:kovaPackageFailBuild = $false

function cargo {
    if ($args[0] -eq 'metadata') {
        [ordered]@{
            target_directory = $targetDirectory
            workspace_members = @('workspace-id')
            packages = @(
                [ordered]@{ id = 'workspace-id'; name = 'kova-desktop'; version = '0.1.0'; source = $null },
                [ordered]@{ id = 'dependency-id'; name = 'serde'; version = '1.0.0'; source = 'registry+https://example.invalid' }
            )
        } | ConvertTo-Json -Depth 5 -Compress
        $global:LASTEXITCODE = 0
        return
    }
    if ($args[0] -eq 'build') {
        $global:kovaPackageBuildArguments = @($args)
        if ($global:kovaPackageFailBuild) {
            $global:LASTEXITCODE = 17
            return
        }
        $profile = if ($args -contains '--release') { 'release' } else { 'debug' }
        Set-Content -LiteralPath (Join-Path $targetDirectory "$profile/kova-desktop.exe") -Value 'desktop-binary'
        Set-Content -LiteralPath (Join-Path $targetDirectory "$profile/kova-identity.exe") -Value 'identity-binary'
        $global:LASTEXITCODE = 0
        return
    }
    throw "Unexpected cargo command: $args"
}

try {
    Push-Location $workspace
    foreach ($configuration in @('Debug', 'Release')) {
        $archivePath = & $scriptPath -Configuration $configuration -OutputDirectory $packageOutput
        $expectedBuildArguments = @('build', '-p', 'kova-desktop', '-p', 'kova-client-core')
        if ($configuration -eq 'Release') { $expectedBuildArguments += '--release' }
        $expectedBuildArguments += '--locked'
        if (($global:kovaPackageBuildArguments -join '|') -ne ($expectedBuildArguments -join '|')) {
            throw "$configuration passed unexpected build args to Cargo: $($global:kovaPackageBuildArguments -join ' ')"
        }
        if (-not (Test-Path -LiteralPath $archivePath -PathType Leaf)) { throw "$configuration ZIP was not created." }

        $contents = Join-Path $fixture "contents-$configuration"
        Expand-Archive -LiteralPath $archivePath -DestinationPath $contents
        $files = Get-ChildItem -LiteralPath $contents -File
        $required = @('kova-desktop.exe', 'kova-identity.exe', 'GPUI-LICENSE-APACHE', 'THIRD-PARTY-NOTICES.md',
            'LICENSE.md', 'LICENSE-GPL-3.0', 'LICENSE-AGPL-3.0', 'LICENSE-APACHE-2.0',
            'windows-spike.md', 'windows-identity.md', 'windows-communities.md', 'windows-chat.md',
            'windows-dm.md', 'distribution.md', 'dependency-inventory.json', 'SHA256SUMS')
        foreach ($name in $required) {
            if (-not (Test-Path -LiteralPath (Join-Path $contents $name) -PathType Leaf)) {
                throw "$configuration package is missing $name."
            }
        }

        $inventory = Get-Content -LiteralPath (Join-Path $contents 'dependency-inventory.json') -Raw | ConvertFrom-Json
        if ($inventory.package_kind -ne 'unsigned-development-zip' -or $inventory.configuration -ne $configuration) {
            throw "$configuration package metadata is inaccurate."
        }
        if ($inventory.packages.name -notcontains 'serde') { throw 'Dependency inventory omitted a registry dependency.' }

        foreach ($line in Get-Content -LiteralPath (Join-Path $contents 'SHA256SUMS')) {
            $parts = $line -split '  ', 2
            $actual = (Get-FileHash -LiteralPath (Join-Path $contents $parts[1]) -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($actual -ne $parts[0]) { throw "$configuration checksum mismatch for $($parts[1])." }
        }
        if ($files.Count -ne $required.Count) { throw "$configuration checksum manifest has unexpected coverage." }

        $archiveFullPath = [IO.Path]::GetFullPath($archivePath)
        $outputFullPath = [IO.Path]::GetFullPath($packageOutput)
        if ([IO.Path]::GetDirectoryName($archiveFullPath) -ne $outputFullPath -or
            [IO.Path]::GetFileName($archiveFullPath) -notmatch '^windows-development-[0-9a-f]{32}\.zip$') {
            throw "Refusing cleanup outside this test's generated package directory: $archiveFullPath"
        }
    }
    $global:kovaPackageFailBuild = $true
    try {
        & $scriptPath -Configuration Debug -OutputDirectory $failureOutput | Out-Null
        throw 'A failed Cargo build unexpectedly created a package.'
    } catch {
        if ($_.Exception.Message -eq 'A failed Cargo build unexpectedly created a package.') { throw }
        if ($_.Exception.Message -notmatch 'Windows build failed') { throw }
    }
    if (Test-Path -LiteralPath $failureOutput -PathType Container) {
        $failedPackageFiles = @(Get-ChildItem -LiteralPath $failureOutput -Force)
        if ($failedPackageFiles.Count -ne 0) { throw 'Cargo build failure left package artifacts.' }
    }
    Write-Output 'OK: Debug/Release args, failed-build handling, package content, licenses, inventory and SHA-256 verified.'
} finally {
    Pop-Location
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    $fixtureFullPath = [IO.Path]::GetFullPath($fixture)
    if (-not $fixtureFullPath.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -or
        [IO.Path]::GetFileName($fixtureFullPath) -notmatch '^kova-package-test-[0-9a-f]{32}$') {
        throw "Refusing cleanup outside the expected test fixture under TEMP: $fixtureFullPath"
    }
    if (Test-Path -LiteralPath $fixtureFullPath -PathType Container) {
        Remove-Item -LiteralPath $fixtureFullPath -Recurse -Force
    }
    Remove-Variable -Name kovaPackageBuildArguments -Scope Global -ErrorAction SilentlyContinue
    Remove-Variable -Name kovaPackageFailBuild -Scope Global -ErrorAction SilentlyContinue
}
