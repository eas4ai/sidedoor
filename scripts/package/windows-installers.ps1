param([string]$Version = $env:SIDEDOOR_VERSION)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Out = Join-Path $Root "target/windows-bundle"
$App = Join-Path $Out "Sidedoor"
if (-not $Version) {
    $Version = [regex]::Match((Get-Content "$Root/Cargo.toml" -Raw), '(?m)^version = "([^"]+)"').Groups[1].Value
}
if ($Version -notmatch '^(\d+)\.(\d+)\.(\d+)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$') {
    throw "Expected a semantic release version, got: $Version"
}
# Windows Installer compares three numeric fields, not SemVer prerelease labels.
$MsiVersion = "$($Matches[1]).$($Matches[2]).$($Matches[3])"
if ([int]$Matches[1] -gt 255 -or [int]$Matches[2] -gt 255 -or [int]$Matches[3] -gt 65535) {
    throw "Version exceeds Windows Installer's 255.255.65535 limit"
}
foreach ($File in @("Sidedoor.exe", "bun.exe", "resources/sdk/package.json", "resources/builtins/weather/index.js", "resources/builtins/clipboard/index.js", "resources/builtins/stats/index.js")) {
    if (-not (Test-Path "$App/$File" -PathType Leaf)) { throw "Missing Windows payload: $File" }
}
# Use the same pinned WiX release locally and in CI; no global install needed.
$Wix = Join-Path $Root "target/tools/wix-3.14.1"
if (-not (Test-Path "$Wix/light.exe")) {
    New-Item -ItemType Directory -Force $Wix | Out-Null
    $Download = Join-Path $Root "target/tools/wix-3.14.1.zip"
    Invoke-WebRequest "https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip" -OutFile $Download
    Expand-Archive $Download -DestinationPath $Wix -Force
}
$Work = Join-Path $Out "wix"
if (Test-Path $Work) { Remove-Item -Recurse -Force $Work }
New-Item -ItemType Directory -Force $Work | Out-Null
$Icon = Join-Path $Root "crates/desktop/assets/icons/icon.ico"
$Msi = Join-Path $Out "Sidedoor-windows-x64.msi"
$Setup = Join-Path $Out "Sidedoor-windows-x64-setup.exe"
$Marker = Join-Path $Work "installation-marker.txt"
Set-Content -Encoding ascii $Marker 'Sidedoor Windows Installer installation'
& "$Wix/heat.exe" dir $App -nologo -ag -srd -sreg -scom -dr INSTALLFOLDER -cg AppFiles -var var.SourceDir -t "$PSScriptRoot/windows/Shortcuts.xsl" -out "$Work/Files.wxs"
if ($LASTEXITCODE -ne 0) { throw "WiX payload harvesting failed" }
& "$Wix/candle.exe" -nologo -arch x64 "-dSourceDir=$App" "-dVersion=$MsiVersion" "-dIcon=$Icon" "-dMarker=$Marker" -out "$Work/" "$PSScriptRoot/windows/Product.wxs" "$Work/Files.wxs"
if ($LASTEXITCODE -ne 0) { throw "WiX MSI compilation failed" }
& "$Wix/light.exe" -nologo -out $Msi "$Work/Product.wixobj" "$Work/Files.wixobj"
if ($LASTEXITCODE -ne 0) { throw "WiX MSI linking failed" }
& "$Wix/candle.exe" -nologo -ext "$Wix/WixBalExtension.dll" "-dVersion=$MsiVersion" "-dMsi=$Msi" "-dIcon=$Icon" -out "$Work/" "$PSScriptRoot/windows/Bundle.wxs"
if ($LASTEXITCODE -ne 0) { throw "WiX setup compilation failed" }
& "$Wix/light.exe" -nologo -ext "$Wix/WixBalExtension.dll" -out $Setup "$Work/Bundle.wixobj"
if ($LASTEXITCODE -ne 0) { throw "WiX setup linking failed" }
foreach ($File in @($Msi, $Setup)) {
    $Hash = (Get-FileHash -Algorithm SHA256 $File).Hash.ToLowerInvariant()
    Set-Content -Encoding ascii "$File.sha256" "$Hash  $([IO.Path]::GetFileName($File))"
    Write-Host "Built $File ($Version)"
}
