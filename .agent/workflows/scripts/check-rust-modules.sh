#!/bin/bash
# check-rust-modules.sh - Vérifie la cohérence des modules dans un projet Rust
# Usage: ./check-rust-modules.sh [répertoire]

set -e

DIR="${1:-.}"
cd "$DIR"

echo "═══════════════════════════════════════════════════════════"
echo "  VÉRIFICATION DES MODULES RUST"
echo "═══════════════════════════════════════════════════════════"
echo ""

# Vérifier que c'est un projet Rust
if [ ! -f "Cargo.toml" ]; then
    echo "❌ Pas de Cargo.toml trouvé. Ce n'est pas un projet Rust."
    exit 1
fi

# Extraire les workspace members si c'est un workspace
echo "📦 WORKSPACE MEMBERS"
echo "────────────────────"
if grep -q "\[workspace\]" Cargo.toml; then
    echo "  C'est un workspace Cargo."
    members=$(grep -A 20 "\[workspace\]" Cargo.toml | grep -E '^\s*"' | tr -d '", ' | head -20)
    for member in $members; do
        if [ -d "$member" ]; then
            if [ -f "$member/Cargo.toml" ]; then
                echo "  ✓ $member (Cargo.toml présent)"
            else
                echo "  ⚠️  $member (répertoire existe, Cargo.toml MANQUANT)"
            fi
        else
            echo "  ❌ $member (RÉPERTOIRE MANQUANT)"
        fi
    done
else
    echo "  Projet simple (pas de workspace)."
fi
echo ""

# Vérifier les points d'entrée
echo "🚀 POINTS D'ENTRÉE"
echo "──────────────────"
check_entry() {
    local dir=$1
    local name=$(basename "$dir")
    
    if [ -f "$dir/src/lib.rs" ]; then
        echo "  ✓ $name: src/lib.rs (bibliothèque)"
    elif [ -f "$dir/src/main.rs" ]; then
        echo "  ✓ $name: src/main.rs (binaire)"
    else
        echo "  ❌ $name: PAS DE POINT D'ENTRÉE (ni lib.rs ni main.rs)"
    fi
}

# Vérifier le projet principal
check_entry "."

# Vérifier les membres du workspace
if grep -q "\[workspace\]" Cargo.toml; then
    for member in $(grep -A 20 "\[workspace\]" Cargo.toml | grep -E '^\s*"' | tr -d '", ' | head -20); do
        if [ -d "$member" ]; then
            check_entry "$member"
        fi
    done
fi
echo ""

# Vérifier les déclarations de modules
echo "📋 MODULES DÉCLARÉS vs FICHIERS"
echo "────────────────────────────────"

check_modules() {
    local src_dir=$1
    local name=$2
    
    if [ ! -d "$src_dir" ]; then
        return
    fi
    
    echo "  [$name]"
    
    # Trouver les déclarations mod
    for entry_file in "$src_dir/lib.rs" "$src_dir/main.rs"; do
        if [ -f "$entry_file" ]; then
            # Extraire les modules déclarés
            declared_mods=$(grep -E "^(pub )?mod [a-z_]+" "$entry_file" 2>/dev/null | sed 's/.*mod \([a-z_]*\).*/\1/' || true)
            
            for mod in $declared_mods; do
                # Vérifier si le fichier ou répertoire existe
                if [ -f "$src_dir/$mod.rs" ]; then
                    echo "    ✓ mod $mod → $mod.rs"
                elif [ -f "$src_dir/$mod/mod.rs" ]; then
                    echo "    ✓ mod $mod → $mod/mod.rs"
                else
                    echo "    ❌ mod $mod → FICHIER MANQUANT"
                fi
            done
        fi
    done
    
    # Trouver les fichiers .rs non déclarés
    for rs_file in "$src_dir"/*.rs; do
        if [ -f "$rs_file" ]; then
            basename_file=$(basename "$rs_file" .rs)
            if [ "$basename_file" != "lib" ] && [ "$basename_file" != "main" ] && [ "$basename_file" != "mod" ]; then
                # Vérifier si ce module est déclaré
                if ! grep -qE "^(pub )?mod $basename_file" "$src_dir/lib.rs" 2>/dev/null && \
                   ! grep -qE "^(pub )?mod $basename_file" "$src_dir/main.rs" 2>/dev/null; then
                    echo "    ⚠️  $basename_file.rs existe mais N'EST PAS DÉCLARÉ"
                fi
            fi
        fi
    done
}

check_modules "src" "racine"

if grep -q "\[workspace\]" Cargo.toml; then
    for member in $(grep -A 20 "\[workspace\]" Cargo.toml | grep -E '^\s*"' | tr -d '", ' | head -20); do
        if [ -d "$member/src" ]; then
            check_modules "$member/src" "$member"
        fi
    done
fi
echo ""

# Tentative de cargo check
echo "🔨 VÉRIFICATION COMPILATION"
echo "───────────────────────────"
echo "  Exécution de cargo check..."
if cargo check 2>&1 | head -30; then
    echo ""
    echo "  ✓ Compilation OK"
else
    echo ""
    echo "  ❌ Erreurs de compilation détectées (voir ci-dessus)"
fi
echo ""

echo "═══════════════════════════════════════════════════════════"
echo "  VÉRIFICATION TERMINÉE"
echo "═══════════════════════════════════════════════════════════"
