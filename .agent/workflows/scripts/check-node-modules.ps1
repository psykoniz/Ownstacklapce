param (
    [string]$Dir = "."
)

$ErrorActionPreference = "Stop"
if (-not (Test-Path $Dir)) { Write-Host "Dir not found"; exit 1 }
Push-Location $Dir

Write-Host "═══════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "  VÉRIFICATION DU PROJET NODE.JS/TYPESCRIPT (PowerShell)" -ForegroundColor Cyan
Write-Host "═══════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host ""

if (-not (Test-Path "package.json")) {
    Write-Host "❌ Pas de package.json trouvé. Ce n'est pas un projet Node.js." -ForegroundColor Red
    exit 1
}

$pkg = Get-Content "package.json" -Raw | ConvertFrom-Json

Write-Host "📦 INFORMATIONS PACKAGE" -ForegroundColor Yellow
Write-Host "───────────────────────" -ForegroundColor Yellow
Write-Host "  Nom: $($pkg.name)"
Write-Host "  Version: $($pkg.version)"
Write-Host "  Point d'entrée: $($pkg.main)"
Write-Host ""

Write-Host "📘 TYPESCRIPT" -ForegroundColor Yellow
Write-Host "─────────────" -ForegroundColor Yellow

if (Test-Path "tsconfig.json") {
    try {
        # JSON standard doesn't support comments, but tsconfig often has them. 
        # We'll try to strip comments or just read simplistic regex if fails.
        $tsconfigContent = Get-Content "tsconfig.json" -Raw
        # Remove single line comments // ...
        $tsconfigContent = $tsconfigContent -replace "(?m)^\s*//.*$", ""
        $tsconfig = $tsconfigContent | ConvertFrom-Json
        
        Write-Host "  ✓ tsconfig.json présent" -ForegroundColor Green
        Write-Host "  rootDir: $($tsconfig.compilerOptions.rootDir)"
        Write-Host "  outDir: $($tsconfig.compilerOptions.outDir)"
    }
    catch {
        Write-Host "  ✓ tsconfig.json présent (erreur lecture JSON complexe)" -ForegroundColor Yellow
    }
}
else {
    Write-Host "  Pas de tsconfig.json" -ForegroundColor Gray
}
Write-Host ""

Write-Host "📚 DÉPENDANCES" -ForegroundColor Yellow
Write-Host "──────────────" -ForegroundColor Yellow
$deps = if ($pkg.dependencies) { $pkg.dependencies.PSObject.Properties.Count } else { 0 }
$devDeps = if ($pkg.devDependencies) { $pkg.devDependencies.PSObject.Properties.Count } else { 0 }
Write-Host "  Dependencies: $deps"
Write-Host "  DevDependencies: $devDeps"

if (-not (Test-Path "node_modules")) {
    Write-Host "  ⚠️  node_modules N'EXISTE PAS - exécuter 'npm install'" -ForegroundColor Red
}
Write-Host ""

Write-Host "📜 SCRIPTS DISPONIBLES" -ForegroundColor Yellow
Write-Host "──────────────────────" -ForegroundColor Yellow
if ($pkg.scripts) {
    $pkg.scripts.PSObject.Properties | Select-Object -First 10 | ForEach-Object {
        Write-Host "  $($_.Name): $($_.Value)"
    }
}
Write-Host ""

Write-Host "🔗 ANALYSE DES IMPORTS" -ForegroundColor Yellow
Write-Host "──────────────────────" -ForegroundColor Yellow
# Find local imports
Get-ChildItem -Recurse -Include *.ts, *.tsx, *.js, *.jsx -ErrorAction SilentlyContinue | 
Where-Object { $_.FullName -notmatch "node_modules" } |
Select-String -Pattern "from ['`"]\.\.?/" | 
Select-Object -First 10 | 
ForEach-Object { Write-Host "  $($_.Line.Trim())" -ForegroundColor Gray }
Write-Host ""

Write-Host "🔨 VÉRIFICATION TYPESCRIPT" -ForegroundColor Yellow
Write-Host "──────────────────────────" -ForegroundColor Yellow

if (Test-Path "tsconfig.json") {
    Write-Host "  Exécution de tsc --noEmit..."
    try {
        # npx might not be in path on windows sometimes, checks first
        if (Get-Command npx -ErrorAction SilentlyContinue) {
            cmd /c "npx tsc --noEmit" 2>&1 | Select-Object -First 20
            if ($LASTEXITCODE -eq 0) {
                Write-Host "  ✓ TypeScript OK" -ForegroundColor Green
            }
            else {
                Write-Host "  ❌ Erreurs TypeScript détectées" -ForegroundColor Red
            }
        }
        else {
            Write-Host "  (npx non trouvé, saut)" -ForegroundColor Gray
        }
    }
    catch {
        Write-Host "  Erreur exécution tsc" -ForegroundColor Red
    }
}

Write-Host ""
Write-Host "═══════════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "  VÉRIFICATION TERMINÉE" -ForegroundColor Cyan
Write-Host "═══════════════════════════════════════════════════════════" -ForegroundColor Cyan

Pop-Location
