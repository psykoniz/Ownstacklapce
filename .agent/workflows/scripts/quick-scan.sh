#!/bin/bash
# quick-scan.sh - Scan rapide d'un projet pour identifier son type et sa structure
# Usage: ./quick-scan.sh [répertoire]

set -e

DIR="${1:-.}"
cd "$DIR"

echo "═══════════════════════════════════════════════════════════"
echo "  SCAN RAPIDE DU PROJET : $(pwd)"
echo "═══════════════════════════════════════════════════════════"
echo ""

# Détection du type de projet
echo "📦 TYPE DE PROJET"
echo "─────────────────"

detect_project_type() {
    if [ -f "Cargo.toml" ]; then
        echo "  ✓ Rust (Cargo.toml trouvé)"
        PROJECT_TYPE="rust"
    fi
    if [ -f "package.json" ]; then
        echo "  ✓ Node.js (package.json trouvé)"
        PROJECT_TYPE="node"
    fi
    if [ -f "go.mod" ]; then
        echo "  ✓ Go (go.mod trouvé)"
        PROJECT_TYPE="go"
    fi
    if [ -f "pyproject.toml" ] || [ -f "setup.py" ] || [ -f "requirements.txt" ]; then
        echo "  ✓ Python (config Python trouvée)"
        PROJECT_TYPE="python"
    fi
    if [ -f "pom.xml" ]; then
        echo "  ✓ Java Maven (pom.xml trouvé)"
        PROJECT_TYPE="java-maven"
    fi
    if [ -f "build.gradle" ] || [ -f "build.gradle.kts" ]; then
        echo "  ✓ Java/Kotlin Gradle (build.gradle trouvé)"
        PROJECT_TYPE="java-gradle"
    fi
    if ls *.csproj 1> /dev/null 2>&1 || ls **/*.csproj 1> /dev/null 2>&1; then
        echo "  ✓ C#/.NET (.csproj trouvé)"
        PROJECT_TYPE="dotnet"
    fi
}

detect_project_type
echo ""

# Comptage des fichiers
echo "📁 FICHIERS SOURCE"
echo "──────────────────"

count_files() {
    local ext=$1
    local count=$(find . -name "*.$ext" -type f 2>/dev/null | grep -v node_modules | grep -v target | grep -v vendor | grep -v __pycache__ | wc -l)
    if [ "$count" -gt 0 ]; then
        echo "  $ext: $count fichiers"
    fi
}

count_files "rs"
count_files "ts"
count_files "tsx"
count_files "js"
count_files "jsx"
count_files "py"
count_files "go"
count_files "java"
count_files "kt"
count_files "cs"
count_files "cpp"
count_files "c"
count_files "h"
echo ""

# Estimation LOC
echo "📊 LIGNES DE CODE (estimation)"
echo "──────────────────────────────"

if command -v cloc &> /dev/null; then
    cloc . --quiet --hide-rate 2>/dev/null | tail -n +3 | head -20
else
    echo "  (installer cloc pour un comptage précis)"
    # Estimation basique
    total_lines=$(find . -type f \( -name "*.rs" -o -name "*.ts" -o -name "*.js" -o -name "*.py" -o -name "*.go" -o -name "*.java" \) 2>/dev/null | \
        grep -v node_modules | grep -v target | grep -v vendor | \
        xargs wc -l 2>/dev/null | tail -1 | awk '{print $1}')
    echo "  Estimation brute: ${total_lines:-0} lignes"
fi
echo ""

# Structure des répertoires
echo "🌳 STRUCTURE (2 niveaux)"
echo "────────────────────────"
find . -maxdepth 2 -type d | grep -v -E "(node_modules|target|vendor|__pycache__|\.git|\.idea|\.vscode)" | head -30
echo ""

# Fichiers de configuration importants
echo "⚙️  FICHIERS DE CONFIG"
echo "─────────────────────"
for config in Cargo.toml package.json go.mod pyproject.toml requirements.txt setup.py pom.xml build.gradle tsconfig.json .gitignore Dockerfile docker-compose.yml Makefile; do
    if [ -f "$config" ]; then
        echo "  ✓ $config"
    fi
done
echo ""

# Git info
if [ -d ".git" ]; then
    echo "📜 HISTORIQUE GIT"
    echo "─────────────────"
    echo "  Dernier commit: $(git log -1 --format='%h - %s (%cr)' 2>/dev/null || echo 'N/A')"
    echo "  Branche: $(git branch --show-current 2>/dev/null || echo 'N/A')"
    echo "  Commits: $(git rev-list --count HEAD 2>/dev/null || echo 'N/A')"
    echo ""
fi

echo "═══════════════════════════════════════════════════════════"
echo "  SCAN TERMINÉ"
echo "═══════════════════════════════════════════════════════════"
