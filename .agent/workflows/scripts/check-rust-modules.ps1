param (
    [string]$Dir = "."
)

$ErrorActionPreference = "Stop"
if (-not (Test-Path $Dir)) { Write-Host "Dir not found"; exit 1 }
Push-Location $Dir

Write-Host "═══════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "  VÉRIFICATION MODULES RUST (PowerShell)" -ForegroundColor Cyan
Write-Host "═══════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host ""

if (-not (Test-Path "Cargo.toml")) {
    Write-Host "❌ Pas de Cargo.toml. Ce n'est pas un projet Rust." -ForegroundColor Red
    exit 1
}

$cargo = Get-Content "Cargo.toml" -Raw

Write-Host "📦 WORKSPACE MEMBERS" -ForegroundColor Yellow
Write-Host "────────────────────" -ForegroundColor Yellow

if ($cargo -match '\[workspace\]') {
    Write-Host "  C'est un workspace Cargo."
    # Extract members array regex roughly
    if ($cargo -match 'members\s*=\s*\[(.*?)\]') {
        $members = $matches[1] -split ',' | ForEach-Object { $_.Trim().Trim('"').Trim("'") } | Where-Object { $_ -ne "" }
        foreach ($member in $members) {
            if (Test-Path $member) {
                if (Test-Path "$member/Cargo.toml") {
                    Write-Host "  ✓ $member (Cargo.toml présent)" -ForegroundColor Green
                }
                else {
                    Write-Host "  ⚠️  $member (Cargo.toml MANQUANT)" -ForegroundColor Yellow
                }
            }
            else {
                Write-Host "  ❌ $member (RÉPERTOIRE MANQUANT)" -ForegroundColor Red
            }
        }
    }
}
else {
    Write-Host "  Projet simple (pas de workspace)."
}
Write-Host ""

Write-Host "🚀 POINTS D'ENTRÉE" -ForegroundColor Yellow
Write-Host "──────────────────" -ForegroundColor Yellow

function Check-Entry ($path, $name) {
    if (Test-Path "$path/src/lib.rs") {
        Write-Host "  ✓ $name : src/lib.rs (lib)" -ForegroundColor Green
    }
    elseif (Test-Path "$path/src/main.rs") {
        Write-Host "  ✓ $name : src/main.rs (bin)" -ForegroundColor Green
    }
    else {
        Write-Host "  ❌ $name : PAS DE POINT D'ENTRÉE" -ForegroundColor Red
    }
}

Check-Entry "." "Root"
if ($members) {
    foreach ($member in $members) {
        if (Test-Path $member) { Check-Entry $member $member }
    }
}
Write-Host ""

Write-Host "📋 MODULES DÉCLARÉS vs FICHIERS" -ForegroundColor Yellow
Write-Host "────────────────────────────────" -ForegroundColor Yellow

function Check-Modules ($path) {
    $src = "$path/src"
    if (-not (Test-Path $src)) { return }
    
    $entries = @("$src/lib.rs", "$src/main.rs")
    foreach ($entry in $entries) {
        if (Test-Path $entry) {
            $content = Get-Content $entry
            # Find 'mod xyz;' or 'pub mod xyz;'
            $mods = $content | Select-String -Pattern '^\s*(pub\s+)?mod\s+([a-z_0-9]+);'
            foreach ($match in $mods) {
                $modName = $match.Matches.Groups[2].Value
                if ((Test-Path "$src/$modName.rs") -or (Test-Path "$src/$modName/mod.rs")) {
                    Write-Host "  ✓ $path : mod $modName -> trouvé" -ForegroundColor Green
                }
                else {
                    Write-Host "  ❌ $path : mod $modName -> FICHIER MANQUANT" -ForegroundColor Red
                }
            }
        }
    }
}

Check-Modules "."
if ($members) {
    foreach ($member in $members) {
        if (Test-Path $member) { Check-Modules $member }
    }
}

Write-Host ""
Write-Host "═══════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "  VÉRIFICATION TERMINÉE" -ForegroundColor Cyan
Write-Host "═══════════════════════════════════════════════════════════" -ForegroundColor Cyan

Pop-Location
