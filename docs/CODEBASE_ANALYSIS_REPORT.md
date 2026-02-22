# Rapport d'analyse : OwnStack Native IDE

## 1. Vision Macro (Phase 1)

### 1.1 Identité du projet
- **Nom** : OwnStack Native IDE
- **Base** : Fork de Lapce (Rust natif, GPU-accelerated)
- **Langage principal** : Rust (Edition 2024 / resolver 3)
- **Langage secondaire** : Python (Sidecar backend legacy)
- **UI Framework** : Floem (Rust natif)

### 1.2 Structure du Monorepo
Le projet est organisé en un workspace Rust de **7 membres** :

| Crate | Rôle |
|-------|------|
| `lapce-app` | Interface utilisateur (GUI), gestion des fenêtres et panels. |
| `lapce-core` | Cœur de l'éditeur (Xi-rope, syntaxe, buffer). |
| `lapce-proxy` | Processus proxy pour LSP, accès fichiers et bridge OwnStack. |
| `lapce-rpc` | Protocole de communication entre GUI et Proxy. |
| `ownstack-engine` | **Noyau sécurité** : PolicyEngine, AuditLogger, Sandbox. |
| `ownstack-agent` | **Agent IA** : Orchestration, Providers LLM, Toolkits. |
| `ownstack-bridge` | Bridge de communication Rust ↔ Python. |

### 1.3 Métriques techniques (estimations)
- **Fichiers source** : ~1,200+ fichiers `.rs`
- **LOC (Rust)** : ~150,000+ (incluant le core Lapce)
- **Points d'entrée** : 
  - GUI : `lapce-app/src/main.rs` (binaire: `ownstack-ide`)
  - Proxy : `lapce-proxy/src/main.rs`
- **Dépendances critiques** : `floem` (UI), `tokio` (Async), `wgpu` (GPU), `candle-core` (ML), `lsp-types`.

---

## 2. Architecture & Flux (Phase 2 & 4)

### 2.1 Flux de Sécurité (Mandatoire)
Toute action IA suit le chainage suivant défini dans `GEMINI.md` :
1. `PolicyEngine::evaluate` → [Blocked | Ask | Auto]
2. `PathValidator::validate` → Validation chemins workspace
3. `ProcessSandbox::exec` → Exécution isolée
4. `AuditLogger::log` → Journalisation JSONL immuable

### 2.2 Intégration de l'Agent
L'agent (`ownstack-agent`) est orchestré par `AgentOrchestrator` qui gère :
- **Planning** : Découpage des tâches utilisateur en missions.
- **Execution** : Appel des toolkits (Healer, Multivers, LSP, Git).
- **Namespacing** : Support du format `toolkit:tool` pour éviter les collisions.

---

## 3. État de Phase (Phase 3 & 5)

### 3.1 Validation de l'existence
Tous les crates déclarés dans le workspace existent et sont branchés. 
Le fichier `.ownstack/current_phase.json` confirme que le projet est en **Phase 12 (Team Orchestration)**.

### 3.2 Vérification Compilation
- `cargo check --workspace` : **PASS** (Zero warnings critiques).
- `python scripts/healthcheck.py` : **PASS** (10/10 gates validées).

---

## 4. Synthèse & Prochaines Étapes (Phase 6)

### Points Forts
- **Performance** : Rendu GPU natif sans overhead Electron.
- **Sécurité** : Audit log concurrent-safe et policy engine shell-aware.
- **Intelligence** : Indexation sémantique persistante et auto-healing robuste.

### Risques identifiés
- **Dette Technique** : La dépendance `ownstack-python` est en cours de dépréciation au profit du code Rust natif.
- **Modularité** : L'extension des toolkits nécessite une recompilation (Phases 3/12 prévoient des plugins WASI signés).

---
*Rapport généré par OwnStack Agent selon le workflow `codebase-explorer`.*
