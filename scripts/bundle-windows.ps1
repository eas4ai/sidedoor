param([switch]$SkipBuild)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$Root = Split-Path -Parent $PSScriptRoot
$PreviousRustFlags = $env:RUSTFLAGS
Push-Location $Root
try {
    if (-not $SkipBuild) {
        # The portable app must not require installing the Visual C++ runtime.
        $env:RUSTFLAGS = "$PreviousRustFlags -C target-feature=+crt-static".Trim()
        & cargo build --release --locked --target x86_64-pc-windows-msvc
        if ($LASTEXITCODE -ne 0) { throw "Rust release build failed" }
    }
    $Out = Join-Path $Root "target/windows-bundle"
    $App = Join-Path $Out "Sidedoor"
    if (Test-Path $App) { Remove-Item -Recurse -Force $App }
    New-Item -ItemType Directory -Force "$App/resources/sdk", "$App/resources/builtins" | Out-Null
    Copy-Item "target/x86_64-pc-windows-msvc/release/sidedoor.exe" "$App/Sidedoor.exe"

    # Resolve the actual executable, not a package-manager shim.
    $Bun = (& bun -p "process.execPath").Trim()
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path $Bun)) { throw "Bun executable not found" }
    Copy-Item $Bun "$App/bun.exe"
    foreach ($Name in @("weather", "clipboard", "stats")) {
        New-Item -ItemType Directory -Force "$App/resources/builtins/$Name" | Out-Null
        & $Bun build "src/builtins/$Name/index.tsx" --target=bun --outfile "$App/resources/builtins/$Name/index.js"
        if ($LASTEXITCODE -ne 0) { throw "Bundling $Name failed" }
    }
    Copy-Item -Recurse "sdk/src" "$App/resources/sdk/src"
    Copy-Item "sdk/package.json", "sdk/tsconfig.json", "sdk/README.md" "$App/resources/sdk/"
    Copy-Item "docs/windows-testing.md" "$App/README.txt"
    $Zip = Join-Path $Out "Sidedoor-windows-x64.zip"
    if (Test-Path $Zip) { Remove-Item $Zip }
    Compress-Archive -Path $App -DestinationPath $Zip
    $Hash = (Get-FileHash -Algorithm SHA256 $Zip).Hash.ToLowerInvariant()
    Set-Content -Encoding ascii "$Zip.sha256" "$Hash  Sidedoor-windows-x64.zip"
    Write-Host "Built $Zip"
} finally {
    $env:RUSTFLAGS = $PreviousRustFlags
    Pop-Location
}
