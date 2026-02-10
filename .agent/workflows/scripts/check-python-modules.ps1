param (
    [string]$Dir = "."
)

$ErrorActionPreference = "Stop"
if (-not (Test-Path $Dir)) { Write-Host "Dir not found"; exit 1 }
Push-Location $Dir

Write-Host "═══════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "  VÉRIFICATION DU PROJET PYTHON (PowerShell)" -ForegroundColor Cyan
Write-Host "═══════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host ""

# Config
Write-Host "📦 CONFIGURATION DU PROJET" -ForegroundColor Yellow
Write-Host "──────────────────────────" -ForegroundColor Yellow

if (Test-Path "pyproject.toml") {
    Write-Host "  ✓ pyproject.toml (moderne)" -ForegroundColor Green
    $content = Get-Content "pyproject.toml" -Raw
    if ($content -match 'name\s*=\s*"(.*)"') { Write-Host "  Nom: $($matches[1])" }
    if ($content -match 'version\s*=\s*"(.*)"') { Write-Host "  Version: $($matches[1])" }
}

if (Test-Path "setup.py") { Write-Host "  ✓ setup.py (legacy)" -ForegroundColor Green }
if (Test-Path "requirements.txt") {
    Write-Host "  ✓ requirements.txt" -ForegroundColor Green
    $count = (Get-Content "requirements.txt" | Where-Object { $_ -match '\w' }).Count
    Write-Host "  Dépendances: $count"
}
Write-Host ""

# Venv
Write-Host "🐍 ENVIRONNEMENT VIRTUEL" -ForegroundColor Yellow
Write-Host "────────────────────────" -ForegroundColor Yellow
if (Test-Path "venv") { Write-Host "  ✓ venv/ présent" -ForegroundColor Green }
elseif (Test-Path ".venv") { Write-Host "  ✓ .venv/ présent" -ForegroundColor Green }
elseif (Test-Path "env") { Write-Host "  ✓ env/ présent" -ForegroundColor Green }
else { Write-Host "  ⚠️  Pas d'environnement virtuel détecté" -ForegroundColor Yellow }
Write-Host ""

# Packages
Write-Host "📁 PACKAGES PYTHON" -ForegroundColor Yellow
Write-Host "──────────────────" -ForegroundColor Yellow
$packages = Get-ChildItem -Recurse -Filter "__init__.py" -ErrorAction SilentlyContinue | 
    Where-Object { $_.FullName -notmatch "venv|\.venv|env|node_modules|__pycache__" } |
    ForEach-Object { $_.Directory.Name } | Select-Object -Unique

if ($packages) {
    foreach ($pkg in $packages) {
        $files = (Get-ChildItem -Recurse -Filter "*.py" -Path $pkg -ErrorAction SilentlyContinue).Count
        Write-Host "  ✓ $pkg ($files fichiers .py)" -ForegroundColor Green
    }
} else {
    Write-Host "  Aucun package trouvé (pas de __init__.py)"
}
Write-Host ""

# Entry Points
Write-Host "🚀 POINTS D'ENTRÉE" -ForegroundColor Yellow
Write-Host "──────────────────" -ForegroundColor Yellow
$entries = @("__main__.py", "main.py", "app.py", "run.py", "cli.py", "src/__main__.py", "src/main.py")
foreach ($entry in $entries) {
    if (Test-Path $entry) { Write-Host "  ✓ $entry" -ForegroundColor Green }
}
Write-Host ""

# Syntax Check
Write-Host "🔨 VÉRIFICATION SYNTAXE" -ForegroundColor Yellow
Write-Host "───────────────────────" -ForegroundColor Yellow
$files = Get-ChildItem -Recurse -Filter "*.py" -ErrorAction SilentlyContinue | 
         Where-Object { $_.FullName -notmatch "venv|\.venv|env|__pycache__" } | Select-Object -First 50

$errors = 0
foreach ($file in $files) {
    try {
        python -m py_compile "$($file.FullName)" 2>$null
        if ($LASTEXITCODE -ne 0) {
            Write-Host "  ❌ Erreur de syntaxe: $($file.Name)" -ForegroundColor Red
            $errors++
        }
    } catch {
        Write-Host "  ❌ Erreur d'exécution python" -ForegroundColor Red
        $errors++
    }
}

if ($errors -eq 0) { Write-Host "  ✓ Syntaxe OK (premiers 50 fichiers vérifiés)" -ForegroundColor Green }
Write-Host ""

Write-Host "═══════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "  VÉRIFICATION TERMINÉE" -ForegroundColor Cyan
Write-Host "═══════════════════════════════════════════════════════════" -ForegroundColor Cyan

Pop-Location
