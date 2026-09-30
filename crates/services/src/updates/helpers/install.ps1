param([int]$ParentPid, [string]$Destination, [string]$Payload, [string]$Executable,
      [string]$Receipt, [string]$Version, [string]$Kind, [string]$ErrorFile,
      [string]$ExpectedHash, [string]$Package, [string]$Ready)
$ErrorActionPreference = 'Stop'
$Stage = Split-Path $Receipt
$Backup = "$Destination.sidedoor-backup"
$Next = "$Destination.sidedoor-next"
$Replaced = $false
$OwnsNext = $false
$Exited = $false
$NewProcess = $null
try {
    New-Item -ItemType File -Path $Ready -Force | Out-Null
    $ParentProcess = Get-Process -Id $ParentPid -ErrorAction SilentlyContinue
    if ($ParentProcess -and -not $ParentProcess.WaitForExit(60000)) { throw 'Sidedoor did not exit' }
    $Exited = $true
    if ((Get-FileHash $Package -Algorithm SHA256).Hash.ToLowerInvariant() -ne $ExpectedHash) { throw 'Update checksum changed' }
    if (Test-Path $ErrorFile) { Remove-Item $ErrorFile }
    switch ($Kind) {
        'portable' {
            if ((Test-Path $Backup) -or (Test-Path $Next)) { throw 'A previous update backup needs recovery' }
            $OwnsNext = $true
            Copy-Item -Recurse $Payload $Next
            Move-Item $Destination $Backup
            $Replaced = $true
            Move-Item $Next $Destination
        }
        'msi' {
            $Installer = Start-Process msiexec.exe -Verb RunAs -ArgumentList "/i `"$Payload`" /passive /norestart" -PassThru -Wait
            if ($Installer.ExitCode -notin @(0, 3010)) { throw "Windows Installer failed ($($Installer.ExitCode))" }
        }
        'setup' {
            $Installer = Start-Process $Payload -Verb RunAs -ArgumentList '/passive /norestart' -PassThru -Wait
            if ($Installer.ExitCode -notin @(0, 3010)) { throw "Setup failed ($($Installer.ExitCode))" }
        }
        default { throw 'Unknown installation type' }
    }
    $env:SIDEDOOR_UPDATE_RECEIPT = $Receipt
    $NewProcess = Start-Process $Executable -PassThru
    $Deadline = (Get-Date).AddSeconds(60)
    do {
        if ((Test-Path $Receipt) -and (Get-Content $Receipt -Raw).Trim() -eq $Version) { break }
        if ($NewProcess.HasExited -or (Get-Date) -gt $Deadline) { throw 'The updated app did not start' }
        Start-Sleep -Milliseconds 250
    } while ($true)
    if ($Replaced) { Remove-Item -Recurse -Force $Backup }
    Write-Output "Updated to $Version"
} catch {
    $_ | Out-String | Set-Content $ErrorFile
    if ($NewProcess -and -not $NewProcess.HasExited) { Stop-Process -Id $NewProcess.Id -Force }
    if ($Replaced -and (Test-Path $Backup)) {
        if (Test-Path $Destination) { Remove-Item -Recurse -Force $Destination }
        Move-Item $Backup $Destination
    }
    Remove-Item Env:SIDEDOOR_UPDATE_RECEIPT -ErrorAction SilentlyContinue
    if ($Exited -and (Test-Path $Executable)) { Start-Process $Executable | Out-Null }
    exit 1
} finally {
    if ($OwnsNext -and (Test-Path $Next)) { Remove-Item -Recurse -Force $Next }
    Remove-Item -Recurse -Force $Stage
}
