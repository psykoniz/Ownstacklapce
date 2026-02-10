#!/bin/bash
# check-python-modules.sh - Vérifie la cohérence d'un projet Python
# Usage: ./check-python-modules.sh [répertoire]

set -e

DIR="${1:-.}"
cd "$DIR"

echo "═══════════════════════════════════════════════════════════"
echo "  VÉRIFICATION DU PROJET PYTHON"
echo "═══════════════════════════════════════════════════════════"
echo ""

# Détecter le type de configuration
echo "📦 CONFIGURATION DU PROJET"
echo "──────────────────────────"

if [ -f "pyproject.toml" ]; then
    echo "  ✓ pyproject.toml (moderne)"
    
    # Extraire les infos
    name=$(grep -E "^name\s*=" pyproject.toml | head -1 | sed 's/.*=\s*"\(.*\)"/\1/')
    version=$(grep -E "^version\s*=" pyproject.toml | head -1 | sed 's/.*=\s*"\(.*\)"/\1/')
    echo "  Nom: ${name:-N/A}"
    echo "  Version: ${version:-N/A}"
fi

if [ -f "setup.py" ]; then
    echo "  ✓ setup.py (legacy)"
fi

if [ -f "requirements.txt" ]; then
    echo "  ✓ requirements.txt"
    deps=$(wc -l < requirements.txt | tr -d ' ')
    echo "  Dépendances: $deps"
fi

if [ -f "setup.cfg" ]; then
    echo "  ✓ setup.cfg"
fi
echo ""

# Vérifier l'environnement virtuel
echo "🐍 ENVIRONNEMENT VIRTUEL"
echo "────────────────────────"
if [ -d "venv" ]; then
    echo "  ✓ venv/ présent"
elif [ -d ".venv" ]; then
    echo "  ✓ .venv/ présent"
elif [ -d "env" ]; then
    echo "  ✓ env/ présent"
else
    echo "  ⚠️  Pas d'environnement virtuel détecté"
fi
echo ""

# Trouver les packages Python
echo "📁 PACKAGES PYTHON"
echo "──────────────────"

find_packages() {
    find . -name "__init__.py" -type f 2>/dev/null | \
        grep -v -E "(venv|\.venv|env|node_modules|__pycache__|\.git)" | \
        sed 's/__init__.py//' | \
        sed 's/^\.\///' | \
        sed 's/\/$//' | \
        sort | uniq
}

packages=$(find_packages)
if [ -n "$packages" ]; then
    echo "$packages" | while read pkg; do
        if [ -n "$pkg" ]; then
            file_count=$(find "./$pkg" -name "*.py" -type f 2>/dev/null | wc -l | tr -d ' ')
            echo "  ✓ $pkg ($file_count fichiers .py)"
        fi
    done
else
    echo "  Aucun package trouvé (pas de __init__.py)"
fi
echo ""

# Vérifier les points d'entrée
echo "🚀 POINTS D'ENTRÉE"
echo "──────────────────"

for entry in "__main__.py" "main.py" "app.py" "run.py" "cli.py" "src/__main__.py" "src/main.py"; do
    if [ -f "$entry" ]; then
        echo "  ✓ $entry"
    fi
done

# Vérifier le entry_points dans pyproject.toml
if [ -f "pyproject.toml" ]; then
    if grep -q "\[project.scripts\]" pyproject.toml; then
        echo "  ✓ Scripts définis dans pyproject.toml"
    fi
    if grep -q "\[project.gui-scripts\]" pyproject.toml; then
        echo "  ✓ GUI scripts définis dans pyproject.toml"
    fi
fi
echo ""

# Analyser les imports
echo "🔗 ANALYSE DES IMPORTS"
echo "──────────────────────"

echo "  Imports les plus fréquents:"
find . -name "*.py" -type f 2>/dev/null | \
    grep -v -E "(venv|\.venv|env|__pycache__|\.git)" | \
    xargs grep -h "^import \|^from " 2>/dev/null | \
    sed 's/from \([a-zA-Z0-9_]*\).*/\1/' | \
    sed 's/import \([a-zA-Z0-9_]*\).*/\1/' | \
    sort | uniq -c | sort -rn | head -10
echo ""

# Vérifier la syntaxe
echo "🔨 VÉRIFICATION SYNTAXE"
echo "───────────────────────"

errors=0
while IFS= read -r pyfile; do
    if ! python3 -m py_compile "$pyfile" 2>/dev/null; then
        echo "  ❌ Erreur de syntaxe: $pyfile"
        errors=$((errors + 1))
    fi
done < <(find . -name "*.py" -type f 2>/dev/null | grep -v -E "(venv|\.venv|env|__pycache__|\.git)" | head -50)

if [ $errors -eq 0 ]; then
    echo "  ✓ Syntaxe OK (premiers 50 fichiers vérifiés)"
fi
echo ""

# Vérifier les outils de qualité
echo "🔍 OUTILS DE QUALITÉ"
echo "────────────────────"

for tool in "mypy.ini" ".mypy.ini" "pyproject.toml:mypy" "pytest.ini" "pyproject.toml:pytest" ".flake8" "setup.cfg:flake8" ".pre-commit-config.yaml"; do
    file=$(echo "$tool" | cut -d: -f1)
    section=$(echo "$tool" | cut -d: -f2)
    
    if [ "$file" = "$section" ]; then
        # Fichier simple
        if [ -f "$file" ]; then
            echo "  ✓ $file"
        fi
    else
        # Section dans fichier
        if [ -f "$file" ] && grep -q "\[$section\]" "$file" 2>/dev/null; then
            echo "  ✓ $section dans $file"
        fi
    fi
done
echo ""

echo "═══════════════════════════════════════════════════════════"
echo "  VÉRIFICATION TERMINÉE"
echo "═══════════════════════════════════════════════════════════"
