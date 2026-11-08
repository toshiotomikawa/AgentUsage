# Builds AgentUsage in release mode, installs it to %LOCALAPPDATA%\Programs\AgentUsage and creates
# "AgentUsage" shortcuts on the Desktop and in the Start menu. Safe to run again after updates.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try { cargo build --release } finally { Pop-Location }
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

$dest = Join-Path $env:LOCALAPPDATA 'Programs\AgentUsage'
New-Item -ItemType Directory -Force $dest | Out-Null
# Close a running copy and wait until Windows releases the file.
$running = Get-Process agent-usage -ErrorAction SilentlyContinue
if ($running) {
    $running | Stop-Process -Force
    $running | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue
}
$exe = Join-Path $dest 'agent-usage.exe'
for ($i = 0; ; $i++) {
    try {
        Copy-Item (Join-Path $root 'target\release\agent-usage.exe') $exe -Force -ErrorAction Stop
        break
    } catch {
        if ($i -ge 20) { throw }
        Start-Sleep -Milliseconds 250
    }
}
$ico = Get-ChildItem (Join-Path $root 'target\release\build') -Recurse -Filter icon.ico |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if ($ico) { Copy-Item $ico.FullName (Join-Path $dest 'agent-usage.ico') -Force }

$shell = New-Object -ComObject WScript.Shell
$places = @(
    [Environment]::GetFolderPath('Desktop'),
    (Join-Path ([Environment]::GetFolderPath('Programs')) '')
)
foreach ($dir in $places) {
    $lnk = $shell.CreateShortcut((Join-Path $dir 'AgentUsage.lnk'))
    $lnk.TargetPath = $exe
    $lnk.WorkingDirectory = $dest
    $lnk.IconLocation = if ($ico) { "$(Join-Path $dest 'agent-usage.ico'),0" } else { "$exe,0" }
    $lnk.Description = 'Cotas dos agentes de IA'
    $lnk.Save()
}
# Ask Explorer to drop cached icons so an updated icon shows at once.
Start-Process ie4uinit.exe -ArgumentList '-show' -NoNewWindow -Wait -ErrorAction SilentlyContinue
Write-Host "Instalado em $dest"
