# For an isolated, elevated Windows CI runner only.
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Out = Join-Path $Root "target/windows-bundle"
$Installed = Join-Path $env:ProgramFiles "Sidedoor"
$Shortcut = Join-Path ([Environment]::GetFolderPath("CommonPrograms")) "Sidedoor/Sidedoor.lnk"
$Logs = Join-Path $Out "installer-logs"
New-Item -ItemType Directory -Force $Logs | Out-Null
function Invoke-Installer([string]$Executable, [string]$Arguments) {
    $Process = Start-Process $Executable -ArgumentList $Arguments -PassThru
    if (-not $Process.WaitForExit(60000)) { throw "Installer timed out: $Executable" }
    if ($Process.ExitCode -notin @(0, 3010)) { throw "Installer failed ($($Process.ExitCode)): $Executable" }
}
function Assert-Installed {
    # Every staged file must survive installation, including the entire SDK.
    $Payload = Join-Path $Out "Sidedoor"
    foreach ($File in Get-ChildItem $Payload -Recurse -File) {
        $Relative = [IO.Path]::GetRelativePath($Payload, $File.FullName)
        $Destination = Join-Path $Installed $Relative
        if (-not (Test-Path $Destination -PathType Leaf)) { throw "Installer omitted $Relative" }
        if ((Get-FileHash $File.FullName).Hash -ne (Get-FileHash $Destination).Hash) { throw "Installed file differs: $Relative" }
    }
    if (-not (Test-Path $Shortcut)) { throw "Start menu shortcut missing" }
    if (-not (Test-Path "$Installed/.sidedoor-msi")) { throw "Installer ownership marker missing" }
    & "$Installed/bun.exe" --version
    if ($LASTEXITCODE -ne 0) { throw "Installed Bun cannot run" }
}
function Assert-Uninstalled {
    if ((Test-Path "$Installed/Sidedoor.exe") -or (Test-Path "$Installed/bun.exe") -or (Test-Path "$Installed/resources") -or (Test-Path $Shortcut)) {
        throw "Uninstall left application files or its shortcut behind"
    }
}
$Msi = Join-Path $Out "Sidedoor-windows-x64.msi"
$Setup = Join-Path $Out "Sidedoor-windows-x64-setup.exe"
foreach ($File in @($Msi, $Setup)) {
    $Expected = ((Get-Content "$File.sha256" -Raw).Trim() -split '\s+')[0]
    if ((Get-FileHash $File -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Expected) { throw "Checksum mismatch: $File" }
}
try {
    Invoke-Installer "msiexec.exe" "/i `"$Msi`" /qn /norestart /l*v `"$Logs/msi-install.log`""
    Assert-Installed
    if (Test-Path "$Installed/.sidedoor-bundle") { throw "MSI misidentified as a setup installation" }
    & "$PSScriptRoot/windows.ps1" -App $Installed -Log "$Out/installer-smoke"
} finally {
    Invoke-Installer "msiexec.exe" "/x `"$Msi`" /qn /norestart /l*v `"$Logs/msi-uninstall.log`""
}
Assert-Uninstalled
try {
    Invoke-Installer $Setup "/quiet /norestart /log `"$Logs/setup-install.log`""
    Assert-Installed
    if (-not (Test-Path "$Installed/.sidedoor-bundle")) { throw "Setup ownership marker missing" }
} finally {
    Invoke-Installer $Setup "/uninstall /quiet /norestart /log `"$Logs/setup-uninstall.log`""
}
Assert-Uninstalled
Write-Host "MSI and setup EXE installed the full payload, created the shortcut, and uninstalled."
