# Codebase Explorer

Un skill pour explorer, analyser et diagnostiquer des projets de code de manière méthodique et efficace.

## Quand utiliser ce skill

Utilisez ce skill quand vous devez :
- Explorer un nouveau projet inconnu
- Diagnostiquer des problèmes de compilation ou de structure
- Comprendre l'architecture d'un codebase
- Vérifier l'intégrité des fichiers et modules
- Auditer les dépendances et configurations

## Philosophie

### Principes fondamentaux

| Principe | Application |
|----------|-------------|
| **Parallélisme maximal** | Lancer les opérations indépendantes en même temps |
| **Du macro au micro** | D'abord la vue d'ensemble, puis les détails ciblés |
| **Vérification par la preuve** | Chaque problème confirmé par un outil, pas de suppositions |
| **Outils dédiés > Bash** | Préférer Read/Grep/Glob aux commandes shell équivalentes |

### Workflow en 6 phases

```
Phase 1: Exploration globale (vision macro)
   └─ Glob, Grep, Read en masse → cartographie complète

Phase 2: Lecture ciblée (fichiers critiques)
   └─ Points d'entrée, configs, code custom

Phase 3: Vérification d'existence (pièces manquantes)
   └─ Répertoires manquants, fichiers absents

Phase 4: Analyse du chaînage (modules)
   └─ Module déclaré ? Utilisé ? Branché ?

Phase 5: Compilation/Validation
   └─ cargo check / npm run build / go build → confirmation des erreurs

Phase 6: Synthèse → rapport final structuré
```

---

## Phase 1 — Exploration globale

**Objectif** : Obtenir une cartographie complète du projet en un minimum d'appels.

### Étape 1.1 : Identifier le type de projet

Utilisez le script d'automatisation pour un scan initial rapide :

```bash
# // turbo
# Scan rapide (détection type, comptage fichiers, structure)
chmod +x .agent/workflows/scripts/quick-scan.sh
./.agent/workflows/scripts/quick-scan.sh .
```
**Windows (PowerShell) :**
```powershell
.agent/workflows/scripts/quick-scan.ps1 .
```

ou analysez manuellement si le script n'est pas disponible :

### Étape 1.2 : Cartographier les fichiers source

Selon le type détecté, lancez en **parallèle** :

**Rust :**
```bash
Glob("**/*.rs")
Glob("**/Cargo.toml")
Read("Cargo.toml")  # racine
Read("rust-toolchain.toml")
```

**Node.js :**
```bash
Glob("**/*.ts")
Glob("**/*.js")
Glob("**/*.tsx")
Read("package.json")
Read("tsconfig.json")
```

**Python :**
```bash
Glob("**/*.py")
Read("pyproject.toml")
Read("setup.py")
Read("requirements.txt")
```

**Go :**
```bash
Glob("**/*.go")
Read("go.mod")
Read("go.sum")
```

### Étape 1.3 : Lire les fichiers de documentation

```bash
# Parallèle
Read("README.md")
Read("CONTRIBUTING.md")
Read("docs/README.md")
Glob("docs/**/*.md")
```

### Métriques à collecter

À la fin de la Phase 1, vous devez avoir :

- [ ] Nombre total de fichiers source
- [ ] Estimation des lignes de code (LOC)
- [ ] Nombre de modules/packages/crates
- [ ] Liste des dépendances principales
- [ ] Points d'entrée identifiés
- [ ] Structure des répertoires (2 niveaux)

---

## Phase 2 — Lecture ciblée

**Objectif** : Lire les fichiers critiques pour comprendre l'architecture.

### Fichiers prioritaires par type de projet

**Rust :**
| Fichier | Raison |
|---------|--------|
| `Cargo.toml` (racine) | Workspace, members, dépendances |
| `src/main.rs` ou `src/lib.rs` | Point d'entrée |
| `*/src/lib.rs` | Points d'entrée des crates membres |

**Node.js :**
| Fichier | Raison |
|---------|--------|
| `package.json` | Scripts, dépendances, entry point |
| `src/index.ts` ou `index.js` | Point d'entrée |
| `tsconfig.json` | Configuration TypeScript |

**Python :**
| Fichier | Raison |
|---------|--------|
| `pyproject.toml` | Metadata, dépendances |
| `src/__init__.py` ou `__main__.py` | Point d'entrée |
| `setup.py` | Configuration legacy |

**Go :**
| Fichier | Raison |
|---------|--------|
| `go.mod` | Module, dépendances |
| `main.go` ou `cmd/*/main.go` | Points d'entrée |

### Stratégie de lecture

**Lancez les lectures en parallèle** quand les fichiers sont indépendants :

```bash
# Exemple Rust - 3 lectures parallèles
Read("/Cargo.toml")
Read("/src/main.rs")
Read("/custom-crate/src/lib.rs")
```

---

## Phase 3 — Vérification d'existence

**Objectif** : Confirmer que tous les fichiers/répertoires référencés existent.

### Méthode

1. Extraire les références depuis les fichiers de config
2. Vérifier chaque référence avec Glob ou Read
3. Documenter les absences

### Exemple Rust

Si `Cargo.toml` déclare :
```toml
[workspace]
members = ["crate-a", "crate-b", "crate-c"]
```

Lancez en **parallèle** :
```bash
Glob("crate-a/**/*")
Glob("crate-b/**/*")
Glob("crate-c/**/*")
Read("crate-a/Cargo.toml")
Read("crate-b/Cargo.toml")
Read("crate-c/Cargo.toml")
```

### Interprétation des résultats

| Résultat | Signification |
|----------|---------------|
| `Glob → "No files found"` | Répertoire inexistant ou vide |
| `Read → "File does not exist"` | Fichier manquant |
| `Read → contenu` | Fichier présent |

---

## Phase 4 — Analyse du chaînage

**Objectif** : Vérifier que les modules sont correctement déclarés et utilisés.

### Stratégie Grep

Utilisez Grep avec des patterns composés pour trouver :
1. Les déclarations de modules
2. Les utilisations/imports
3. Les références croisées

### Patterns par langage

**Rust :**
```bash
# Déclarations
Grep("mod MODULE_NAME", "src/")
Grep("pub mod MODULE_NAME", "src/")

# Utilisations
Grep("use crate::MODULE_NAME", "src/")
Grep("MODULE_NAME::", "src/")
```

**TypeScript/JavaScript :**
```bash
# Imports
Grep("import.*from.*MODULE", "src/")
Grep("require\\(.*MODULE", "src/")

# Exports
Grep("export.*MODULE", "src/")
```

**Python :**
```bash
# Imports
Grep("from MODULE import", "src/")
Grep("import MODULE", "src/")
```

**Go :**
```bash
# Imports
Grep("import.*MODULE", ".")
Grep("\".*MODULE\"", ".")
```

### Vérification de cohérence (Scripts Automatisés)

Utilisez les scripts dédiés pour une vérification approfondie :

**Rust :**
```bash
# // turbo
chmod +x .agent/workflows/scripts/check-rust-modules.sh
./.agent/workflows/scripts/check-rust-modules.sh .
```
*(Windows: `.agent/workflows/scripts/check-rust-modules.ps1`)*

**Node.js :**
```bash
# // turbo
chmod +x .agent/workflows/scripts/check-node-modules.sh
./.agent/workflows/scripts/check-node-modules.sh .
```
*(Windows: `.agent/workflows/scripts/check-node-modules.ps1`)*

**Python :**
```bash
# // turbo
chmod +x .agent/workflows/scripts/check-python-modules.sh
./.agent/workflows/scripts/check-python-modules.sh .
```
*(Windows: `.agent/workflows/scripts/check-python-modules.ps1`)*

---

## Phase 5 — Compilation/Validation

**Objectif** : Confirmer les erreurs par une tentative de build.

### Commandes par langage

| Langage | Commande rapide | Commande complète |
|---------|-----------------|-------------------|
| Rust | `cargo check 2>&1 \| head -80` | `cargo build 2>&1` |
| Node.js | `npm run build 2>&1 \| head -80` | `npm run build` |
| TypeScript | `tsc --noEmit 2>&1 \| head -80` | `tsc` |
| Python | `python -m py_compile main.py` | `mypy .` |
| Go | `go build ./... 2>&1 \| head -80` | `go build ./...` |

### Pourquoi `2>&1 | head -N` ?

- `2>&1` : Capture stderr (où les erreurs apparaissent)
- `head -N` : Limite la sortie pour éviter le flood
- Suffisant pour identifier le premier problème bloquant

### Analyse des erreurs

Les erreurs de compilation confirment généralement :
- Fichiers manquants
- Modules non déclarés
- Dépendances absentes
- Erreurs de syntaxe

---

## Phase 6 — Synthèse

**Objectif** : Produire un rapport structuré et actionnable.

### Template de rapport

Utilisez le template fourni dans `.agent/workflows/templates/report_template.md`.

```markdown
# Rapport d'analyse : [NOM_PROJET]
...
```

### Checklist de validation

Avant de soumettre votre rapport, vérifiez que vous avez couvert tous les points grâce à la checklist :
`.agent/workflows/templates/checklist.md`

---

## Commandes de référence rapide

### Exploration

```bash
# Structure des répertoires
ls -la
ls -R | head -100

# Historique Git
git log --oneline -20
git status

# Compter les lignes
find . -name "*.rs" | xargs wc -l
cloc . --quiet
```

### Recherche

```bash
# Trouver des patterns
Grep("pattern", "directory/")
Grep("error|warning", "src/")

# Trouver des fichiers
Glob("**/test*.rs")
Glob("**/*_test.go")
```

### Validation

```bash
# Vérifier la syntaxe
cargo check
npm run lint
go vet ./...
python -m py_compile *.py
```

---

## Anti-patterns à éviter

| ❌ Ne pas faire | ✅ Faire plutôt |
|----------------|-----------------|
| `cat file.txt` | `Read("file.txt")` |
| `grep pattern file` | `Grep("pattern", "file")` |
| `find . -name "*.rs"` | `Glob("**/*.rs")` |
| Lire les fichiers un par un | Lectures parallèles |
| Supposer qu'un fichier existe | Vérifier avec Glob/Read |
| Build complet pour diagnostiquer | `cargo check` / `tsc --noEmit` |

---

## Exemples d'utilisation

### Exemple 1 : Nouveau projet Rust inconnu

```
1. Glob("**/*.rs", "**/Cargo.toml")     → 143 fichiers Rust
2. Read("Cargo.toml")                    → 7 workspace members
3. Glob parallèle sur chaque member      → 2 manquants
4. Grep("mod ", "src/")                  → module non déclaré
5. cargo check                           → confirme l'erreur
6. Rapport avec 5 problèmes critiques
```

### Exemple 2 : Debug d'un projet Node.js

```
1. Read("package.json")                  → scripts, deps
2. Glob("src/**/*.ts")                   → 89 fichiers
3. Grep("import.*from", "src/")          → dépendances circulaires ?
4. npm run build                         → erreur TypeScript
5. Read du fichier mentionné             → type manquant
6. Solution proposée
```

---

## Checklist de diagnostic

- [ ] Type de projet identifié
- [ ] Fichiers source cartographiés
- [ ] Fichiers de config lus
- [ ] Dépendances listées
- [ ] Points d'entrée identifiés
- [ ] Modules/packages vérifiés
- [ ] Chaînage des imports vérifié
- [ ] Compilation testée
- [ ] Problèmes documentés avec preuves
- [ ] Solutions proposées
