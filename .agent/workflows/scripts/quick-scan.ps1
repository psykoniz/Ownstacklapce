param (
    [string]$Dir = "."
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $Dir)) {
    Write-Host "Directory not found: $Dir" -ForegroundColor Red
    exit 1
}

Push-Location $Dir

Write-Host "═══════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "  SCAN RAPIDE DU PROJET : $(Get-Location)" -ForegroundColor Cyan
Write-Host "═══════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host ""

# Project Type Detection
Write-Host "📦 TYPE DE PROJET" -ForegroundColor Yellow
Write-Host "─────────────────" -ForegroundColor Yellow

$projectTypes = @()

if (Test-Path "Cargo.toml") {
    Write-Host "  ✓ Rust (Cargo.toml trouvé)" -ForegroundColor Green
    $projectTypes += "rust"
}
if (Test-Path "package.json") {
    Write-Host "  ✓ Node.js (package.json trouvé)" -ForegroundColor Green
    $projectTypes += "node"
}
if (Test-Path "go.mod") {
    Write-Host "  ✓ Go (go.mod trouvé)" -ForegroundColor Green
    $projectTypes += "go"
}
if ((Test-Path "pyproject.toml") -or (Test-Path "setup.py") -or (Test-Path "requirements.txt")) {
    Write-Host "  ✓ Python (config Python trouvée)" -ForegroundColor Green
    $projectTypes += "python"
}
if (Test-Path "pom.xml") {
    Write-Host "  ✓ Java Maven (pom.xml trouvé)" -ForegroundColor Green
    $projectTypes += "java-maven"
}
if ((Test-Path "build.gradle") -or (Test-Path "build.gradle.kts")) {
    Write-Host "  ✓ Java/Kotlin Gradle (build.gradle trouvé)" -ForegroundColor Green
    $projectTypes += "java-gradle"
}
if (Get-ChildItem -Filter "*.csproj" -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1) {
    Write-Host "  ✓ C#/.NET (.csproj trouvé)" -ForegroundColor Green
    $projectTypes += "dotnet"
}
Write-Host ""

# File Counts
Write-Host "📁 FICHIERS SOURCE" -ForegroundColor Yellow
Write-Host "──────────────────" -ForegroundColor Yellow

function Count-Files ($ext) {
    $count = (Get-ChildItem -Recurse -Filter "*.$ext" -File -ErrorAction SilentlyContinue | 
              Where-Object { $_.FullName -notmatch "node_modules|target|vendor|__pycache__" }).Count
    if ($count -gt 0) {
        Write-Host "  $ext : $count fichiers"
    }
}

$extensions = @("rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt", "cs", "cpp", "c", "h")
foreach ($ext in $extensions) { Count-Files $ext }
Write-Host ""

# LOC Estimation
Write-Host "📊 LIGNES DE CODE (estimation)" -ForegroundColor Yellow
Write-Host "──────────────────────────────" -ForegroundColor Yellow

if (Get-Command cloc -ErrorAction SilentlyContinue) {
    cloc . --quiet --hide-rate | Select-Object -Skip 2 | Select-Object -First 20
} else {
    Write-Host "  (installer cloc pour un comptage précis)" -ForegroundColor Gray
    $files = Get-ChildItem -Recurse -File -Include *.rs,*.ts,*.js,*.py,*.go,*.java -ErrorAction SilentlyContinue | 
             Where-Object { $_.FullName -notmatch "node_modules|target|vendor" }
    
    $totalLines = 0
    foreach ($file in $files) {
        $totalLines += (Get-Content $file.FullName | Measure-Object -Line).Lines
    }
    Write-Host "  Estimation brute: $totalLines lignes"
}
Write-Host ""

# Directory Structure
Write-Host "🌳 STRUCTURE (2 niveaux)" -ForegroundColor Yellow
Write-Host "────────────────────────" -ForegroundColor Yellow
Get-ChildItem -Directory -Recurse -Depth 2 -ErrorAction SilentlyContinue | 
    Where-Object { $_.FullName -notmatch "node_modules|target|vendor|__pycache__|\.git|\.idea|\.vscode" } | 
    Select-Object -First 30 -ExpandProperty FullName | 
    ForEach-Object { $_.Substring((Get-Location).Path.Length + 1) }
Write-Host ""

# Config Files
Write-Host "⚙️  FICHIERS DE CONFIG" -ForegroundColor Yellow
Write-Host "─────────────────────" -ForegroundColor Yellow
$configs = @("Cargo.toml", "package.json", "go.mod", "pyproject.toml", "requirements.txt", "setup.py", "pom.xml", "build.gradle", "tsconfig.json", ".gitignore", "Dockerfile", "docker-compose.yml", "Makefile")
foreach ($config in $configs) {
    if (Test-Path $config) {
        Write-Host "  ✓ $config" -ForegroundColor Green
    }
}
Write-Host ""

# Git Info
if (Test-Path ".git") {
    Write-Host "📜 HISTORIQUE GIT" -ForegroundColor Yellow
    Write-Host "─────────────────" -ForegroundColor Yellow
    try {
        $log = git log -1 --format='%h - %s (%cr)' 2>$null
        Write-Host "  Dernier commit: $log"
        $branch = git branch --show-current 2>$null
        Write-Host "  Branche: $branch"
        $count = git rev-list --count HEAD 2>$null
        Write-Host "  Commits: $count"
    } catch {
        Write-Host "  Info git non disponible"
    }
    Write-Host ""
}

Write-Host "═══════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "  SCAN TERMINÉ" -ForegroundColor Cyan
Write-Host "═══════════════════════════════════════════════════════════" -ForegroundColor Cyan

Pop-Location
