# Development ZIP only: unsigned, not a release installer.
$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
Push-Location $workspace
try {
    & cargo build -p kova-desktop -p kova-client-core --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Windows build failed.' }
    $output = Join-Path $workspace ('dist/windows-spike-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $output | Out-Null
    Copy-Item -LiteralPath 'target/release/kova-desktop.exe', 'target/release/kova-identity.exe', 'apps/desktop/GPUI-LICENSE-APACHE', 'apps/desktop/THIRD-PARTY-NOTICES.md', 'docs/windows-spike.md' -Destination $output
    & cargo metadata --locked --format-version 1 | Set-Content -LiteralPath (Join-Path $output 'dependency-metadata.json') -Encoding utf8
    if ($LASTEXITCODE -ne 0) { throw 'Dependency inventory failed.' }
    Compress-Archive -LiteralPath $output -DestinationPath "$output.zip"
    Write-Output "$output.zip"
} finally { Pop-Location }
