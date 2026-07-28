<#
.SYNOPSIS
    br (beads_rust) installer for Windows PowerShell

.DESCRIPTION
    Installs the br CLI tool on Windows. Downloads the release binary from GitHub,
    extracts it. Mirrors the functionality of install.sh for Unix.

.PARAMETER Version
    Specific version to install (default: latest), e.g. v0.2.15

.PARAMETER InstallDir
    Installation directory (default: $env:LOCALAPPDATA\Programs\br)

.PARAMETER System
    Install to a system-wide location (requires admin)

.PARAMETER EasyMode
    Auto-update PATH in user environment

.PARAMETER SkipSkills
    Don't install any Claude Code / Codex skills

.PARAMETER WithMigrationSkill
    Install the bd-to-br-migration skill (opt-in)

.PARAMETER Verify
    Run self-test after install

.PARAMETER Quiet
    Suppress non-error output

.PARAMETER Uninstall
    Remove br and clean up

.PARAMETER Help
    Show this help

.EXAMPLE
    # One-liner install
    irm "https://raw.githubusercontent.com/quangdang46/beads_rust/main/install.ps1" | iex

    # Install with options
    irm .../install.ps1 | iex -Args "--easy-mode --verify"

    # Install specific version
    irm .../install.ps1 | iex -Args "--version v0.2.15"

    # Uninstall
    irm .../install.ps1 | iex -Args "--uninstall"
#>

param(
    [string]$Version = "",
    [string]$InstallDir = "",
    [switch]$System = $false,
    [switch]$EasyMode = $false,
    [switch]$SkipSkills = $false,
    [switch]$WithMigrationSkill = $false,
    [switch]$Verify = $false,
    [switch]$Quiet = $false,
    [switch]$Uninstall = $false,
    [switch]$Help = $false
)

# ============================================================================
# Configuration
# ============================================================================
$Script:BinaryName = "br.exe"
$Script:Owner = "quangdang46"
$Script:Repo = "beads_rust"
$Script:MaxRetries = 3
$Script:DownloadTimeout = 120
$Script:InstallerVersion = "1.0.0"
$Script:TempDir = ""
$Script:LockDir = ""

# Default install directory
if (-not $InstallDir) {
    if ($System) {
        $InstallDir = "$env:ProgramFiles\br"
    } else {
        $InstallDir = "$env:LOCALAPPDATA\Programs\br"
    }
}

# ============================================================================
# Help
# ============================================================================
function Show-Help {
    Write-Host @"

br installer v$($Script:InstallerVersion) - Install beads_rust (br) CLI tool

USAGE:
  irm .../install.ps1 | iex
  irm .../install.ps1 | iex -Args "--easy-mode"

OPTIONS:
  --version vX.Y.Z       Install specific version (default: latest)
  --install-dir DIR      Install to directory (default: ~\AppData\Local\Programs\br)
  --system               Install to Program Files (requires admin)
  --easy-mode            Auto-update PATH in user environment
  --skip-skills          Don't install any Claude Code / Codex skills
  --with-migration-skill Install the bd-to-br-migration skill (opt-in)
  --verify               Run self-test after install
  --quiet                Suppress non-error output
  --uninstall            Remove br and clean up
  --help                 Show this help

EXAMPLES:
  # Default install
  irm https://raw.githubusercontent.com/quangdang46/beads_rust/main/install.ps1 | iex

  # With PATH update and verify
  irm .../install.ps1 | iex -Args "--easy-mode --verify"

  # Uninstall
  irm .../install.ps1 | iex -Args "--uninstall"

PLATFORMS:
  - Windows x86_64

"@
    exit 0
}

if ($Help) { Show-Help }

# ============================================================================
# Logging functions
# ============================================================================
function Write-Info {
    if (-not $Quiet) { Write-Host "→ $($args[0])" -ForegroundColor Cyan }
}
function Write-Success {
    if (-not $Quiet) { Write-Host "✓ $($args[0])" -ForegroundColor Green }
}
function Write-Warn {
    Write-Host "WARN: $($args[0])" -ForegroundColor Yellow
}
function Write-Error {
    Write-Host "ERROR: $($args[0])" -ForegroundColor Red
    exit 1
}
function Write-Step {
    if (-not $Quiet) { Write-Host "  $($args[0])" -ForegroundColor DarkGray }
}

# ============================================================================
# Locking
# ============================================================================
function Acquire-Lock {
    $Script:LockDir = Join-Path $env:TEMP "br-install.lock.d"
    try {
        New-Item -Path $Script:LockDir -ItemType Directory -ErrorAction Stop | Out-Null
    } catch {
        # Check if lock is stale (older than 5 minutes)
        if (Test-Path $Script:LockDir) {
            $lockAge = (Get-Date) - (Get-Item $Script:LockDir).CreationTime
            if ($lockAge.TotalMinutes -gt 5) {
                Remove-Lock
                New-Item -Path $Script:LockDir -ItemType Directory -ErrorAction Stop | Out-Null
            } else {
                Write-Error "Another installation is running. If stuck, remove $($Script:LockDir) manually."
            }
        } else {
            Write-Error "Another installation is running. If stuck, remove $($Script:LockDir) manually."
        }
    }
}

function Remove-Lock {
    if ($Script:LockDir -and (Test-Path $Script:LockDir)) {
        Remove-Item -Path $Script:LockDir -Force -Recurse -ErrorAction SilentlyContinue | Out-Null
    }
}

# ============================================================================
# Cleanup
# ============================================================================
function Cleanup {
    if ($Script:TempDir -and (Test-Path $Script:TempDir)) {
        Remove-Item -Path $Script:TempDir -Force -Recurse -ErrorAction SilentlyContinue | Out-Null
    }
    Remove-Lock
}

# ============================================================================
# Uninstall
# ============================================================================
function Do-Uninstall {
    Write-Info "Uninstalling br..."

    $binPath = Join-Path $InstallDir $Script:BinaryName
    if (Test-Path $binPath) {
        Remove-Item -Path $binPath -Force -ErrorAction SilentlyContinue
        Write-Success "Removed $binPath"
    } else {
        Write-Warn "Binary not found at $binPath"
    }

    # Remove install directory if empty
    if (Test-Path $InstallDir) {
        $remaining = Get-ChildItem $InstallDir -Recurse -ErrorAction SilentlyContinue
        if (-not $remaining) {
            Remove-Item -Path $InstallDir -Force -ErrorAction SilentlyContinue
        }
    }

    # Remove PATH entries
    $currentPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($currentPath -like "*$InstallDir*") {
        $newPath = ($currentPath -split ';' | Where-Object { $_ -ne $InstallDir }) -join ';'
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        Write-Info "Cleaned PATH (user)"
    }

    Write-Success "br uninstalled successfully"
    exit 0
}

# ============================================================================
# Platform detection
# ============================================================================
function Get-Platform {
    if ($IsLinux) { $os = "linux" }
    elseif ($IsMacOS) { $os = "macos" }
    else { $os = "windows" }

    $envArch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    if ($envArch -eq [System.Runtime.InteropServices.Architecture]::Arm64) { $arch = "arm64" }
    elseif ($envArch -eq [System.Runtime.InteropServices.Architecture]::X64) { $arch = "x64" }
    else { $arch = "x64" }

    return "${os}-${arch}"
}

# ============================================================================
# Version resolution
# ============================================================================
function Resolve-Version {
    if ($Script:Version) { return }

    Write-Info "Resolving latest version..."
    $apiUrl = "https://api.github.com/repos/$($Script:Owner)/$($Script:Repo)/releases/latest"
    $tag = ""

    for ($attempt = 0; $attempt -lt $Script:MaxRetries; $attempt++) {
        try {
            $response = Invoke-RestMethod -Uri $apiUrl -Headers @{
                "Accept" = "application/vnd.github.v3+json"
            } -TimeoutSec 30 -ErrorAction Stop
            $tag = $response.tag_name
            if ($tag -match '^v[0-9]') { break }
        } catch {
            Write-Step "Attempt $($attempt+1) failed"
        }
        if (-not $tag -and $attempt -lt ($Script:MaxRetries-1)) { Start-Sleep -Seconds 2 }
    }

    if ($tag -match '^v[0-9]') {
        $Script:Version = $tag
        Write-Success "Latest version: $Script:Version"
        return
    }

    # Fallback: redirect-based resolution
    try {
        $redirectUrl = "https://github.com/$($Script:Owner)/$($Script:Repo)/releases/latest"
        $request = [System.Net.WebRequest]::Create($redirectUrl)
        $request.AllowAutoRedirect = $false
        $response = $request.GetResponse()
        $tag = [regex]::match($response.Headers['Location'], '/tag/(v[0-9][^/]*)').Groups[1].Value
        $response.Close()
    } catch { }

    if ($tag -match '^v[0-9]') {
        $Script:Version = $tag
        Write-Success "Latest version (via redirect): $Script:Version"
        return
    }

    Write-Error "Could not resolve latest version. Check internet or specify --version."
}

# ============================================================================
# Download file with retry
# ============================================================================
function Download-File {
    param([string]$Url, [string]$DestPath)

    for ($attempt = 0; $attempt -lt $Script:MaxRetries; $attempt++) {
        try {
            Write-Step "Downloading: $Url"
            $webClient = New-Object System.Net.WebClient
            $webClient.DownloadFile($Url, $DestPath)
            return $true
        } catch {
            Write-Step "Attempt $($attempt+1) failed: $_"
            if ($attempt -lt ($Script:MaxRetries-1)) { Start-Sleep -Seconds 3 }
        }
    }
    return $false
}

# ============================================================================
# Checksum verification
# ============================================================================
function Get-SHA256 {
    param([string]$FilePath)
    $hash = Get-FileHash -Path $FilePath -Algorithm SHA256
    return $hash.Hash.ToLower()
}

function Verify-Checksum {
    param([string]$ArchivePath, [string]$ArchiveName, [string]$Expected)

    if (-not $Expected) {
        Write-Success "No checksum to verify (skipping)"
        return $true
    }

    Write-Info "Verifying checksum..."
    $actual = Get-SHA256 -FilePath $ArchivePath
    if ($expected -ne $actual) {
        Write-Error "Checksum mismatch for $ArchiveName`n  Expected: $expected`n  Got:      $actual"
        return $false
    }
    Write-Success "Checksum verified"
    return $true
}

# ============================================================================
# Install binary atmoic
# ============================================================================
function Install-BinaryAtomic {
    param([string]$SourcePath, [string]$DestPath)

    $destDir = Split-Path $DestPath -Parent
    if (-not (Test-Path $destDir)) {
        New-Item -Path $destDir -ItemType Directory -Force | Out-Null
    }

    # Copy to temp then rename for atomicity
    $tmpPath = "$DestPath.tmp"
    Copy-Item -Path $SourcePath -Destination $tmpPath -Force
    Move-Item -Path $tmpPath -Destination $DestPath -Force
    Write-Success "Installed to $DestPath"
}

# ============================================================================
# Download release
# ============================================================================
function Download-Release {
    param([string]$Platform)

    $archiveName = "br-${Platform}.zip"
    $url = "https://github.com/$($Script:Owner)/$($Script:Repo)/releases/download/$($Script:Version)/${archiveName}"
    $archivePath = Join-Path $Script:TempDir $archiveName

    Write-Info "Downloading $archiveName..."
    if (-not (Download-File -Url $url -DestPath $archivePath)) {
        return $null
    }

    # Download checksum
    $checksumUrl = "${url}.sha256"
    $checksumPath = Join-Path $Script:TempDir "checksum.sha256"
    $expected = ""
    if (Download-File -Url $checksumUrl -DestPath $checksumPath) {
        $content = Get-Content $checksumPath
        if ($content[0] -and $content[0] -match '^[0-9a-f]{64}') {
            $expected = $content[0].Split(' ')[0]
        } else {
            foreach ($line in $content) {
                if ($line -and $line -match '^[0-9a-f]{64}') {
                    $expected = $line.Split(' ')[0]
                    break
                }
            }
        }
    }

    if (-not (Verify-Checksum -ArchivePath $archivePath -ArchiveName $archiveName -Expected $expected)) {
        return $null
    }

    # Extract
    Write-Info "Extracting..."
    $extractDir = Join-Path $Script:TempDir "extract"
    try {
        Expand-Archive -Path $archivePath -DestinationPath $extractDir -Force
    } catch {
        Write-Error "Failed to extract archive: $_"
        return $null
    }

    # Find binary
    $binPath = Get-ChildItem -Path $extractDir -Recurse -Filter $Script:BinaryName | Select-Object -First 1 -ExpandProperty FullName
    if (-not $binPath) {
        # Try without .exe
        $binPath = Get-ChildItem -Path $extractDir -Recurse -Filter "br" | Select-Object -First 1 -ExpandProperty FullName
    }
    if (-not $binPath -or -not (Test-Path $binPath)) {
        Write-Error "Binary not found after extraction"
        return $null
    }

    return $binPath
}

# ============================================================================
# PATH update
# ============================================================================
function Update-Path {
    $currentPath = [Environment]::GetEnvironmentVariable("Path", "User")

    if ($currentPath -like "*$InstallDir*") {
        Write-Step "Already in PATH"
        return
    }

    if ($EasyMode) {
        $newPath = "$InstallDir;$currentPath"
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        Write-Warn "PATH updated (user). Restart terminal or reload profile."
        # Also update current session
        $env:Path = "$InstallDir;$env:Path"
    } else {
        Write-Warn "Add to PATH manually: $InstallDir"
        Write-Step "Or re-run with --easy-mode"
    }
}

# ============================================================================
# Install Claude Code / Codex skills
# ============================================================================
function Install-Skills {
    if ($SkipSkills) {
        Write-Info "Skipping skills installation (--skip-skills)"
        return
    }

    Write-Info "Installing Claude Code / Codex skills..."

    $skillsBaseUrl = "https://raw.githubusercontent.com/$($Script:Owner)/$($Script:Repo)/main/skills"
    $claudeSkillsDir = "$env:USERPROFILE\.claude\skills"
    $codexSkillsDir = "$env:USERPROFILE\.codex\skills"

    $skillName = "bd-to-br-migration"
    $files = @(
        "SKILL.md",
        "SELF-TEST.md",
        "references/TRANSFORMS.md",
        "references/BULK.md",
        "references/PITFALLS.md",
        "scripts/find-bd-refs.sh",
        "scripts/verify-migration.sh",
        "subagents/batch-migrator.md"
    )

    if (-not $WithMigrationSkill) {
        Write-Info "Skipping skill: $skillName (opt-in via --with-migration-skill)"
        return
    }

    $filesInstalled = 0
    foreach ($file in $files) {
        $url = "$skillsBaseUrl/$skillName/$file"
        $claudeDest = "$claudeSkillsDir\$skillName\$file"

        # Ensure directory exists
        $destDir = Split-Path $claudeDest -Parent
        if (-not (Test-Path $destDir)) {
            New-Item -Path $destDir -ItemType Directory -Force | Out-Null
        }

        if (Download-File -Url $url -DestPath $claudeDest) {
            $filesInstalled++
            Write-Step "Downloaded $file"

            # Copy to Codex skills
            $codexDest = "$codexSkillsDir\$skillName\$file"
            $codexDestDir = Split-Path $codexDest -Parent
            if (-not (Test-Path $codexDestDir)) {
                New-Item -Path $codexDestDir -ItemType Directory -Force | Out-Null
            }
            Copy-Item -Path $claudeDest -Destination $codexDest -Force -ErrorAction SilentlyContinue
        }
    }

    if ($filesInstalled -gt 0) {
        Write-Success "Installed skill: $skillName ($filesInstalled files)"
    } else {
        Write-Warn "Skill ${skillName}: no files could be downloaded"
    }
}

# ============================================================================
# Print summary
# ============================================================================
function Print-Summary {
    $binPath = Join-Path $InstallDir $Script:BinaryName
    $version = "unknown"
    try {
        $version = & $binPath --version
    } catch { }

    Write-Host @"

✓ br installed successfully!

  Version:  $version
  Location: $binPath

  Quick Start:
    br init            Initialize a workspace
    br create          Create an issue
    br list            List issues
    br ready           Show ready work
    br --help          Full help

"@
}

# ============================================================================
# Main
# ============================================================================
function Main {
    Acquire-Lock
    $Script:TempDir = Join-Path $env:TEMP "br-install-$([System.Guid]::NewGuid().ToString().Substring(0,8))"
    New-Item -Path $Script:TempDir -ItemType Directory -Force | Out-Null

    try {
        $platform = Get-Platform
        Write-Info "Platform: $platform"
        Write-Info "Install directory: $InstallDir"

        Resolve-Version

        # Download and install
        $binPath = Download-Release -Platform $platform
        if (-not $binPath) {
            Write-Warn "No pre-built binary available for ${platform} (Windows builds are paused)."
            Write-Warn "Install WSL and use: curl -fsSL https://raw.githubusercontent.com/quangdang46/beads_rust/main/install.sh | bash"
        }

        $destPath = Join-Path $InstallDir $Script:BinaryName
        Install-BinaryAtomic -SourcePath $binPath -DestPath $destPath

        # Post-install (auto-add to PATH)
        $EasyMode = $true
        Update-Path

        Install-Skills

        # Verify
        if ($Verify) {
            Write-Info "Running self-test..."
            try { & $destPath --version } catch { }
            Write-Success "Self-test complete"
        }

        Print-Summary

    } finally {
        Cleanup
    }
}

# Run main
if ($Uninstall) { Do-Uninstall }
Main
