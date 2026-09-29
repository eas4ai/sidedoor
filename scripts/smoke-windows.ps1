param([string]$App = "$PSScriptRoot/../target/windows-bundle/Sidedoor")
$ErrorActionPreference = "Stop"
$App = (Resolve-Path $App).Path
$Log = Join-Path (Split-Path $App) "smoke"
New-Item -ItemType Directory -Force $Log | Out-Null

# Keep CI history and settings isolated from the runner account's normal profile.
$env:APPDATA = Join-Path $Log "roaming"
$env:LOCALAPPDATA = Join-Path $Log "local"
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class SidedoorSmoke {
    public delegate bool Callback(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumWindows(Callback callback, IntPtr data);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, UIntPtr wparam, IntPtr lparam);
    public static IntPtr[] Windows(uint pid, string title, string className) {
        var matches = new List<IntPtr>();
        EnumWindows((window, _) => {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            if (owner != pid) return true;
            var text = new StringBuilder(256);
            GetWindowText(window, text, text.Capacity);
            if (title != null && text.ToString() != title) return true;
            text.Clear();
            GetClassName(window, text, text.Capacity);
            if (className != null && text.ToString() != className) return true;
            matches.Add(window);
            return true;
        }, IntPtr.Zero);
        return matches.ToArray();
    }
}
'@
$Process = Start-Process "$App/Sidedoor.exe" -PassThru -RedirectStandardOutput "$Log/stdout.log" -RedirectStandardError "$Log/stderr.log"
try {
    $Deadline = (Get-Date).AddSeconds(30)
    do {
        Start-Sleep -Milliseconds 250
        $Process.Refresh()
        if ($Process.HasExited) { throw "Sidedoor exited during startup: $(Get-Content "$Log/stderr.log" -Raw)" }
        $MessageWindows = [SidedoorSmoke]::Windows($Process.Id, $null, "SidedoorMessages")
    } while ($MessageWindows.Count -lt 2 -and (Get-Date) -lt $Deadline)
    if ($MessageWindows.Count -lt 2) { throw "Tray and shortcut message windows were not created" }

    # Exercise the same callback used by RegisterHotKey, without sending keys to other apps.
    foreach ($Window in $MessageWindows) {
        [SidedoorSmoke]::PostMessage($Window, 0x0312, [UIntPtr]::new(1), [IntPtr]::Zero) | Out-Null
    }
    $Deadline = (Get-Date).AddSeconds(15)
    do {
        Start-Sleep -Milliseconds 250
        $History = [SidedoorSmoke]::Windows($Process.Id, "Clipboard History", $null)
    } while ($History.Count -eq 0 -and (Get-Date) -lt $Deadline)
    if ($History.Count -eq 0) { throw "Clipboard History did not open from its shortcut" }
    $Bun = Get-CimInstance Win32_Process -Filter "Name = 'bun.exe'" | Where-Object { $_.ParentProcessId -eq $Process.Id }
    if (-not $Bun) { throw "The bundled plugin supervisor did not start" }
    foreach ($Window in $History) {
        [SidedoorSmoke]::PostMessage($Window, 0x0010, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    }
    Start-Sleep -Seconds 1
    $Process.Refresh()
    if ($Process.HasExited) { throw "Closing History terminated Sidedoor" }
    Write-Host "Packaged app started, bundled Bun started, and Clipboard History opened and closed."
} finally {
    # Stop only this smoke test's process tree, including its bundled Bun child.
    if (-not $Process.HasExited) { & taskkill.exe /PID $Process.Id /T /F | Out-Null }
}
