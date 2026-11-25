<#
.SYNOPSIS
    One-click legal setup, runtime pack builder/preflight, and launcher for ORPHEUS (1.74.0.0.104319).
.DESCRIPTION
    Unifies all required runtime checks and server launch into one script without splitting into
    optional packs or embedding any copyrighted Blizzard assets.
    - If -SourceDir is provided (and -RuntimeDir is missing or -Repack is set), packs a single
      complete runtime directory via `orpheus-runtime`'s `prepare_server_runtime`.
    - Builds `orpheus_server.exe` in release mode with `--features inspector,lua-mods` if needed.
    - Runs `--check-runtime` preflight and reports enriched retail data availability.
    - Uses a persistent local save directory (`%LOCALAPPDATA%\ORPHEUS\state` by default) so skins,
      loot boxes, credits, and settings persist across sessions.
#>
[CmdletBinding()]
param(
    [string]$RuntimeDir = "",
    [string]$SourceDir = "",
    [string]$DataDir = "",
    [string]$Account = "",
    [int]$DebugPort = 3726,
    [string]$ModScript = "",
    [string]$AiScript = "",
    [string[]]$Maps = @("0688", "06C9"),
    [switch]$Repack,
    [switch]$NoBuild,
    [switch]$PreflightOnly
)

$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if ([string]::IsNullOrWhiteSpace($RuntimeDir)) {
    if (-not [string]::IsNullOrWhiteSpace($env:ORPHEUS_RUNTIME_DIR)) {
        $RuntimeDir = $env:ORPHEUS_RUNTIME_DIR
    } else {
        $RuntimeDir = Join-Path (Split-Path $RepoRoot -Parent) "orpheus-runtime-104319"
    }
}

if ([string]::IsNullOrWhiteSpace($DataDir)) {
    $LocalAppData = if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        Join-Path $env:USERPROFILE "AppData\Local"
    } else {
        $env:LOCALAPPDATA
    }
    $DataDir = Join-Path $LocalAppData "ORPHEUS\state"
}

Write-Host "=== ORPHEUS 1.74 Unified Launcher ===" -ForegroundColor Cyan
Write-Host "Repo Root   : $RepoRoot"
Write-Host "Runtime Dir : $RuntimeDir"
Write-Host "Save Dir    : $DataDir"

Push-Location $RepoRoot
try {
    $NeedsPack = $Repack -or (-not (Test-Path (Join-Path $RuntimeDir "data\schemas_174.json")))
    if ($NeedsPack -and (-not [string]::IsNullOrWhiteSpace($SourceDir))) {
        Write-Host "[1/4] Packing unified 1.74 runtime from local source: $SourceDir" -ForegroundColor Yellow
        $PackArgs = @(
            "run", "--locked", "-p", "orpheus-runtime", "--bin", "prepare_server_runtime", "--",
            "--source", $SourceDir,
            "--destination", $RuntimeDir
        )
        foreach ($MapId in $Maps) {
            if (-not [string]::IsNullOrWhiteSpace($MapId)) {
                $PackArgs += @("--map", $MapId)
            }
        }
        & cargo @PackArgs
        if ($LASTEXITCODE -ne 0) {
            throw "prepare_server_runtime failed with exit code $LASTEXITCODE"
        }
    } elseif (-not (Test-Path $RuntimeDir)) {
        throw "Runtime directory not found: $RuntimeDir`nProvide -RuntimeDir <prepared-pack> or -SourceDir <local-extracted-1.74-source>."
    }

    $ServerExe = Join-Path $RepoRoot "target\release\orpheus_server.exe"
    if ((-not $NoBuild) -and (-not (Test-Path $ServerExe))) {
        Write-Host "[2/4] Building release orpheus_server (inspector + lua-mods)..." -ForegroundColor Yellow
        & cargo build --locked --release --features "inspector,lua-mods" --bin orpheus_server
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed with exit code $LASTEXITCODE"
        }
    } elseif (-not (Test-Path $ServerExe)) {
        throw "Release binary not found at $ServerExe (re-run without -NoBuild to compile)."
    } else {
        Write-Host "[2/4] Using release binary: $ServerExe" -ForegroundColor Green
    }

    Write-Host "[3/4] Running unified runtime preflight (--check-runtime)..." -ForegroundColor Yellow
    & $ServerExe --check-runtime --runtime $RuntimeDir
    if ($LASTEXITCODE -ne 0) {
        throw "Runtime preflight failed with exit code $LASTEXITCODE"
    }

    $EnrichedFiles = @(
        "collision\0688.rays.col",
        "data\combat_hurt_volumes.json",
        "data\emote_aliases_174.json",
        "data\emote_blend_trees_174.json",
        "data\voice_effects_174.json",
        "data\player_voice_174.json",
        "data\combat_voice_174.json",
        "data\map_environment_174.json",
        "data\junkenstein\junkenstein-npcs.json"
    )
    Write-Host "Enriched runtime data status:" -ForegroundColor DarkCyan
    foreach ($Rel in $EnrichedFiles) {
        $Full = Join-Path $RuntimeDir $Rel
        if (Test-Path $Full) {
            Write-Host "  [OK]      $Rel" -ForegroundColor Green
        } else {
            Write-Host "  [MISSING] $Rel" -ForegroundColor DarkYellow
        }
    }

    if ($PreflightOnly) {
        Write-Host "[4/4] PreflightOnly complete. Exiting cleanly." -ForegroundColor Green
        exit 0
    }

    New-Item -ItemType Directory -Force -Path $DataDir | Out-Null

    $ServerArgs = @("--runtime", $RuntimeDir, "--data-dir", $DataDir)
    if (-not [string]::IsNullOrWhiteSpace($Account)) {
        $ServerArgs += @("--account", $Account)
    }
    if ($DebugPort -gt 0) {
        $ServerArgs += @("--debug-port", $DebugPort.ToString())
    }
    if (-not [string]::IsNullOrWhiteSpace($ModScript)) {
        $ResolvedMod = (Resolve-Path $ModScript).Path
        $ServerArgs += @("--mod-script", $ResolvedMod)
    }
    if (-not [string]::IsNullOrWhiteSpace($AiScript)) {
        $ResolvedAi = (Resolve-Path $AiScript).Path
        $ServerArgs += @("--ai-script", $ResolvedAi)
    }

    Write-Host "[4/4] Starting ORPHEUS native server on loopback..." -ForegroundColor Green
    if ($DebugPort -gt 0) {
        Write-Host "      Web Scene Inspector : http://127.0.0.1:$DebugPort/" -ForegroundColor Cyan
    }
    if (-not [string]::IsNullOrWhiteSpace($ModScript)) {
        Write-Host "      Lua 5.4 Mod         : $ModScript (type 'reload' + Enter to hot-reload)" -ForegroundColor Cyan
    }
    Write-Host "      Legal notice        : Connect using your own local Overwatch 1.74.0.0.104319 client + relay." -ForegroundColor DarkGray
    Write-Host ""

    & $ServerExe @ServerArgs
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
