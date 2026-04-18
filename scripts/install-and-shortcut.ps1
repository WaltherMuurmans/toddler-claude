# Install Toddler Claude silently (currentUser NSIS) and put a shortcut on the Desktop.
# Usage: pwsh -NoProfile -ExecutionPolicy Bypass -File install-and-shortcut.ps1 -Installer "path\to\Toddler Claude_0.1.0_x64-setup.exe"

param(
    [Parameter(Mandatory=$true)]
    [string]$Installer
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $Installer)) {
    throw "Installer not found: $Installer"
}

Write-Host "Installing silently..." -ForegroundColor Cyan
$p = Start-Process -FilePath $Installer -ArgumentList "/S" -Wait -PassThru
if ($p.ExitCode -ne 0) {
    throw "Installer exited with code $($p.ExitCode)"
}
Write-Host "Install complete."

# Find installed exe (currentUser NSIS puts it in %LOCALAPPDATA%\Programs\<productName>\)
$candidates = @(
    "$env:LOCALAPPDATA\Programs\Toddler Claude\toddler-claude.exe",
    "$env:LOCALAPPDATA\Programs\Toddler Claude\Toddler Claude.exe",
    "$env:LOCALAPPDATA\Toddler Claude\toddler-claude.exe"
)
$exe = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $exe) {
    Write-Warning "Could not locate installed exe in default locations; searching..."
    $exe = Get-ChildItem -Path "$env:LOCALAPPDATA\Programs","$env:ProgramFiles" -Recurse -Filter "toddler-claude.exe" -ErrorAction SilentlyContinue | Select-Object -First 1 | ForEach-Object FullName
}
if (-not $exe) {
    throw "Could not find installed toddler-claude.exe"
}
Write-Host "Found app: $exe"

$desktop = [Environment]::GetFolderPath("Desktop")
$shortcut = Join-Path $desktop "Toddler Claude.lnk"

$wsh = New-Object -ComObject WScript.Shell
$lnk = $wsh.CreateShortcut($shortcut)
$lnk.TargetPath = $exe
$lnk.WorkingDirectory = Split-Path $exe
$lnk.IconLocation = $exe
$lnk.Description = "Toddler-proof remote Claude coding sessions"
$lnk.Save()

Write-Host "Desktop shortcut created: $shortcut" -ForegroundColor Green
