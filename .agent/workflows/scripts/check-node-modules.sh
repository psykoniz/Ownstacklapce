#!/bin/bash
# check-node-modules.sh - Vérifie la cohérence d'un projet Node.js/TypeScript
# Usage: ./check-node-modules.sh [répertoire]

set -e

DIR="${1:-.}"
cd "$DIR"

echo "═══════════════════════════════════════════════════════════"
echo "  VÉRIFICATION DU PROJET NODE.JS/TYPESCRIPT"
echo "═══════════════════════════════════════════════════════════"
echo ""

# Vérifier que c'est un projet Node
if [ ! -f "package.json" ]; then
    echo "❌ Pas de package.json trouvé. Ce n'est pas un projet Node.js."
    exit 1
fi

# Info du package
echo "📦 INFORMATIONS PACKAGE"
echo "───────────────────────"
name=$(jq -r '.name // "N/A"' package.json)
version=$(jq -r '.version // "N/A"' package.json)
main=$(jq -r '.main // "N/A"' package.json)
echo "  Nom: $name"
echo "  Version: $version"
echo "  Point d'entrée: $main"
echo ""

# Vérifier TypeScript
echo "📘 TYPESCRIPT"
echo "─────────────"
if [ -f "tsconfig.json" ]; then
    echo "  ✓ tsconfig.json présent"
    
    # Vérifier les chemins importants
    root_dir=$(jq -r '.compilerOptions.rootDir // "N/A"' tsconfig.json)
    out_dir=$(jq -r '.compilerOptions.outDir // "N/A"' tsconfig.json)
    echo "  rootDir: $root_dir"
    echo "  outDir: $out_dir"
    
    if [ "$root_dir" != "N/A" ] && [ ! -d "$root_dir" ]; then
        echo "  ❌ rootDir '$root_dir' N'EXISTE PAS"
    fi
else
    echo "  Pas de tsconfig.json (JavaScript pur ou pas de TypeScript)"
fi
echo ""

# Vérifier les dépendances
echo "📚 DÉPENDANCES"
echo "──────────────"
deps=$(jq -r '.dependencies | length // 0' package.json)
dev_deps=$(jq -r '.devDependencies | length // 0' package.json)
echo "  Dependencies: $deps"
echo "  DevDependencies: $dev_deps"

if [ ! -d "node_modules" ]; then
    echo "  ⚠️  node_modules N'EXISTE PAS - exécuter 'npm install'"
fi
echo ""

# Vérifier les scripts
echo "📜 SCRIPTS DISPONIBLES"
echo "──────────────────────"
jq -r '.scripts | to_entries[] | "  \(.key): \(.value)"' package.json 2>/dev/null | head -10
echo ""

# Vérifier les points d'entrée
echo "🚀 POINTS D'ENTRÉE"
echo "──────────────────"

check_entry_exists() {
    local file=$1
    local description=$2
    if [ -f "$file" ]; then
        echo "  ✓ $description: $file"
    else
        echo "  ❌ $description: $file (MANQUANT)"
    fi
}

# Vérifier le main
if [ "$main" != "N/A" ]; then
    check_entry_exists "$main" "main"
fi

# Vérifier les entrées communes
for entry in "src/index.ts" "src/index.js" "index.ts" "index.js" "src/main.ts" "src/main.js"; do
    if [ -f "$entry" ]; then
        echo "  ✓ $entry"
    fi
done
echo ""

# Analyser les imports
echo "🔗 ANALYSE DES IMPORTS"
echo "──────────────────────"

# Trouver les imports qui pourraient être cassés
echo "  Imports locaux (./  ../): "
find . -name "*.ts" -o -name "*.tsx" -o -name "*.js" -o -name "*.jsx" 2>/dev/null | \
    grep -v node_modules | \
    xargs grep -h "from ['\"]\.\.?/" 2>/dev/null | \
    sed "s/.*from ['\"]//g" | sed "s/['\"].*//g" | \
    sort | uniq -c | sort -rn | head -10

echo ""

# Vérifier la compilation TypeScript
if [ -f "tsconfig.json" ]; then
    echo "🔨 VÉRIFICATION TYPESCRIPT"
    echo "──────────────────────────"
    echo "  Exécution de tsc --noEmit..."
    if npx tsc --noEmit 2>&1 | head -30; then
        echo "  ✓ TypeScript OK"
    else
        echo "  ❌ Erreurs TypeScript détectées"
    fi
    echo ""
fi

# Vérifier ESLint si présent
if [ -f ".eslintrc.js" ] || [ -f ".eslintrc.json" ] || [ -f ".eslintrc" ]; then
    echo "🔍 ESLINT"
    echo "─────────"
    echo "  Configuration ESLint trouvée"
fi

echo ""
echo "═══════════════════════════════════════════════════════════"
echo "  VÉRIFICATION TERMINÉE"
echo "═══════════════════════════════════════════════════════════"
