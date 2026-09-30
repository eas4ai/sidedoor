param([string]$App = "$PSScriptRoot/../../target/windows-bundle/Sidedoor", [string]$Log = "")
$ErrorActionPreference = "Stop"
$App = (Resolve-Path $App).Path
if (-not $Log) { $Log = Join-Path (Split-Path $App) "smoke" }
# A restored build cache must not supply stale screenshots or a previous profile.
if (Test-Path $Log) { Remove-Item -Recurse -Force $Log }
New-Item -ItemType Directory -Force $Log | Out-Null

# Keep CI history and settings isolated from the runner account's normal profile.
$env:APPDATA = Join-Path $Log "roaming"
$env:LOCALAPPDATA = Join-Path $Log "local"

# Widgets come from the plugin gallery. Install the official Clipboard and
# Stats plugins from this checkout, as the gallery would, and put them in
# the dock.
$Support = Join-Path $env:APPDATA "Sidedoor"
$Repo = (Resolve-Path "$PSScriptRoot/../..").Path
foreach ($Name in @("clipboard", "stats")) {
    New-Item -ItemType Directory -Force "$Support/plugins/$Name" | Out-Null
    Get-ChildItem "$Repo/plugins/$Name" -Exclude node_modules |
        Copy-Item -Destination "$Support/plugins/$Name" -Recurse
}
$Config = @{
    items = @(@{ type = "plugin"; id = "clipboard" }, @{ type = "plugin"; id = "stats" })
    trusted_plugins = @("clipboard", "stats")
} | ConvertTo-Json -Depth 4
# UTF-8 without a byte-order mark, which JSON readers reject.
[System.IO.File]::WriteAllText("$Support/config.json", $Config)
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class SidedoorSmoke {
    [StructLayout(LayoutKind.Sequential)] public struct Bounds { public int Left, Top, Right, Bottom; }
    public delegate bool Callback(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumWindows(Callback callback, IntPtr data);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, UIntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Bounds bounds);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window, out Bounds bounds);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window, int command);
    [DllImport("user32.dll")] static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);
    public static void OpenHistory() {
        // This script runs only on an isolated Windows CI desktop.
        // Real key events also exercise RegisterHotKey and foreground permission.
        keybd_event(0x11, 0, 0, UIntPtr.Zero);
        keybd_event(0x12, 0, 0, UIntPtr.Zero);
        keybd_event(0x56, 0, 0, UIntPtr.Zero);
        keybd_event(0x56, 0, 2, UIntPtr.Zero);
        keybd_event(0x12, 0, 2, UIntPtr.Zero);
        keybd_event(0x11, 0, 2, UIntPtr.Zero);
    }
    public static bool IsBorderless(IntPtr window) {
        Bounds outer, client;
        return GetWindowRect(window, out outer) && GetClientRect(window, out client)
            && outer.Right - outer.Left == client.Right - client.Left
            && outer.Bottom - outer.Top == client.Bottom - client.Top;
    }
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
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
function Save-Screen([string]$Name, [System.Drawing.Rectangle]$Bounds) {
    $Bitmap = New-Object System.Drawing.Bitmap($Bounds.Width, $Bounds.Height)
    $Graphics = [System.Drawing.Graphics]::FromImage($Bitmap)
    try {
        $Graphics.CopyFromScreen($Bounds.Location, [System.Drawing.Point]::Empty, $Bounds.Size)
        $Bitmap.Save((Join-Path $Log "$Name.png"))
    } finally { $Graphics.Dispose(); $Bitmap.Dispose() }
}
$Process = Start-Process "$App/Sidedoor.exe" -PassThru -RedirectStandardOutput "$Log/stdout.log" -RedirectStandardError "$Log/stderr.log"
try {
    $Deadline = (Get-Date).AddSeconds(30)
    do {
        Start-Sleep -Milliseconds 250
        $Process.Refresh()
        if ($Process.HasExited) { throw "Sidedoor exited during startup: $(Get-Content "$Log/stderr.log" -Raw)" }
        # PowerShell converts $null to "" for .NET string parameters.
        $MessageWindows = [SidedoorSmoke]::Windows($Process.Id, [NullString]::Value, "SidedoorMessages")
    } while ($MessageWindows.Count -lt 2 -and (Get-Date) -lt $Deadline)
    if ($MessageWindows.Count -lt 2) { throw "Tray and shortcut message windows were not created" }

    foreach ($Text in @("First clipboard entry", "https://example.com", "Windows clipboard — æøå 日本語", "A longer clipboard entry to check truncation and spacing", "Fifth clipboard entry")) {
        Set-Clipboard -Value $Text
        Start-Sleep -Milliseconds 650
    }

    $Previous = [SidedoorSmoke]::GetForegroundWindow()
    [SidedoorSmoke]::OpenHistory()
    $Deadline = (Get-Date).AddSeconds(15)
    do {
        Start-Sleep -Milliseconds 250
        $History = [SidedoorSmoke]::Windows($Process.Id, "Clipboard History", [NullString]::Value)
    } while ($History.Count -eq 0 -and (Get-Date) -lt $Deadline)
    if ($History.Count -eq 0) { throw "Clipboard History did not open from its shortcut" }
    Start-Sleep -Seconds 1
    if ([SidedoorSmoke]::GetForegroundWindow() -ne $History[0]) {
        throw "Clipboard History did not receive keyboard focus from its shortcut"
    }
    $Bounds = New-Object SidedoorSmoke+Bounds
    [SidedoorSmoke]::GetWindowRect($History[0], [ref]$Bounds) | Out-Null
    Save-Screen "history" ([System.Drawing.Rectangle]::FromLTRB($Bounds.Left, $Bounds.Top, $Bounds.Right, $Bounds.Bottom))
    $Bun = Get-CimInstance Win32_Process -Filter "Name = 'bun.exe'" | Where-Object { $_.ParentProcessId -eq $Process.Id }
    if (-not $Bun) { throw "The bundled plugin supervisor did not start" }
    foreach ($Window in $History) {
        [SidedoorSmoke]::PostMessage($Window, 0x0010, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    }
    Start-Sleep -Seconds 1
    $Process.Refresh()
    if ($Process.HasExited) { throw "Closing History terminated Sidedoor" }
    if ([SidedoorSmoke]::GetForegroundWindow() -ne $Previous) {
        throw "Closing History did not restore the previous window's focus"
    }
    # Keep the runner console from covering later UI captures.
    [SidedoorSmoke]::ShowWindow($Previous, 6) | Out-Null
    $Screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    [SidedoorSmoke]::SetCursorPos($Screen.Right - 1, $Screen.Height / 2) | Out-Null
    Start-Sleep -Seconds 1
    $Dock = $null
    foreach ($Window in [SidedoorSmoke]::Windows($Process.Id, "", [NullString]::Value)) {
        if (-not [SidedoorSmoke]::IsWindowVisible($Window)) { continue }
        [SidedoorSmoke]::GetWindowRect($Window, [ref]$Bounds) | Out-Null
        $Width = $Bounds.Right - $Bounds.Left
        if ($Width -ge 50 -and $Width -le 150 -and $Bounds.Bottom - $Bounds.Top -gt $Width) {
            if (-not [SidedoorSmoke]::IsBorderless($Window)) { throw "Dock has an unwanted native frame" }
            $Scale = [SidedoorSmoke]::GetDpiForWindow($Window) / 96
            if ($Width -ne [Math]::Round(60 * $Scale)) { throw "Dock width is $Width; expected $([Math]::Round(60 * $Scale))" }
            $Dock = $Bounds
            break
        }
    }
    if (-not $Dock) { throw "Dock did not reveal at the screen edge" }
    # Stats is the final default slot; Clipboard is immediately above it.
    foreach ($Widget in @(@("stats", 34), @("clipboard", 86))) {
        [SidedoorSmoke]::SetCursorPos(($Dock.Left + $Dock.Right) / 2, $Dock.Bottom - $Widget[1] * $Scale) | Out-Null
        Start-Sleep -Seconds 2
        Save-Screen $Widget[0] $Screen
        $CardFound = $false
        foreach ($Window in [SidedoorSmoke]::Windows($Process.Id, "", [NullString]::Value)) {
            if (-not [SidedoorSmoke]::IsWindowVisible($Window)) { continue }
            [SidedoorSmoke]::GetWindowRect($Window, [ref]$Bounds) | Out-Null
            if ($Bounds.Right - $Bounds.Left -gt 200) {
                if (-not [SidedoorSmoke]::IsBorderless($Window)) { throw "$($Widget[0]) card has an unwanted native frame" }
                if ($Bounds.Right - $Bounds.Left -ne [Math]::Round(308 * $Scale)) { throw "$($Widget[0]) card width does not match its declared size" }
                $CardFound = $true
            }
        }
        if (-not $CardFound) { throw "$($Widget[0]) card did not open on hover" }
    }
    [SidedoorSmoke]::SetCursorPos(10, 10) | Out-Null
    Start-Sleep -Seconds 1
    $Second = Start-Process "$App/Sidedoor.exe" -PassThru
    if (-not $Second.WaitForExit(10000)) {
        Stop-Process -Id $Second.Id -Force
        throw "Launching again created a second app instance"
    }
    if ($Second.ExitCode -ne 0) { throw "Second launch failed" }
    $Deadline = (Get-Date).AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 250
        $Settings = [SidedoorSmoke]::Windows($Process.Id, "General", [NullString]::Value)
    } while ($Settings.Count -eq 0 -and (Get-Date) -lt $Deadline)
    if ($Settings.Count -eq 0) { throw "Second launch did not open existing app settings" }
    Start-Sleep -Seconds 1
    [SidedoorSmoke]::GetWindowRect($Settings[0], [ref]$Bounds) | Out-Null
    Save-Screen "settings" ([System.Drawing.Rectangle]::FromLTRB($Bounds.Left, $Bounds.Top, $Bounds.Right, $Bounds.Bottom))
    Write-Host "Packaged app and bundled Bun started; History opened and closed; repeat launch opened Settings."
} catch {
    try {
        Save-Screen "failure" ([System.Windows.Forms.Screen]::PrimaryScreen.Bounds)
    } catch { Write-Warning "Could not capture failure screenshot: $_" }
    throw
} finally {
    # Stop only this smoke test's process tree, including its bundled Bun child.
    if (-not $Process.HasExited) { & taskkill.exe /PID $Process.Id /T /F | Out-Null }
}
