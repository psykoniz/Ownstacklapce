# GEMINI.md — Directives Agent pour OwnStack Native IDE

> **DOCUMENT CRITIQUE** : Ce fichier est la source de vérité pour tout agent IA travaillant sur ce repository.
> Toute action qui contredit ce fichier est **interdite**.
> En cas de doute, **STOP** et demande confirmation à l'humain.

---

## 🔒 RÈGLE ABSOLUE NUMÉRO ZÉRO

```
AVANT TOUTE MODIFICATION STRUCTURELLE, LIRE :
  → docs/ARCHITECTURE.md (plan complet)
  → Ce fichier GEMINI.md (directives agent)

SI TU N'AS PAS LU CES DEUX FICHIERS → NE FAIS RIEN.
```

---

## 📋 TABLE DES MATIÈRES

1. [Identité du projet](#1-identité-du-projet)
2. [Architecture imposée](#2-architecture-imposée)
3. [Phases et roadmap](#3-phases-et-roadmap)
4. [Règles de code](#4-règles-de-code)
5. [Fichiers protégés](#5-fichiers-protégés)
6. [Sécurité — Règles inviolables](#6-sécurité--règles-inviolables)
   - 6.1 Commandes interdites dans le code
   - 6.2 Policy Engine
   - 6.3 Path Safety
   - 6.4 Audit
   - 6.5 Sandbox
   - 6.6 Secrets
   - **6.7 Mode Auto-Approve (Autopilot)**
   - **6.8 Budgets d'exécution agent (anti-runaway)**
   - **6.9 Kill-Switch agent (arrêt forcé)**
   - **6.10 Erreurs critiques non récupérables**
   - **6.11 Traçabilité du mode agent**
7. [Outils et commandes autorisés](#7-outils-et-commandes-autorisés)
8. [Patterns obligatoires](#8-patterns-obligatoires)
9. [Anti-patterns interdits](#9-anti-patterns-interdits)
10. [Checklist de validation](#10-checklist-de-validation)
11. [Protocole de décision](#11-protocole-de-décision)
12. [Gestion des erreurs](#12-gestion-des-erreurs)
13. [Tests obligatoires](#13-tests-obligatoires)
14. [Git et branches](#14-git-et-branches)
15. [Garde-fous automatiques](#15-garde-fous-automatiques)

---

## 1. IDENTITÉ DU PROJET

```yaml
nom: OwnStack Native IDE
base: Fork de Lapce (https://github.com/lapce/lapce)
licence_lapce: Apache 2.0
licence_ownstack: MIT
langage_principal: Rust
langage_secondaire: Python (sidecar OwnStack backend)
ui_framework: Floem (Rust natif, PAS Electron, PAS Tauri)
rendu: wgpu (GPU natif)
commit_reference_ownstack: f6c2d2c1759b3b48f132f783e4fed592105e5ad2
repo_ownstack_source: https://github.com/psykoniz/Ownstack
```

### Ce que ce projet EST :
- Un IDE Rust-first, rapide, sécurisé
- Un fork de Lapce avec OwnStack comme noyau IA interne
- Un produit indépendant de VS Code et Electron
- Un IDE orienté agents IA avec exécution contrôlée

### Ce que ce projet N'EST PAS :
- Une extension VS Code
- Un wrapper Electron
- Un clone de Cursor/Windsurf
- Un IDE from scratch (c'est un fork Lapce)
- Un prototype jetable

---

## 2. ARCHITECTURE IMPOSÉE

### 2.1 Structure du monorepo — NE PAS MODIFIER

```
ownstack-ide/
├── Cargo.toml                  # Workspace Rust (6 membres)
├── LICENSE                     # Apache 2.0 (Lapce original, NE PAS TOUCHER)
├── LICENSE-OWNSTACK            # MIT (composants OwnStack)
├── NOTICE                      # Attribution Lapce + OwnStack
├── GEMINI.md                   # CE FICHIER
├── AGENTS.md                   # Directives Codex
│
│  ── CRATES LAPCE (hérités, modifications minimales) ──
├── lapce-app/                  # GUI Floem — modifier uniquement pour :
│   └── src/                    #   → ajout panels IA
│       ├── app.rs              #   → ajout ownstack_palette.rs
│       ├── editor.rs           #   → ajout ownstack_chat.rs
│       ├── palette.rs          #   → extension commandes IA
│       ├── terminal.rs         #   → pont sandbox OwnStack
│       └── ...
├── lapce-core/                 # Core editor — modifier uniquement pour :
│   └── src/                    #   → ajout types commandes IA
│       ├── command.rs          #   → OwnStackCommand enum
│       └── ...
├── lapce-proxy/                # Proxy process — modifier uniquement pour :
│   └── src/                    #   → routage vers OwnStack bridge
│       ├── dispatch.rs         #   → ajout handlers OwnStack
│       └── ...
├── lapce-rpc/                  # Protocole RPC — modifier pour :
│   └── src/                    #   → OwnStackRpcMessage
│       └── ...
│
│  ── CRATES OWNSTACK (nouveaux, notre code) ──
├── ownstack-engine/            # ★ Noyau sécurité (Rust pur)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs              # Exports publics
│       ├── policy.rs           # PolicyEngine (Ask/Auto/Blocked)
│       ├── audit.rs            # AuditLogger (JSONL)
│       ├── path_safety.rs      # PathValidator (canonicalize)
│       ├── security.rs         # Security layer
│       ├── tool_result.rs      # ToolResult struct
│       └── sandbox/
│           ├── mod.rs
│           ├── process.rs      # Process sandbox (seccomp/namespace)
│           └── docker.rs       # Docker sandbox (optionnel)
│
├── ownstack-agent/             # ★ Agent IA (Rust)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── provider.rs         # Trait LlmProvider
│       ├── providers/
│       │   ├── openrouter.rs
│       │   ├── anthropic.rs
│       │   └── local.rs        # Ollama/llama.cpp
│       ├── context.rs          # Context window management
│       ├── toolkits/
│       │   ├── mod.rs
│       │   ├── core.rs         # exec, read, write, search
│       │   ├── lsp.rs          # LSP operations
│       │   └── mcp.rs          # MCP bridge
│       └── orchestrator.rs     # Multi-agent
│
├── ownstack-bridge/            # ★ Bridge Rust ↔ Python
│   ├── Cargo.toml
│   └── src/
│       └── lib.rs              # JSON-RPC stdio spawn
│
├── ownstack-python/            # ★ Backend Python (copie depuis f6c2d2c)
│   └── app/                    # Structure identique au commit source
│       ├── agent/
│       ├── api/endpoints/
│       ├── core/
│       ├── runtime/
│       ├── tools/
│       ├── missions/
│       ├── utils/
│       ├── main.py
│       └── bridge_rpc.py       # ★ Entry point pour communication Rust
│
├── defaults/
├── icons/
├── extra/
└── docs/
    ├── ARCHITECTURE.md         # ★ LIRE EN PREMIER
    ├── INTEGRATION.md
    └── OPERATIONS.md
```

### 2.2 Cargo.toml workspace — FORMAT EXACT

```toml
[workspace]
members = [
    "lapce-app",
    "lapce-core",
    "lapce-proxy",
    "lapce-rpc",
    "ownstack-engine",
    "ownstack-bridge",
    "ownstack-agent",
]
resolver = "2"
```

> **INTERDIT** : Ajouter des membres au workspace sans mettre à jour cette section ET docs/ARCHITECTURE.md.

### 2.3 Diagramme de flux obligatoire

Toute exécution de commande IA DOIT suivre ce flux exact :

```
Entrée utilisateur (palette/chat)
    │
    ▼
[1] PolicyEngine.evaluate(command)        ← ownstack-engine/policy.rs
    ├─ Blocked → STOP + audit log + notif UI
    ├─ Ask → prompt user via RPC → GUI
    │       ├─ Approved → continue
    │       └─ Denied → STOP + audit log
    └─ Auto → continue
    │
    ▼
[2] PathValidator.validate(paths)          ← ownstack-engine/path_safety.rs
    ├─ Outside workspace → STOP + audit log
    └─ Inside workspace → continue
    │
    ▼
[3] Sandbox.exec(command)                  ← ownstack-engine/sandbox/
    │   env_clear, timeout, network:none
    │
    ▼
[4] ToolResult { success, stdout, stderr } ← ownstack-engine/tool_result.rs
    │
    ▼
[5] AuditLogger.log(entry)                ← ownstack-engine/audit.rs
    │
    ▼
[6] Retour vers GUI (RPC)
```

> **VIOLATION DE CE FLUX = REJET AUTOMATIQUE.**
> Aucun outil, aucune commande, aucun toolkit ne peut bypasser les étapes 1-2-5.

---

## 3. PHASES ET ROADMAP

### Phase actuelle : déterminer avant toute action

```
AVANT DE CODER, VÉRIFIE LA PHASE ACTUELLE :

  Phase 0 (Semaines 1-3)   : Fork + rebrand uniquement
  Phase 1 (Semaines 4-10)  : OwnStack embedded (sidecar Python)
  Phase 2 (Semaines 11-18) : Agents IA natifs Rust
  Phase 3 (Semaines 19-26) : MCP + plugins
  Phase 4 (Semaines 27-32) : Distribution

RÈGLE : Ne JAMAIS implémenter du code d'une phase future.
  - En Phase 0 → PAS de code IA, PAS de bridge Python
  - En Phase 1 → PAS de multi-agent, PAS de MCP
  - En Phase 2 → PAS de plugins dynamiques
```

### 3.1 Phase 0 — Tâches exhaustives

```yaml
checklist:
  - fork_github: "lapce/lapce → org/ownstack-ide"
  - renommer_binaire:
      fichier: "lapce-app/Cargo.toml"
      section: "[[bin]]"
      valeur: 'name = "ownstack-ide"'
  - ajouter_fichiers:
      - NOTICE
      - LICENSE-OWNSTACK
      - GEMINI.md
      - AGENTS.md
  - modifier_splash:
      fichier: "lapce-app/src/app.rs"
      action: "Remplacer 'Lapce' par 'OwnStack IDE' dans le titre fenêtre"
  - modifier_about:
      action: "About dialog → 'OwnStack IDE — Based on Lapce'"
  - ci_github_actions:
      copier: ".github/workflows/ci.yml"
      adapter: "Nom du binaire, artifacts"
  - readme:
      action: "Réécrire README.md pour OwnStack Native"
  - verification:
      commande: "cargo build --release"
      attendu: "Binaire ./target/release/ownstack-ide fonctionnel"

NE PAS FAIRE en Phase 0:
  - Créer ownstack-engine/
  - Créer ownstack-bridge/
  - Modifier lapce-proxy/dispatch.rs
  - Ajouter du code Python
  - Toucher au système de commandes
```

### 3.2 Phase 1 — Tâches exhaustives

```yaml
prerequis: "Phase 0 complète et vérifiée"

checklist:
  - creer_crate_engine:
      chemin: "ownstack-engine/"
      fichiers:
        - Cargo.toml
        - src/lib.rs
        - src/policy.rs
        - src/audit.rs
        - src/path_safety.rs
        - src/security.rs
        - src/tool_result.rs
        - src/sandbox/mod.rs
        - src/sandbox/process.rs
        - src/sandbox/docker.rs
      deps:
        - serde = { version = "1", features = ["derive"] }
        - serde_json = "1"
        - chrono = { version = "0.4", features = ["serde"] }
        - thiserror = "1"
        - regex = "1"
        - tokio = { version = "1", features = ["full"] }
        - tracing = "0.1"

  - creer_crate_bridge:
      chemin: "ownstack-bridge/"
      fichiers:
        - Cargo.toml
        - src/lib.rs

  - copier_python:
      source: "commit f6c2d2c → backend/app/"
      destination: "ownstack-python/app/"
      ajouter: "ownstack-python/app/bridge_rpc.py"

  - etendre_rpc:
      fichier: "lapce-rpc/src/"
      ajouter: "OwnStackRpcMessage enum complet"
      types:
        - AiPrompt
        - ToolExec
        - PolicyOverride
        - AiStreamChunk
        - PolicyPrompt
        - AuditEvent
        - ToolResultMsg

  - modifier_proxy:
      fichier: "lapce-proxy/src/dispatch.rs"
      action: "Ajouter handler pour messages OwnStack → bridge"

  - ajouter_ui:
      fichiers:
        - "lapce-app/src/ownstack_palette.rs"
        - "lapce-app/src/ownstack_chat.rs"
        - "lapce-app/src/ownstack_audit.rs"
      modifier:
        - "lapce-app/src/app.rs"
        - "lapce-app/src/palette.rs"

  - status_bar:
      action: "Ajouter indicateur mode Ask/Auto/Plan"

  - workspace_toml:
      fichier: "Cargo.toml"
      action: "Ajouter ownstack-engine et ownstack-bridge aux members"

  - tests:
      - "cargo test -p ownstack-engine"
      - "cargo test -p ownstack-bridge"
      - "cargo build --release"

NE PAS FAIRE en Phase 1:
  - Créer ownstack-agent/
  - Implémenter LlmProvider en Rust
  - Porter les toolkits en Rust
  - Implémenter MCP
  - Multi-agent orchestration
```

### 3.3 Phase 2 — Tâches exhaustives

```yaml
prerequis: "Phase 1 complète, policy engine + bridge fonctionnels"

checklist:
  - creer_crate_agent:
      chemin: "ownstack-agent/"
      fichiers:
        - Cargo.toml
        - src/lib.rs
        - src/provider.rs
        - src/providers/openrouter.rs
        - src/providers/anthropic.rs
        - src/providers/local.rs
        - src/context.rs
        - src/toolkits/mod.rs
        - src/toolkits/core.rs
        - src/toolkits/lsp.rs
        - src/toolkits/mcp.rs  # stub uniquement
        - src/orchestrator.rs
      deps:
        - reqwest = { version = "0.12", features = ["json", "stream"] }
        - tokio = { version = "1", features = ["full"] }
        - futures = "0.3"
        - async-trait = "0.1"
        - ownstack-engine = { path = "../ownstack-engine" }

  - porter_healer:
      source: "ownstack-python/app/agent/healer.py"
      destination: "ownstack-agent/src/toolkits/healer.rs"

  - porter_multivers:
      source: "ownstack-python/app/runtime/multivers.py"
      destination: "ownstack-agent/src/toolkits/multivers.rs"

  - streaming_ui:
      action: "SSE agent → GUI via RPC (AiStreamChunk)"

  - diff_preview:
      action: "Afficher diff dans le chat panel"

  - accept_reject:
      action: "Boutons Accept/Reject/Discuss sur suggestions agent"

  - sandbox_rust:
      action: "Sandbox process natif (seccomp Linux, sandbox-exec macOS)"
      note: "Doit fonctionner SANS Docker"

  - project_memory:
      action: "Lecture .ownstack/rules.md par l'agent"

  - mission_system:
      action: "Planification multi-étapes basique"
```

### 3.4 Phase 3 — Tâches exhaustives

```yaml
prerequis: "Phase 2 complète, agent Rust natif fonctionnel"

checklist:
  - mcp_client: "Client MCP natif Rust complet"
  - mcp_server: "Exposer tools OwnStack via MCP"
  - plugin_system: "Toolkits chargés dynamiquement (WASI)"
  - openclaw_rust: "Multi-agent: Planner + Critic + Worker"
  - git_integration: "Staging contextuel, commit assisté par IA"
  - doc_developpeur: "Guide pour créer des toolkits OwnStack"
```

### 3.5 Phase 4 — Tâches exhaustives

```yaml
prerequis: "Phase 3 complète"

checklist:
  - installers:
      linux: [".deb", ".rpm", "AppImage", "Flatpak"]
      macos: [".dmg signé", "Homebrew cask"]
      windows: [".msi signé", "winget", "Scoop"]
  - auto_updater: "Sparkle (macOS), WinSparkle (Windows), custom (Linux)"
  - onboarding: "Wizard premier lancement"
  - python_bundling: "Inclure Python runtime pour ownstack-python"
  - release: "v0.1.0"
```

---

## 4. RÈGLES DE CODE

### 4.1 Rust — Standards obligatoires

```yaml
edition: "2021"
format: "rustfmt (config dans .rustfmt.toml existant)"
lints: ["clippy::all", "clippy::pedantic"]
erreurs: "thiserror pour les types d'erreurs"
interdit: ["unwrap() en prod", "panic!() en prod", "println!() en prod"]
async: "tokio unique"
serialisation: "serde + serde_json partout"
logging: "tracing (pas log, pas println!)"
```

### 4.2 Python (ownstack-python/) — Standards obligatoires

```yaml
version: "3.11+"
framework: "FastAPI + uvicorn"
format: "black + isort"
tests: "pytest"
deps: "requirements.txt"
interdit: ["print()", "import *", "eval()", "exec()", "secrets en dur"]
bridge: "bridge_rpc.py est le SEUL entry point pour la communication Rust"
```

### 4.3 Nommage obligatoire

```yaml
crates_rust:
  ownstack-engine: "Noyau sécurité — policy, audit, path, sandbox"
  ownstack-agent: "Agent IA — providers, toolkits, orchestrator"
  ownstack-bridge: "Bridge Rust ↔ Python"

fichiers_nouveaux_dans_lapce: "ownstack_*.rs (TOUJOURS préfixer)"

modules_python: "Pas de renommage des modules existants d'OwnStack"
```

---

## 5. FICHIERS PROTÉGÉS

### 5.1 Fichiers JAMAIS modifiables par l'agent

```
LICENSE                          # Apache 2.0 Lapce — INTOUCHABLE
lapce-core/src/buffer.rs         # Xi Rope — ne pas toucher
lapce-core/src/syntax.rs         # Tree-sitter — ne pas toucher
.rustfmt.toml                    # Format Rust — ne pas toucher
.taplo.toml                      # Format TOML — ne pas toucher
deny.toml                        # Audit deps — ne pas toucher
```

### 5.2 Fichiers modifiables UNIQUEMENT de manière additive

```
lapce-app/src/app.rs             # AJOUTER panels, pas supprimer existants
lapce-app/src/palette.rs         # AJOUTER commandes, pas modifier existantes
lapce-proxy/src/dispatch.rs      # AJOUTER handlers, pas modifier existants
lapce-rpc/src/core.rs            # AJOUTER types messages, pas modifier existants
Cargo.toml (workspace)           # AJOUTER members, pas supprimer existants
```

### 5.3 Fichiers librement modifiables

```
ownstack-engine/**               # Notre code, libre
ownstack-agent/**                # Notre code, libre
ownstack-bridge/**               # Notre code, libre
ownstack-python/**               # Notre code, libre
docs/**                          # Documentation, libre
lapce-app/src/ownstack_*.rs      # Nos fichiers dans lapce-app
README.md                        # Libre
```

---

## 6. SÉCURITÉ — RÈGLES INVIOLABLES

### 6.1 Commandes INTERDITES dans le code généré

```rust
// L'agent NE DOIT JAMAIS générer de code Rust qui :
// - Utilise unsafe {} sans commentaire "// SAFETY:" documenté
// - Utilise std::process::Command sans passer par le sandbox
// - Désactive les checks du PolicyEngine
// - Hardcode des secrets/API keys
// - Utilise unwrap() sur des entrées utilisateur
// - Utilise shell=true dans tout subprocess
```

```python
# L'agent NE DOIT JAMAIS générer de code Python qui :
# - os.system()
# - subprocess.call(shell=True)
# - eval() / exec() hors sandbox strict
# - import pickle
# - __import__
# - globals() pour modifier des modules
```

### 6.2 Policy Engine — Patterns obligatoires

```
TOUTE commande exécutée par l'agent IA DOIT passer par PolicyEngine.

TOUJOURS BLOQUÉ (PolicyDecision::Blocked) :
  rm -rf /          sudo *               chmod 777 *
  curl * | sh       curl * | bash        wget * | sh
  mkfs *            dd if=*              > /dev/sd*
  > /dev/nvme*      :(){ :|:& };:        shutdown
  reboot            halt                  init 0
  kill -9 1         echo * > /etc/*      mount *
  umount *

CONFIRMATION REQUISE (PolicyDecision::Ask) :
  git push *         git reset --hard *   git force *
  npm publish *      cargo publish *      docker rm *
  docker rmi *       rm -rf (workspace)   curl/wget (réseau)

AUTO (PolicyDecision::Auto) :
  Lecture fichiers workspace              grep, find, cat, head, tail
  cargo build/test/check                  pytest, npm test
  Écriture fichiers workspace             git add/commit/status/diff/log
```

### 6.3 Path Safety — Règles absolues

```
1. canonicalize() OBLIGATOIRE avant toute opération
2. Le résultat DOIT commencer par workspace_root
3. Les symlinks sont résolus AVANT la validation
4. ".." dans le chemin → REJET IMMÉDIAT
5. Chemins absolus hors workspace → REJET IMMÉDIAT
6. /etc, /usr, /bin, /sbin, /var → REJET
7. ~ (home) hors workspace → REJET
```

### 6.4 Audit — Toute action est loguée

```json
// Format OBLIGATOIRE dans .ownstack/audit.jsonl
{
  "timestamp": "ISO8601",
  "session_id": "uuid",
  "action": "exec|read|write|delete|search",
  "command": "la commande exacte",
  "policy_decision": "Auto|Ask|Blocked",
  "tool_name": "nom du toolkit",
  "success": true,
  "duration_ms": 123,
  "workspace": "/path/to/workspace",
  "user_approved": null,
  "paths_accessed": ["liste", "des", "chemins"]
}
```

> **RÈGLE** : Si l'audit log n'est pas écrit → l'action N'A PAS EU LIEU.
> En cas d'erreur d'écriture audit → ABORT l'action.

### 6.5 Sandbox — Isolation obligatoire

```
Propriétés OBLIGATOIRES :
  ✅ env_clear()            → Aucune variable héritée
  ✅ PATH = /usr/bin:/bin    → Minimal
  ✅ current_dir(workspace)  → Jamais hors workspace
  ✅ timeout 300s            → Configurable
  ✅ Pas d'accès réseau      → Par défaut
  ✅ Pas de sudo             → User normal
  ✅ cap_drop ALL            → Docker / seccomp process

Niveaux :
  Niveau 1 (léger)    : process sandbox → cat, grep, ls
  Niveau 2 (standard) : process étendu  → cargo, npm, pytest
  Niveau 3 (strict)   : Docker container → scripts inconnus
```

### 6.6 Secrets — Gestion obligatoire

```
INTERDIT : Hardcoder des API keys
INTERDIT : Committer des fichiers .env avec secrets
INTERDIT : Logger des API keys dans l'audit
OBLIGATOIRE : Variables d'environnement uniquement
OBLIGATOIRE : .gitignore contient .env, *.key, *.pem, .ownstack/secrets/
```

### 6.7 Mode Auto-Approve (Autopilot)

Le mode Auto-Approve permet à l'agent d'agir sans intervention humaine,
tout en respectant **strictement** les règles de sécurité du projet.

```
COMPORTEMENT PAR MODE :

  Mode Ask (défaut) :
    - PolicyDecision::Auto     → exécuté automatiquement
    - PolicyDecision::Ask      → prompt utilisateur obligatoire
    - PolicyDecision::Blocked  → TOUJOURS refusé

  Mode Auto :
    - PolicyDecision::Auto     → exécuté automatiquement
    - PolicyDecision::Ask      → exécuté automatiquement (sans prompt)
    - PolicyDecision::Blocked  → TOUJOURS refusé

  Mode Auto-Approve (Autopilot) :
    - PolicyDecision::Auto     → exécuté automatiquement
    - PolicyDecision::Ask      → approuvé automatiquement
    - PolicyDecision::Blocked  → TOUJOURS refusé (NON CONTOURNABLE)

INVARIANTS (valides QUEL QUE SOIT le mode) :
  ✅ Le PolicyEngine est TOUJOURS évalué
  ✅ Le PathValidator est TOUJOURS appliqué
  ✅ La Sandbox est TOUJOURS utilisée
  ✅ L'AuditLogger est TOUJOURS écrit
  ✅ Les commandes Blocked sont TOUJOURS refusées

INTERDIT :
  ❌ Bypasser le PolicyEngine dans AUCUN mode
  ❌ Reclassifier une commande Blocked → Ask ou Auto
  ❌ Désactiver l'audit ou la sandbox en mode Autopilot
  ❌ Modifier ces règles sans validation humaine
  ❌ Activer Auto-Approve sans action explicite de l'utilisateur

AUDIT :
  Tout changement de mode est enregistré :
  {
    "action": "mode_change",
    "from": "Ask",
    "to": "Auto-Approve",
    "timestamp": "ISO8601",
    "user_initiated": true
  }

UI :
  Le mode actif doit être :
  - Explicitement visible dans la status bar
  - Visuellement distinct (couleur/icône différente par mode)
  - Modifiable uniquement par action utilisateur (pas par l'agent)
```

### 6.8 Budgets d'exécution agent (anti-runaway) — OBLIGATOIRES

Toute mission agent (interactive ou autonome) est soumise à des **budgets stricts**
afin d'éviter les boucles infinies, l'emballement ou les comportements destructeurs.

```
BUDGETS PAR DÉFAUT :

  max_steps: 50                    # Nombre total d'étapes agent
  max_exec_commands: 30            # Commandes shell exécutées
  max_files_modified: 20           # Fichiers créés/modifiés/supprimés
  max_total_diff_lines: 2000       # Lignes ajoutées + supprimées
  max_duration_minutes: 30         # Durée totale de la mission
  max_consecutive_failures: 3      # Échecs consécutifs sans succès
  max_llm_calls: 100               # Appels API au provider LLM
  max_sandbox_spawns: 20           # Nombre de process sandbox lancés

COMPORTEMENT AU DÉPASSEMENT :

  1. STOP immédiat de toute exécution en cours
  2. Les subprocess sandbox sont terminés (SIGTERM → 5s → SIGKILL)
  3. Une entrée d'audit OBLIGATOIRE :
     {
       "action": "budget_exceeded",
       "budget_name": "max_steps",
       "budget_value": 50,
       "current_value": 51,
       "mission_id": "uuid"
     }
  4. Message clair à l'utilisateur :
     "⚠️ Mission arrêtée : budget 'max_steps' dépassé (51/50).
      Vérifiez les résultats et relancez si nécessaire."
  5. L'agent NE PEUT PAS continuer sans action humaine

INTERDIT :
  ❌ Ignorer un budget dépassé
  ❌ Réinitialiser les compteurs automatiquement
  ❌ Modifier les budgets sans validation humaine explicite
  ❌ Augmenter un budget par l'agent lui-même
  ❌ Fractionner une mission pour contourner les budgets

CONFIGURATION :
  Les budgets sont définis dans : .ownstack/budgets.json
  L'humain peut les ajuster. L'agent ne les modifie JAMAIS.

  {
    "max_steps": 50,
    "max_exec_commands": 30,
    "max_files_modified": 20,
    "max_total_diff_lines": 2000,
    "max_duration_minutes": 30,
    "max_consecutive_failures": 3,
    "max_llm_calls": 100,
    "max_sandbox_spawns": 20
  }
```

### 6.9 Kill-Switch agent (arrêt forcé)

L'utilisateur peut **interrompre une mission agent à tout moment**, sans condition.
Le Kill-Switch est **prioritaire sur tout** : modes, budgets, exécution en cours.

```
DÉCLENCHEMENT :
  - Raccourci clavier : Ctrl+Shift+K (configurable)
  - Bouton "STOP" dans le chat panel
  - Bouton "STOP" dans la status bar
  - Commande palette : "OwnStack: Kill Agent"

AU MOMENT DE L'ARRÊT :
  1. Signal STOP envoyé à l'agent immédiatement
  2. Toute exécution sandbox en cours est terminée :
     → SIGTERM envoyé au process
     → Timeout 5 secondes
     → SIGKILL si toujours vivant
  3. Le bridge Rust ↔ Python est interrompu si nécessaire
  4. Les containers Docker sandbox sont stoppés
  5. Une entrée d'audit OBLIGATOIRE :
     {
       "action": "kill_switch",
       "reason": "user_initiated",
       "timestamp": "ISO8601",
       "mission_id": "uuid",
       "pending_commands_cancelled": 3,
       "active_processes_killed": 1
     }
  6. État final affiché à l'utilisateur :
     "🛑 Agent arrêté. X commandes annulées, Y fichiers modifiés."

RÈGLES :
  ✅ Le Kill-Switch est TOUJOURS disponible (jamais grisé/désactivé)
  ✅ Le Kill-Switch est prioritaire sur TOUS les modes
  ✅ Un agent arrêté NE PEUT PAS reprendre automatiquement
  ✅ Toute reprise nécessite une action humaine explicite
  ✅ Le Kill-Switch fonctionne même si l'audit est en erreur

INTERDIT :
  ❌ Désactiver le Kill-Switch
  ❌ Intercepter ou retarder le Kill-Switch
  ❌ Reprendre automatiquement après un Kill-Switch
  ❌ Ignorer un Kill-Switch en mode Auto-Approve
```

### 6.10 Erreurs critiques non récupérables

L'agent DOIT s'arrêter **immédiatement et sans tentative de récupération**
dans les cas suivants :

```
CONDITIONS D'ARRÊT CRITIQUE :

  🔴 Échec d'écriture de l'AuditLogger
     → Aucune action ne peut être exécutée sans audit
  🔴 Échec ou incohérence du PolicyEngine
     → Le moteur de policy ne peut pas évaluer une commande
  🔴 Échec du PathValidator non déterministe
     → canonicalize() retourne des résultats incohérents
  🔴 Crash répété du bridge Rust ↔ Python (≥ 3 tentatives)
     → Le sidecar Python ne répond plus
  🔴 Sandbox indisponible ou non initialisable
     → Impossible de créer un environnement isolé
  🔴 Corruption du fichier .ownstack/current_phase.json
     → Impossible de déterminer la phase actuelle
  🔴 Mémoire insuffisante pour opérer
     → OOM détecté ou allocation échouée

COMPORTEMENT :

  1. STOP immédiat de toute exécution
  2. Aucune tentative de récupération automatique
  3. Message d'erreur EXPLICITE à l'utilisateur :
     "🔴 ERREUR CRITIQUE : [description]. Agent arrêté.
      Action requise : [diagnostic]."
  4. Tentative d'écriture audit (si possible)
  5. Si audit impossible → journal d'urgence :
     → Écriture dans stderr
     → Écriture dans .ownstack/emergency.log (best effort)

INTERDIT :
  ❌ Continuer l'exécution sans audit valide
  ❌ Masquer ou ignorer une erreur critique
  ❌ Tenter un retry automatique sur une erreur critique
  ❌ Basculer en "mode dégradé" sans consentement humain
  ❌ Supprimer ou écraser emergency.log
```

### 6.11 Traçabilité du mode agent

Toute session agent DOIT enregistrer de manière traçable le mode actif et ses transitions.

```
DONNÉES DE TRAÇABILITÉ (enregistrées dans l'audit) :

  À chaque démarrage de session :
  {
    "action": "session_start",
    "mode": "Ask|Auto|Auto-Approve|Plan",
    "budgets": { ... },        # Copie des budgets actifs
    "phase": 1,                 # Phase actuelle du projet
    "timestamp": "ISO8601"
  }

  À chaque changement de mode :
  {
    "action": "mode_change",
    "from": "Ask",
    "to": "Auto",
    "user_initiated": true,     # TOUJOURS true (l'agent ne change pas le mode)
    "timestamp": "ISO8601"
  }

  À chaque fin de session :
  {
    "action": "session_end",
    "mode": "Auto",
    "stats": {
      "steps_executed": 12,
      "commands_run": 8,
      "files_modified": 3,
      "diff_lines": 142,
      "duration_minutes": 4.2,
      "llm_calls": 15,
      "policy_blocked": 1,
      "policy_asked": 2
    },
    "exit_reason": "completed|budget_exceeded|kill_switch|error|user_stopped",
    "timestamp": "ISO8601"
  }

AFFICHAGE UI :

  Status bar DOIT montrer en permanence :
  ┌──────────────────────────────────────────────────────┐
  │ 🤖 Mode: Auto │ Steps: 12/50 │ Cmds: 8/30 │ ⏱ 4m  │
  └──────────────────────────────────────────────────────┘

  Le chat panel DOIT afficher :
  - Le mode actif en en-tête
  - Les compteurs de budget en temps réel
  - Un indicateur visuel quand un budget approche 80%

INTERDIT :
  ❌ L'agent modifie le mode lui-même
  ❌ Session sans entry session_start dans l'audit
  ❌ Fin de session sans entry session_end
  ❌ Masquer les compteurs de budget à l'utilisateur
```

---

## 7. OUTILS ET COMMANDES AUTORISÉS

### 7.1 Rust

```bash
cargo build                      # Build debug
cargo build --release            # Build release
cargo test --workspace           # Tous les tests
cargo clippy --workspace         # Lints
cargo fmt --all -- --check       # Vérifier format
cargo fmt --all                  # Appliquer format
cargo check                      # Check rapide
```

### 7.2 Python

```bash
cd ownstack-python && pytest -v
cd ownstack-python && black app/
cd ownstack-python && isort app/
pip install -r ownstack-python/requirements.txt
```

### 7.3 Git autorisé

```bash
git status                        git diff
git add <fichiers spécifiques>    # PAS git add .
git commit -m "type(scope): msg"
git log --oneline -20             git branch
git fetch upstream
```

### 7.4 INTERDIT à l'agent

```bash
git push          git push -f         git reset --hard
rm -rf            sudo *              curl * | sh
npm publish       cargo publish       docker rm -f
```

---

## 8. PATTERNS OBLIGATOIRES

### 8.1 Tout nouveau fichier Rust dans ownstack-*

```rust
//! Description du module.
//!
//! Ce module fait partie de OwnStack Native IDE.
//! Voir docs/ARCHITECTURE.md pour le contexte.

use serde::{Serialize, Deserialize};

/// Description de la struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaStruct { /* ... */ }

impl MaStruct {
    /// Crée une nouvelle instance.
    pub fn new() -> Self { /* ... */ }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_basique() { /* OBLIGATOIRE : au moins 1 test */ }
}
```

### 8.2 Tout nouveau fichier dans lapce-app

```rust
//! Panel/Widget OwnStack: description.
//! Intégration: référencé depuis app.rs
//! Architecture: voir docs/ARCHITECTURE.md §5

// Utiliser les imports Floem existants
// Suivre les patterns des panels existants (terminal.rs)
```

### 8.3 ToolResult — Partout obligatoire

```rust
// Tout toolkit retourne un ToolResult. JAMAIS un String brut ou bool seul.
use ownstack_engine::tool_result::ToolResult;

pub fn execute_something() -> ToolResult {
    ToolResult {
        success: true,
        output: "résultat".to_string(),
        error: None,
        metadata: serde_json::json!({"duration_ms": 42, "tool": "nom"}),
    }
}
```

### 8.4 Communication RPC — Schéma obligatoire

```
GUI (lapce-app) → Proxy (lapce-proxy) → Bridge (ownstack-bridge) → Python
                                                                    ↓
GUI ← Proxy ← Bridge ← ToolResult ←────────────────────────────────┘

PAS de communication directe GUI ↔ Python.
PAS de HTTP entre composants internes.
TOUT passe par le RPC Lapce étendu.
```

---

## 9. ANTI-PATTERNS INTERDITS

### 9.1 Architecture

```
❌ Ajouter Electron, Tauri, ou framework web
❌ Remplacer Floem
❌ Dépendance VS Code ou son API
❌ Serveur HTTP interne pour comm GUI↔Backend
❌ Bypasser lapce-proxy pour parler au Python
❌ Code IA directement dans lapce-app
❌ Fusionner ownstack-engine et ownstack-agent
❌ Supprimer le plugin system WASI de Lapce
❌ Réécrire le text buffer Xi Rope
❌ Réécrire le rendu GPU wgpu
```

### 9.2 Code

```
❌ unwrap() sur données externes
❌ unsafe {} sans "// SAFETY:" commentaire
❌ println!/dbg! en production
❌ TODO sans issue
❌ Code commenté au lieu de supprimé
❌ Fichiers > 500 lignes sans découpage
❌ Clone excessif (préférer &ref)
❌ String quand &str suffit
❌ let _ = peut_echouer (ignorer erreurs)
```

### 9.3 Sécurité

```
❌ Exécution sans policy check
❌ Accès fichier sans path validation
❌ Oublier audit log
❌ shell=True (Python subprocess)
❌ Command::new avec entrées brutes
❌ Désactiver sandbox "pour tester"
❌ Secrets en clair dans code/config
❌ Agent qui change son propre mode (Ask/Auto/Auto-Approve)
❌ Agent qui réinitialise ou modifie ses budgets
❌ Agent qui intercepte ou retarde le Kill-Switch
❌ Agent qui reprend après Kill-Switch sans action humaine
❌ Continuer l'exécution quand l'audit est en erreur
❌ Retry automatique sur erreur critique
❌ Reclassifier Blocked → Ask ou Auto
❌ Session sans entry session_start / session_end dans l'audit
```

---

## 10. CHECKLIST DE VALIDATION

### Avant chaque commit

```
□ cargo check --workspace                    → 0 erreurs
□ cargo test --workspace                     → Tous passent
□ cargo clippy --workspace -- -D warnings    → 0 warnings
□ cargo fmt --all -- --check                 → Formaté
□ Pas de unwrap() ajouté hors tests
□ Pas de println! ajouté
□ Pas de secret hardcodé
□ ARCHITECTURE.md à jour si structure modifiée
□ Nouveaux fichiers ont des tests
□ Nouveau code passe par policy/audit/path safety
□ Fichiers protégés non modifiés
□ Cargo.toml workspace cohérent
□ Pas de dépendance Electron/web ajoutée
□ cargo build --release réussit
```

### Validation par phase

```
PHASE 0 COMPLÈTE QUAND :
  □ cargo build --release → binaire ownstack-ide
  □ Le binaire se lance, titre "OwnStack IDE"
  □ LICENSE, NOTICE, LICENSE-OWNSTACK existent
  □ CI passe sur 3 OS

PHASE 1 COMPLÈTE QUAND :
  □ ownstack-engine compile avec tous modules
  □ PolicyEngine bloque "rm -rf /"
  □ PathValidator rejette chemins hors workspace
  □ AuditLogger écrit JSONL valides
  □ Bridge spawn Python et reçoit réponses
  □ Palette affiche commandes IA
  □ Chat panel visible
  □ Status bar montre mode actuel

PHASE 2 COMPLÈTE QUAND :
  □ ownstack-agent compile avec providers
  □ Prompt IA → réponse streamée dans chat
  □ Agent exécute dans sandbox
  □ Healer fonctionne (fail → fix → pass)
  □ Multivers fonctionne (2 variants)
  □ Commandes Blocked effectivement bloquées
  □ Audit log complet
  □ E2E: prompt → policy → sandbox → result → audit → UI

PHASE 3 COMPLÈTE QUAND :
  □ Client MCP connecte serveur externe
  □ Serveur MCP expose tools OwnStack
  □ Multi-agent: Planner + Worker fonctionnels
  □ Plugins tiers chargés dynamiquement
```

---

## 11. PROTOCOLE DE DÉCISION

### DEMANDER à l'humain pour :

```
1. Créer un nouveau crate Cargo
2. Ajouter une dépendance externe
3. Modifier un fichier protégé (§5.1/5.2)
4. Changer la structure du monorepo
5. Supprimer du code (>20 lignes)
6. Modifier le protocole RPC
7. Modifier le PolicyEngine
8. Toute action irréversible
9. Passer à la phase suivante
10. Ajouter un framework/lib majeur
```

### NE PAS demander pour :

```
- Écrire du code dans ownstack-*
- Ajouter des tests
- Corriger des bugs
- Documenter
- Formatter
```

### Quand BLOQUÉ :

```
1. Relire docs/ARCHITECTURE.md section pertinente
2. Relire GEMINI.md section pertinente
3. Vérifier si tâche = phase actuelle
4. Si phase future → STOP, informer l'humain
5. Si contradiction ARCHITECTURE vs GEMINI → GEMINI gagne
6. Si aucune directive → DEMANDER à l'humain
7. JAMAIS improviser sur sécurité ou architecture
```

### Priorisation :

```
HAUTE   : Fixes sécurité, bugs compilation, tests échoués
MOYENNE : Fonctionnalités phase actuelle, perf, refactoring
BASSE   : Documentation, cosmétique UI, optimisations mineures
JAMAIS  : Fonctionnalités phase future, réécriture composants Lapce
```

---

## 12. GESTION DES ERREURS

### Erreur de compilation

```
1. Lire le message COMPLET
2. Si dans lapce-* → vérifier qu'on n'a pas cassé un import
3. Si dans ownstack-* → corriger normalement
4. cargo test doit passer après correction
5. JAMAIS commenter du code pour "résoudre"
```

### Test qui échoue

```
1. Identifier quel test et pourquoi
2. Si test OwnStack → corriger code OU test
3. Si test Lapce existant → revert nos changements dans Lapce
4. JAMAIS supprimer un test
5. JAMAIS #[ignore] sans justification
```

### Conflit upstream Lapce

```
1. git fetch upstream && git checkout master && git merge upstream/master
2. git checkout ownstack/main && git rebase master
3. Conflit lapce-* → garder upstream, réappliquer additivement
4. Conflit ownstack-* → garder notre version
5. Conflit Cargo.toml → garder les deux sets de members
6. TESTER : cargo test --workspace
```

---

## 13. TESTS OBLIGATOIRES

### 13.1 Tests unitaires minimum

```yaml
ownstack-engine/policy.rs:
  - test_blocked_commands
  - test_ask_commands
  - test_auto_commands
  - test_unknown_defaults_auto

ownstack-engine/path_safety.rs:
  - test_valid_workspace_path
  - test_reject_outside_workspace
  - test_reject_traversal
  - test_resolve_symlinks
  - test_new_file_in_workspace
  - test_reject_absolute_outside

ownstack-engine/audit.rs:
  - test_write_entry
  - test_read_recent
  - test_jsonl_format_valid
  - test_handles_missing_file

ownstack-engine/sandbox/process.rs:
  - test_env_cleared
  - test_timeout_kills_process
  - test_working_dir_set

ownstack-bridge/lib.rs:
  - test_json_rpc_roundtrip
  - test_handle_process_crash

ownstack-agent/toolkits/core.rs:
  - test_exec_through_policy
  - test_read_through_path_safety

ownstack-engine/budget.rs:
  - test_budget_defaults_loaded
  - test_budget_step_increment
  - test_budget_exceeded_returns_error
  - test_budget_reset_requires_flag      # Pas de reset auto
  - test_budget_consecutive_failures
  - test_budget_duration_timeout
  - test_budget_custom_from_file
  - test_budget_cannot_exceed_max        # Même si config dit 999999

ownstack-engine/kill_switch.rs:
  - test_kill_switch_stops_execution
  - test_kill_switch_kills_subprocess
  - test_kill_switch_writes_audit
  - test_kill_switch_prevents_auto_resume
  - test_kill_switch_overrides_all_modes

ownstack-engine/mode.rs:
  - test_mode_default_is_ask
  - test_mode_change_requires_user_flag
  - test_mode_auto_approve_allows_ask_commands
  - test_mode_auto_approve_blocks_blocked_commands
  - test_mode_change_writes_audit
  - test_agent_cannot_change_own_mode

ownstack-engine/critical_errors.rs:
  - test_audit_failure_stops_execution
  - test_policy_failure_stops_execution
  - test_bridge_crash_threshold
  - test_sandbox_unavailable_stops_execution
  - test_emergency_log_written_on_audit_failure
```

### 13.2 Tests E2E critiques

```yaml
  # --- Flux standard ---
  - "E2E: commande safe → policy(Auto) → sandbox → result → audit"
  - "E2E: commande blocked → policy(Blocked) → STOP → audit"
  - "E2E: commande ask + approve → exec → audit(user_approved=true)"
  - "E2E: commande ask + deny → STOP → audit(user_approved=false)"
  - "E2E: path outside workspace → PathValidator reject → audit"
  - "E2E: path traversal '../../secret' → reject"
  - "E2E: timeout 'sleep 999' → killed → ToolResult.success=false"
  - "E2E: Python crash → bridge respawn → log error"

  # --- Auto-Approve (§6.7) ---
  - "E2E: mode Auto-Approve + commande Ask → exécutée sans prompt"
  - "E2E: mode Auto-Approve + commande Blocked → TOUJOURS refusée"
  - "E2E: changement mode → audit entry mode_change écrite"
  - "E2E: agent tente de changer son propre mode → REJET"

  # --- Budgets (§6.8) ---
  - "E2E: max_steps dépassé → STOP immédiat → audit budget_exceeded"
  - "E2E: max_exec_commands dépassé → STOP → processes tués"
  - "E2E: max_consecutive_failures (3 fails) → STOP → message user"
  - "E2E: max_duration_minutes dépassé → STOP → cleanup sandbox"
  - "E2E: agent tente de réinitialiser budget → REJET"
  - "E2E: budget à 80% → warning visuel dans UI"

  # --- Kill-Switch (§6.9) ---
  - "E2E: Kill-Switch pendant exécution → process tué en <5s"
  - "E2E: Kill-Switch → audit entry kill_switch écrite"
  - "E2E: après Kill-Switch → agent ne reprend PAS automatiquement"
  - "E2E: Kill-Switch en mode Auto-Approve → agent stoppé quand même"
  - "E2E: Kill-Switch pendant Python bridge → bridge interrompu proprement"

  # --- Erreurs critiques (§6.10) ---
  - "E2E: audit write failure → agent STOP → erreur affichée"
  - "E2E: PolicyEngine failure → agent STOP → aucune commande exécutée"
  - "E2E: bridge crash ×3 → agent STOP → pas de retry"
  - "E2E: sandbox non initialisable → agent STOP → message explicite"

  # --- Traçabilité (§6.11) ---
  - "E2E: session_start entry écrite au lancement"
  - "E2E: session_end entry écrite à la fin avec stats complètes"
  - "E2E: compteurs affichés dans status bar en temps réel"
```

### 13.3 Script de validation

```bash
#!/bin/bash
set -e
echo "=== Format ===" && cargo fmt --all -- --check
echo "=== Clippy ===" && cargo clippy --workspace -- -D warnings
echo "=== Tests ===" && cargo test --workspace
echo "=== Build ===" && cargo build --release
echo "=== Security ===" 
grep -rn "unwrap()" ownstack-engine/src/ ownstack-agent/src/ | grep -v test || true
grep -rn "unsafe" ownstack-engine/src/ ownstack-agent/src/ | grep -v "SAFETY:" | grep -v test || true
echo "=== Binary ===" && test -f target/release/ownstack-ide && echo "OK"
echo "=== DONE ✅ ==="
```

---

## 14. GIT ET BRANCHES

### Structure

```
master           # Miroir lapce/lapce upstream — JAMAIS committer
ownstack/main    # Branche principale — protégée, review obligatoire
ownstack/phase-N # Branches de travail par phase
feature/xxx      # Feature branches
fix/xxx          # Fix branches
```

### Format des commits

```
type(scope): message court

Types  : feat, fix, refactor, test, docs, chore, security
Scopes : engine, agent, bridge, python, app, proxy, rpc, ci, docs

Exemples :
  feat(engine): implement PolicyEngine with blocked commands
  fix(bridge): handle Python process crash gracefully
  security(engine): prevent symlink escape in PathValidator
```

### Règles

```
OBLIGATOIRE : 1 commit = 1 chose logique
OBLIGATOIRE : cargo test passe AVANT commit
INTERDIT : "WIP", "fix fix fix"
INTERDIT : Committer .pyc, __pycache__, target/
INTERDIT : Committer secrets, .env, API keys
INTERDIT : Force push sur ownstack/main
```

---

## 15. GARDE-FOUS AUTOMATIQUES

### 15.1 Pre-commit hooks

```yaml
hooks:
  - cargo-fmt: "cargo fmt --all -- --check"
  - cargo-clippy: "cargo clippy --workspace -- -D warnings"
  - cargo-test: "cargo test --workspace"
  - no-secrets: "grep -rn 'sk-\\|api_key.*=.*[A-Za-z0-9]' --include='*.rs' --include='*.py'"
  - no-unwrap-prod: "grep unwrap() dans ownstack-*/src/ hors tests"
  - protected-files: "Rejeter si LICENSE, buffer.rs, syntax.rs modifiés"
```

### 15.2 CI GitHub Actions

```yaml
jobs:
  - check: [format, clippy, test, build] × [linux, macos, windows]
  - security-audit: [no secrets, no unsafe sans SAFETY, protected files]
  - python-checks: [pytest] si ownstack-python/ modifié
```

### 15.3 Fichier de suivi de phase

```json
// .ownstack/current_phase.json
// L'agent LIT ce fichier, seul l'humain le MODIFIE
{
  "current_phase": 0,
  "phase_0_complete": false,
  "phase_1_complete": false,
  "phase_2_complete": false,
  "phase_3_complete": false,
  "phase_4_complete": false,
  "last_updated": "2026-02-09",
  "notes": "Démarrage du projet"
}
```

### 15.4 Fichier de budgets agent

```json
// .ownstack/budgets.json
// L'agent LIT ce fichier, seul l'humain le MODIFIE
{
  "max_steps": 50,
  "max_exec_commands": 30,
  "max_files_modified": 20,
  "max_total_diff_lines": 2000,
  "max_duration_minutes": 30,
  "max_consecutive_failures": 3,
  "max_llm_calls": 100,
  "max_sandbox_spawns": 20
}
```

```
RÈGLE : Si .ownstack/budgets.json n'existe pas :
  → Utiliser les valeurs par défaut ci-dessus
  → NE PAS créer le fichier automatiquement

RÈGLE : Si un budget est à 0 ou négatif :
  → Considérer comme "illimité" SAUF pour max_consecutive_failures
  → max_consecutive_failures minimum = 1 (toujours actif)
```

### 15.5 Vérification avant chaque action

```
⚠️ L'AGENT VÉRIFIE SYSTÉMATIQUEMENT :

  1. "Suis-je dans la bonne phase ?"
     → Lire .ownstack/current_phase.json
  2. "Ce fichier est-il protégé ?"
     → Vérifier §5.1 et §5.2
  3. "Cette action nécessite confirmation ?"
     → Vérifier §11
  4. "Mon code passe les checks ?"
     → cargo check avant commit
  5. "Le flux sécurité est respecté ?"
     → Policy → PathSafety → Sandbox → ToolResult → Audit
  6. "Suis-je dans les budgets ?"
     → Vérifier tous les compteurs vs .ownstack/budgets.json
     → Si un budget est à ≥80% → log warning
     → Si un budget est dépassé → STOP immédiat
  7. "Le Kill-Switch est-il fonctionnel ?"
     → Le handler Kill-Switch DOIT être enregistré et actif
     → Si Kill-Switch non disponible → NE PAS démarrer la mission
  8. "L'audit est-il opérationnel ?"
     → Tenter une écriture test au démarrage de session
     → Si échec → NE PAS démarrer (erreur critique §6.10)
  9. "Quel est mon mode actif ?"
     → Lire le mode depuis la config session
     → Logger session_start avec le mode
```

---

## RÉSUMÉ DES INVARIANTS

```
┌─────────────────────────────────────────────────────────────────┐
│                    INVARIANTS NON-NÉGOCIABLES                    │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  1. Rust-first. Pas d'Electron. Pas de VS Code.                │
│  2. Toute exécution → Policy → PathSafety → Sandbox → Audit.   │
│  3. Flux RPC : GUI → Proxy → Bridge → Python (ou Rust).        │
│  4. Phases séquentielles. Pas de saut.                          │
│  5. Fichiers protégés = intouchables.                           │
│  6. Tests passent AVANT chaque commit.                          │
│  7. Pas de secrets dans le code.                                │
│  8. ARCHITECTURE.md = source de vérité structurelle.            │
│  9. GEMINI.md = source de vérité comportementale.               │
│ 10. En cas de doute → STOP → demander à l'humain.              │
│ 11. Blocked = TOUJOURS refusé, quel que soit le mode.           │
│ 12. Budgets agent = limites dures, jamais contournables.        │
│ 13. Kill-Switch = prioritaire sur tout, toujours disponible.    │
│ 14. Pas d'audit = pas d'exécution. Jamais.                      │
│ 15. L'agent ne change JAMAIS son propre mode.                   │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

*Dernière mise à jour : 9 février 2026*
*Compatible avec : docs/ARCHITECTURE.md v1.1*
*Ce fichier est versionné et fait partie du repository.*
