<#
.SYNOPSIS
  Installs (or repairs) a Kernel node on this Windows PC. Made for Pluto.

.DESCRIPTION
  - Downloads the newest node build from GitHub Releases (or uses -Zip), checks its SHA-256.
  - Puts it in <Root>\app, with a Python environment for modules in <Root>\python (via uv).
  - Writes <Root>\node.toml with a new node token (kept if it already exists).
  - Registers the "Kernel node" scheduled task: starts at logon, and every 5 minutes starts
    kerneld again if it isn't running (a watchdog). Self-updates restart through it too.
  - Opens the port in Windows Firewall for Tailscale only (needs admin). Not your home network.

  After this, updates come from the app: Node > Install update. You don't need this script again.

.EXAMPLE
  # From your PC, in the repo folder:
  scp scripts\install-node.ps1 pluto:
  ssh -t pluto powershell -ExecutionPolicy Bypass -File install-node.ps1
#>
[CmdletBinding()]
param(
  # Use a downloaded bundle (kernel-node-windows-x64.zip) instead of fetching the newest release.
  [string]$Zip,
  [string]$Repo = 'B0RE16/laughing-waffle',
  [string]$Root = (Join-Path $env:LOCALAPPDATA 'Kernel\node'),
  [int]$Port = 47800,
  # Modules to run. hello is only for tests.
  [string[]]$Modules = @('minecraft', 'pc-monitor', 'roblox', 'comfyui', 'vram', 'laya', 'flowrace', 'openfork'),
  [switch]$NoFirewall
)

$ErrorActionPreference = 'Stop'
# Windows PowerShell 5.1 may still default to old TLS versions, which GitHub refuses.
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
$ProgressPreference = 'SilentlyContinue'  # Invoke-WebRequest is very slow with the progress bar
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$Asset = 'kernel-node-windows-x64.zip'
$TaskName = 'Kernel node'

function Step($text) { Write-Host "`n== $text" -ForegroundColor Cyan }
function Note($text) { Write-Host "   $text" }

function Read-Secret($prompt) {
  $secure = Read-Host -AsSecureString $prompt
  $ptr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
  try { [Runtime.InteropServices.Marshal]::PtrToStringBSTR($ptr) }
  finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr) }
}

function New-Token {
  $bytes = New-Object byte[] 32
  [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
  [Convert]::ToBase64String($bytes).TrimEnd('=').Replace('+', '-').Replace('/', '_')
}

# A TOML literal string: no escaping needed for Windows paths. Single quotes aren't allowed in it.
function TomlPath($path) {
  if ($path -match "'") { throw "Paths with a ' in them aren't supported: $path" }
  "'$path'"
}

New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Config = Join-Path $Root 'node.toml'
$GitHubToken = ''
if (Test-Path $Config) {
  $m = Select-String -Path $Config -Pattern '^\s*token\s*=\s*"(github_[^"]*|ghp_[^"]*)"' | Select-Object -First 1
  if ($m) { $GitHubToken = $m.Matches[0].Groups[1].Value }
}

# ---------------------------------------------------------------- 1. the bundle
Step 'Getting the node build'
$download = Join-Path $Root 'download'
New-Item -ItemType Directory -Force -Path $download | Out-Null
if ($Zip) {
  $bundle = (Resolve-Path $Zip).Path
  Note "Using $bundle"
} else {
  if (-not $GitHubToken) {
    Note "If the repo is private, the node needs a read-only GitHub token to download builds."
    Note "Create one at https://github.com/settings/personal-access-tokens/new :"
    Note "  Repository access: only $Repo.  Permissions: Contents = Read-only.  Nothing else."
    Note "If the repo is public, just press Enter."
    $GitHubToken = Read-Secret 'Paste the token (or Enter for none)'
  }
  $headers = @{
    'X-GitHub-Api-Version' = '2022-11-28'
    'User-Agent'           = 'kernel-installer'
  }
  if ($GitHubToken) { $headers.Authorization = "Bearer $GitHubToken" }
  $bundle = Join-Path $download $Asset
  $releases = $null
  try {
    $releases = Invoke-RestMethod -Headers ($headers + @{ Accept = 'application/vnd.github+json' }) `
      -Uri "https://api.github.com/repos/$Repo/releases?per_page=30"
  } catch {
    Note "Couldn't reach GitHub's API ($($_.Exception.Message)); downloading the latest release directly."
  }
  if ($null -ne $releases) {
    $best = $null; $bestBuild = -1
    foreach ($r in $releases) {
      if ($r.draft -or $r.prerelease -or $r.tag_name -notmatch '^node-build-(\d+)$') { continue }
      $n = [int]$Matches[1]
      $hasZip = $r.assets | Where-Object { $_.name -eq $Asset }
      $hasSha = $r.assets | Where-Object { $_.name -eq "$Asset.sha256" }
      if ($hasZip -and $hasSha -and $n -gt $bestBuild) { $best = $r; $bestBuild = $n }
    }
    if (-not $best) { throw "No node builds found in $Repo. Merge to main once so CI publishes one." }
    Note "Newest build: $($best.tag_name)"
    foreach ($name in @($Asset, "$Asset.sha256")) {
      $a = $best.assets | Where-Object { $_.name -eq $name }
      Invoke-WebRequest -Headers ($headers + @{ Accept = 'application/octet-stream' }) `
        -Uri $a.url -OutFile (Join-Path $download $name) -UseBasicParsing
    }
  } else {
    # Public repos only: the release GitHub marks as latest, without the API.
    foreach ($name in @($Asset, "$Asset.sha256")) {
      Invoke-WebRequest -Uri "https://github.com/$Repo/releases/latest/download/$name" `
        -OutFile (Join-Path $download $name) -UseBasicParsing
    }
  }
  $expected = ((Get-Content (Join-Path $download "$Asset.sha256") -Raw).Trim() -split '\s+')[0].ToLower()
  $actual = (Get-FileHash -Algorithm SHA256 $bundle).Hash.ToLower()
  if ($expected -ne $actual) { throw "Checksum mismatch: expected $expected, got $actual" }
  Note 'Checksum OK'
}

# ---------------------------------------------------------------- 2. stop a running node
Step 'Stopping a running node, if any'
if (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue) {
  Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
}
$pidFile = Join-Path $Root 'data\kerneld.pid'
if (Test-Path $pidFile) {
  $old = Get-Process -Id ([int](Get-Content $pidFile -Raw).Trim()) -ErrorAction SilentlyContinue
  if ($old -and $old.ProcessName -eq 'kerneld') { $old | Stop-Process -Force; Start-Sleep -Seconds 2 }
}

# ---------------------------------------------------------------- 3. unpack
Step "Installing to $Root\app"
$staging = Join-Path $Root 'staging\install'
if (Test-Path $staging) { Remove-Item -Recurse -Force $staging }
Expand-Archive -Path $bundle -DestinationPath $staging
if (-not (Test-Path (Join-Path $staging 'kerneld.exe'))) { throw 'The bundle has no kerneld.exe' }
$app = Join-Path $Root 'app'
$previous = Join-Path $Root 'app.previous'
if (Test-Path $app) {
  if (Test-Path $previous) { Remove-Item -Recurse -Force $previous }
  Move-Item $app $previous
}
Move-Item $staging $app
Note "Build $((Get-Content (Join-Path $app 'BUILD') -ErrorAction SilentlyContinue))"

# ---------------------------------------------------------------- 4. Python for modules
Step 'Python environment for modules'
$uv = (Get-Command uv -ErrorAction SilentlyContinue).Source
if (-not $uv) {
  Note 'Installing uv (https://docs.astral.sh/uv/)'
  Invoke-RestMethod https://astral.sh/uv/install.ps1 | Invoke-Expression
  $uv = Join-Path $env:USERPROFILE '.local\bin\uv.exe'
}
$venv = Join-Path $Root 'python'
$python = Join-Path $venv 'Scripts\python.exe'
if (-not (Test-Path $python)) { & $uv venv $venv --python 3.12; if ($LASTEXITCODE) { throw 'uv venv failed' } }
# The SDK, plus each module's own packages (the same set the self-updater installs).
$wheel = Get-ChildItem (Join-Path $app 'sdk') -Filter *.whl | Select-Object -First 1
$pipArgs = @('pip', 'install', '--python', $python, '--reinstall-package', 'kernel-sdk', $wheel.FullName)
Get-ChildItem (Join-Path $app 'modules') -Directory | Sort-Object Name | ForEach-Object {
  $req = Join-Path $_.FullName 'requirements.txt'
  if (Test-Path $req) { $pipArgs += @('-r', $req) }
}
& $uv @pipArgs
if ($LASTEXITCODE) { throw 'Installing the Python packages failed' }

# ---------------------------------------------------------------- 4b. Node.js (Flow Race)
if ($Modules -contains 'flowrace' -or $Modules -contains 'openfork') {
  Step 'Node.js for the game modules'
  $nodeExe = (Get-Command node -ErrorAction SilentlyContinue).Source
  if (-not $nodeExe -and (Test-Path "$env:ProgramFiles\nodejs\node.exe")) { $nodeExe = "$env:ProgramFiles\nodejs\node.exe" }
  $nodeOk = $false
  if ($nodeExe) {
    $v = (& $nodeExe --version) -replace '^v', ''
    $nodeOk = [version]$v -ge [version]'22.18'
    Note "Found Node $v"
  }
  if (-not $nodeOk) {
    if (Get-Command winget -ErrorAction SilentlyContinue) {
      Note 'Installing Node.js LTS with winget'
      winget install --id OpenJS.NodeJS.LTS -e --silent --accept-package-agreements --accept-source-agreements
      if ($LASTEXITCODE) { Note 'winget failed; install Node 22.18+ from https://nodejs.org, then restart Kernel' }
    } else {
      Note 'Install Node 22.18+ from https://nodejs.org, then restart Kernel (Flow Race waits for it)'
    }
  }
}

# ---------------------------------------------------------------- 5. config
Step 'Node config'
$nodeToken = $null
if (Test-Path $Config) {
  Note "Keeping $Config"
} else {
  $nodeToken = New-Token
  $id = ($env:COMPUTERNAME.ToLower() -replace '[^a-z0-9-]', '-')
  $moduleList = ($Modules | ForEach-Object { "`"$_`"" }) -join ', '
  @"
# Kernel node. Written by install-node.ps1; edit freely.
node_id = "$id"
node_name = "$env:COMPUTERNAME"
listen = "0.0.0.0:$Port"          # kerneld only accepts Tailscale and this PC
allow_lan = false                 # true would also allow your home network
token = "$nodeToken"
modules_dir = 'app\modules'
data_dir = 'data'
python = $(TomlPath $python)
enabled_modules = [$moduleList]

[update]
repo = "$Repo"
token = "$GitHubToken"
uv = $(TomlPath $uv)
scheduled_task = "$TaskName"
check_interval_h = 6
auto_install = false
"@ | Set-Content -Path $Config -Encoding ascii
  Note "Wrote $Config"
}

# ---------------------------------------------------------------- 6. autostart + watchdog
Step "Scheduled task '$TaskName'"
$exe = Join-Path $app 'kerneld.exe'
$action = New-ScheduledTaskAction -Execute $exe -Argument "--config `"$Config`"" -WorkingDirectory $Root
$logon = New-ScheduledTaskTrigger -AtLogOn -User "$env:USERDOMAIN\$env:USERNAME"
# Watchdog: every 5 minutes, start kerneld if it isn't running (IgnoreNew skips it if it is).
$watch = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(1) -RepetitionInterval (New-TimeSpan -Minutes 5)
$settings = New-ScheduledTaskSettingsSet -MultipleInstances IgnoreNew -ExecutionTimeLimit (New-TimeSpan -Seconds 0) `
  -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable
Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger @($logon, $watch) -Settings $settings `
  -Description 'Kernel node (kerneld). Starts at logon; the 5-minute trigger restarts it if it stopped.' -Force | Out-Null
Note 'Starts at logon, restarts within 5 minutes if it ever stops'

# ---------------------------------------------------------------- 7. firewall
if (-not $NoFirewall) {
  Step 'Windows Firewall'
  $admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
  if ($admin) {
    Get-NetFirewallRule -DisplayName 'Kernel node' -ErrorAction SilentlyContinue | Remove-NetFirewallRule
    New-NetFirewallRule -DisplayName 'Kernel node' -Direction Inbound -Action Allow -Protocol TCP `
      -LocalPort $Port -Program $exe -RemoteAddress '100.64.0.0/10' -Profile Any | Out-Null
    Note "Port $Port open to Tailscale (100.64.0.0/10) only"
  } else {
    Note 'Not running as admin, so the firewall rule was skipped. As admin, run:'
    Note "  New-NetFirewallRule -DisplayName 'Kernel node' -Direction Inbound -Action Allow -Protocol TCP -LocalPort $Port -Program '$exe' -RemoteAddress 100.64.0.0/10 -Profile Any"
  }
}

# ---------------------------------------------------------------- 8. start
Step 'Starting the node'
Start-ScheduledTask -TaskName $TaskName
$up = $false
for ($i = 0; $i -lt 30 -and -not $up; $i++) {
  Start-Sleep -Seconds 1
  try { $c = New-Object Net.Sockets.TcpClient('127.0.0.1', $Port); $c.Close(); $up = $true } catch { }
}
if (-not $up) {
  Write-Warning "kerneld didn't start listening. Logs: $Root\data\logs"
  exit 1
}
Note "Running (logs in $Root\data\logs)"

$ts = $null
if (Get-Command tailscale -ErrorAction SilentlyContinue) { $ts = (& tailscale ip -4 2>$null | Select-Object -First 1) }
Write-Host ''
Write-Host 'Done. In the Kernel app, open Settings and enter:' -ForegroundColor Green
if ($ts) {
  Write-Host "   Address  ws://${ts}:$Port/ws"
} else {
  Write-Warning 'Tailscale not found on this PC. The node only accepts Tailscale connections, so install and sign in to Tailscale.'
}
if ($nodeToken) {
  Write-Host "   Token    $nodeToken"
} else {
  Write-Host "   Token    (unchanged, it's the token = line in $Config)"
}
