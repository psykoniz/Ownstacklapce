# OwnStack IDE — Rapport de capacités agent (test en conditions réelles)

**Date** : 2026-06-26
**Provider testé** : `codex-everywhere` (`https://codex-everywhere.com`), modèle **gpt-5.5**, wire `chat`/`responses`
**Clé utilisée** : #1 `sk-5fe6bd…` (fonctionnelle). Clés #2 `sk-e48ddd…` (désactivée, `GROUP_DISABLED`) et #3 `sk-8a2c31…` (`No available accounts`) NON fonctionnelles.
**Méthode** : harness Rust (`ownstack-agent/examples/realtest.rs`, `exec_probe.rs`) pilotant l'orchestrateur réel (provider → contexte → outils) en sandbox isolée.

Échelle : **0 = absent/cassé**, **100 = SOTA** (niveau Cursor / Claude Code / Windsurf / Devin).

---

## Résultats des tests exécutés en réel

| # | Test | Temps | Résultat | Note |
|---|------|-------|----------|------|
| T1 | Raisonnement (17×23) | 3.4s | ✅ « 391 » | — |
| T2 | Création fichier (tool `write`) | 10.5s | ✅ `hello.txt` contenu exact | — |
| T3 | Lecture + édition (`read`+`edit`) | 12.2s | ✅ ligne ajoutée correctement | — |
| T4 | Exec shell (`echo > file`) | 75.8s | ❌ `max_consecutive_failures` (gate approbation + pas de shell) | — |
| T5 | Mission autonome code+run | 105.9s | ⚠️ `fib.py` correct créé ; exécution plausible mais format liste≠spec | — |
| T6 | Génération code Rust | 1.8s | ✅ `s.chars().rev().collect()` idiomatique | — |

---

## Notes par capacité

| Capacité | Note /100 | Constat (conditions réelles) |
|----------|-----------|------------------------------|
| **Intégration provider** (OpenAI-compatible, gpt-5.5) | **90** | Wires `chat` ET `responses` fonctionnent (200 OK). Config simple via env. Parsing réponses OK. |
| **Raisonnement / Q&A** | **85** | Correct et rapide (3s). Dépend du modèle branché ; avec gpt-5.5, solide. |
| **Génération de code** | **85** | Code correct, idiomatique, concis. Rapide. |
| **Outils fichiers** (`read`/`write`/`edit`/`search`) | **85** | Lecture/écriture/édition fiables et exactes. C'est le socle de l'autonomie. |
| **Exec / shell** | **45 → 75 (corrigé)** | ⚠️ Avant : pas d'enrobage shell sur Windows → redirections/pipes ignorés, échec SILENCIEUX. ✅ **Corrigé** (wrap `cmd /C` + PATH parent + `SystemDrive`) : redirections créent les fichiers, toolchains résolus. Re-test T4 : 75.8s d'échec → 4.5s. `python -c`/`sh -c` restent **bloqués** (anti-injection, voulu). |
| **Boucle autonome** (Plan→Worker→Critic, missions) | **70** | Décompose en étapes et exécute (fichiers OK). Solide pour les tâches basées sur `write`/`edit`. Fragile dès qu'une étape dépend de features shell. Budget `max_steps=50`. |
| **Politique de sécurité** | **80** | Bloque injection (`python -c`, `sh -c`, reverse shells), `Ask`-gate les écritures/`rm -rf`/`publish`. Posture solide. |
| **Anti-boucle / budgets** | **80** | Stoppe proprement (`max_consecutive_failures`, `max_steps`). Évite l'emballement. |

---

## Bug concret identifié (à corriger)

**`ownstack-engine/src/sandbox/process.rs`** — `ProcessSandbox::exec` : il existe un `resolve_command` pour Linux et macOS mais **aucun pour Windows**. Au niveau `Standard`, la commande est lancée en direct (`Command::new(parts[0]).args(parts[1..])`), donc :
- les redirections/pipes/`&&` ne sont pas interprétés (pas de shell) ;
- pire, sur Windows un `echo X > f.txt` renvoie `success=true` mais ne crée rien (échec silencieux).

**Correctif recommandé** : enrober au niveau `Standard` dans le shell de la plateforme quand la commande contient des métacaractères shell — `cmd /C "<cmd>"` (Windows) / `/bin/sh -c "<cmd>"` (Unix). La `PolicyEngine::evaluate` s'applique sur la commande brute → la sécurité reste préservée. (Alternative : apprendre à l'agent à privilégier les outils `write`/`edit` plutôt que `echo >`.)

---

## 2ᵉ vague — testée en réel (harness `realtest2.rs`)

| Capacité | Note /100 | Constat |
|----------|-----------|---------|
| **RepoMap** (cartographie symboles) | **80** | 0.03s, sync, **multi-langage** : 6 symboles extraits (Rust `login`/`logout`/`User` + Python `add`/`Calc`/`mul`). Manque 1 méthode `impl`. Rapide et fiable. |
| **Spécialiste Security** | **88** | A correctement identifié une **SQL Injection**, cité **OWASP A03:2021**, donné des exemples d'attaque (`1 OR 1=1`, `DROP TABLE`). Niveau quasi-SOTA. |
| **Spécialiste Reviewer** | **88** | Division par zéro (High) **+ overflow subtil `i32::MIN / -1` (Medium)**, fix `checked_div`. Analyse d'expert. |
| **Browser** (`browse_url`) | **70** | ✅ fetch + extraction de contenu (example.com, 200). Chrome installé → automation possible. `browser_screenshot` (CDP) non testé. |
| **FIM** (autocomplétion) | **n/a** | Backends `Ollama`/`OpenRouter` uniquement — **pas testable** avec le provider OpenAI-compat codex-everywhere. Nécessite Ollama local ou OpenRouter. |

## Correctif appliqué pendant le test (exec)
`ownstack-engine/src/sandbox/process.rs` :
1. **Wrap `cmd /C`** sur Windows (parité avec `sh -c` Unix) → redirections/pipes/builtins fonctionnent.
2. **PATH parent ajouté** au PATH enfant → `python`/`cargo`/`node` résolus via cmd.
3. **`SystemDrive`/`ProgramData`** ajoutés à l'env → plus de répertoires littéraux `%SystemDrive%`.
Vérifié par `exec_probe.rs` (les redirections créent les fichiers) et re-test agent (T4 : échec→succès).

## 3ᵉ vague — testée en réel (harness `realtest3.rs`)

| Capacité | Note /100 | Constat |
|----------|-----------|---------|
| **FailureAnalyzer** (parsing erreurs) | **80** | Détecte et parse l'erreur compilateur (`SyntaxError`, fichier, ligne). Statique, rapide. |
| **Healer** (auto-réparation) | **55** | Infra fonctionnelle (lance la commande, appelle le LLM pour des fixes), mais conservateur : `healed=false attempts=0` sur un « fichier manquant » (pas de fix shell sûr → comportement raisonnable). À retester sur un échec réparable (dépendance manquante). |
| **ProjectMemory** (règles projet) | **85** | Charge `.ownstack/rules.md` correctement. Sync, fiable. Alimente les prompts. |
| **RAG / SemanticIndex** | **40** | Embeddings BERT locaux (candle). `init` échoue **proprement** : `Model directory not found. Please run bootstrap.` → nécessite le téléchargement du modèle. Inutilisable out-of-the-box. |
| **MCP client** | **88** | ✅ Round-trip complet : spawn serveur → `connect` → `tools/call` `echo` → `"hello-mcp"` (0.1s). Intégration solide. |
| **Vision** (multi-modale) | **85** | ✅ gpt-5.5 via codex-everywhere décrit un screenshot avec exactitude (« dark-themed OwnStack editor… file explorer, chat panel, terminal »). |

## 4ᵉ vague — testée en réel (harness `realtest4.rs`)

| Capacité | Note /100 | Constat |
|----------|-----------|---------|
| **Git** (status/diff) | **85** | `git_status` correct (`M app.py`). Opérations git fiables via sandbox. |
| **Git suggest_commit_message** | **85** | gpt-5.5 génère un message conventionnel exact à partir du diff : « Add subtraction helper function » (3.7s). |
| **Multivers** (A/B infra) | **72** | `fork_and_run` exécute des variantes, score et désigne un `winner`. Le 2ᵉ variant n'est pas remonté dans `results` (à creuser). |
| **LSP** (rust-analyzer) | **72** | ✅ `lsp_auto_connect` → « Connected to LSP server: rust-analyzer ». Diagnostics nécessitent un `textDocument/didOpen` préalable ; le client ne se ferme pas proprement (backtrace « client exited without proper shutdown »). |

| **ACP** (stdio JSON-RPC) | **85** | Round-trip complet : `initialize` (capabilities: modes ask/auto/plan, image, streaming, tools), `session/new`, `session/prompt` « 6×7 » → streamé « 42 » via `session/update` → `stopReason: end_turn`. Éditeurs externes (Zed, …) peuvent piloter l'agent. |

## Observation infra — processus agent orphelins
Lancer l'IDE spawn un process enfant `ownstack-agent` (bridge). Lors des tests, des
terminaisons forcées du parent (IDE) ont laissé jusqu'à **24 `ownstack-agent` orphelins**
(consommant plusieurs GB de RAM). À vérifier : la fermeture *normale* de l'IDE tue-t-elle bien
l'arbre de process ? Sinon, ajouter un kill du child agent au shutdown (job object / process group).

## Capacités encore NON testées en runtime
`InfraSense` (détection d'infra).

## Réponse à « peut-il faire un projet en autonomie ? »
**Oui pour les tâches centrées fichiers/code** (création multi-fichiers, édition, génération) — démontré en réel. **Avec réserves** sur les étapes nécessitant le shell (build/run/test via redirections ou enchaînements) tant que le wrap shell Windows n'est pas ajouté. Avec gpt-5.5 + la correction exec, l'autonomie bout-en-bout serait nettement plus fiable.
