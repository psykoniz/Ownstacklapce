# Journal de Test Utilisateur v2 — Audit d'accessibilite de toutes les fonctionnalites OwnStack

**Testeur** : Dev fullstack senior, session de stress-test complet.
**Date** : 7 mars 2026
**Objectif** : Verifier que chaque fonctionnalite OwnStack est accessible, decouvrable et fonctionnelle depuis l'UI.
**Methode** : Parcours systematique de chaque feature, tentative d'y acceder sans lire la doc, notation de l'accessibilite.

---

## Inventaire complet des fonctionnalites OwnStack

Avant de tester, voici la liste exhaustive construite a partir du code source :

| # | Feature | Module source | Expose dans l'UI ? |
|---|---------|---------------|---------------------|
| 1 | Onboarding (provider + mode) | `ownstack_onboarding.rs` | A tester |
| 2 | AI Chat Panel (Ask/Auto/Plan) | `ownstack_chat.rs` | A tester |
| 3 | Mission System (plan + steps) | `mission/` | A tester |
| 4 | OwnStack Palette (AI commands) | `ownstack_palette.rs` | A tester |
| 5 | Audit Log Viewer | `ownstack_audit.rs` | A tester |
| 6 | MCP Server Manager | `ownstack_mcp.rs` | A tester |
| 7 | Status Bar (mode/budget/bridge) | `ownstack_status.rs` | A tester |
| 8 | Empty States (editor/chat/MCP/audit) | `ownstack_empty_state.rs` | A tester |
| 9 | Browser Toolkit (browse_url) | `toolkits/browser.rs` | A tester |
| 10 | Time Machine (snapshots/restore) | `toolkits/time_machine.rs` | A tester |
| 11 | Vision (analyze_image/capture_ui) | `toolkits/vision.rs` | A tester |
| 12 | Multivers (A/B fork & run) | `toolkits/multivers.rs` | A tester |
| 13 | Healer (self-healing agent) | `toolkits/healer.rs` | A tester |
| 14 | Specialists (QA/Security/Designer/Docs/PM/Reviewer) | `toolkits/specialists/` | A tester |
| 15 | RepoMap (AST codebase graph) | `repomap.rs` | A tester |
| 16 | Project Memory (rules.md) | `project_memory.rs` | A tester |
| 17 | Model Router (routing.json) | `routing.rs` | A tester |
| 18 | InfraSense (system metrics) | `infra_sense.rs` | A tester |
| 19 | Context Manager (token budget) | `context.rs` | A tester |
| 20 | Artifact Manager | `artifact_manager.rs` | A tester |
| 21 | Policy Engine (command security) | `engine/policy.rs` | A tester |
| 22 | Sandbox (process isolation) | `engine/sandbox/` | A tester |
| 23 | Resilience (retry + backoff) | `resilience.rs` | A tester |
| 24 | Signed Toolkit verification | `toolkits/mod.rs` | A tester |
| 25 | UI Snapshot (Vision Bridge) | `window_tab.rs:take_ui_snapshot` | A tester |

---

## TEST 1 — Onboarding

**Chemin d'acces** : Automatique au premier lancement.

### Actions
L'onboarding s'affiche seul. 5 etapes : Welcome → Provider Setup → Mode Selection → Workspace Config → Ready.

### Observations
- Chaque etape a un titre gras 18px, un badge "Step N/5" en pill bleue, et 5 dots de progression.
- Le choix de provider (OpenRouter / Anthropic / Ollama) est clair. Les cles API sont stockees via le keyring natif (`KEYRING_SERVICE = "OwnStack IDE"`).
- Le choix de mode (Ask/Auto/Plan) est propose avec descriptions.
- **Skip est present** (l.401) — il appelle `finish()` qui persiste l'etat.
- Navigation : boutons Skip / Next / Finish. Esc ne ferme pas l'overlay.

### Verdict accessibilite : ✅ Accessible
### Probleme
- **P1** : Skip persiste `OpenRouter` comme provider par defaut *sans cle API*. L'IDE se retrouve configure vers un provider injoignable. Il manque un mode "None" explicite.

---

## TEST 2 — AI Chat Panel

**Chemin d'acces** : Panneau droit, icone bulle de commentaire (`PanelKind::OwnStackChat`, position `RightTop`).

### Actions
Je clique sur l'icone Chat dans la sidebar droite. Le panneau s'ouvre.

### Observations
- Header : "OwnStack AI Chat" en gras 13px, badge mode cliquable (💬 Ask / ⚡ Auto / 🗺 Plan) avec couleur codee (bleu/ambre/violet). Clic = cycle entre les modes. Un bouton poubelle efface l'historique.
- Zone messages : scroll avec `ChatMessage { role, content, sub_role, diff_target }`. Les messages ont un role (User/Assistant/System/Tool/Alert) et un `sub_role` optionnel ("Worker", "Critic", "Healer").
- Zone de saisie : input avec placeholder "Type a message to OwnStack...", bord arrondi, glow au focus. Enter = envoyer. Bouton Send/Stop qui bascule selon `is_loading`.
- **Bouton attach** pour envoyer le contexte UI via `UiSnapshotRequest`.
- **Mission Display** : quand l'agent est en mission, un encart s'affiche avec "Mission: {goal}" et la liste des etapes avec leur statut.
- **Monitor tabs** : onglets Output / Problems en bas du chat pour voir le flux agent brut.
- **Empty state** : si aucun message, un placeholder elegant "Start a conversation" avec icone AI branded et description.
- **Context bar** : affichage `context_current / context_max` tokens utilises.
- **Diff view** : les patches proposes par l'agent sont affiches inline avec syntaxe coloree (vert/rouge).

### Verdict accessibilite : ✅ Accessible et riche
### Probleme
- **P2** : Le chat ne verifie pas `bridge_connected` avant d'envoyer. Aucun feedback si le bridge est down. (Confirme du premier rapport)
- **P3** : Les onglets Output/Problems sont petits et peu visibles. Un utilisateur peut ne jamais les remarquer.

---

## TEST 3 — Mission System

**Chemin d'acces** : Indirect — via l'agent quand il recoit un `MissionUpdate { goal, steps }`.

### Actions
Je demande dans le chat : "Refactor the authentication module to use JWT". L'agent passe en mode Plan.

### Observations
- L'agent cree une `MissionSpec` avec status, mode (`StaticRead`/`SafeTooling`/`DynamicExec`/`Hypothetical`), strategy (`EphemeralBranch`/`PatchLog`/`DryRun`), permissions, et checkpoints.
- Le `MissionCompiler` transforme le prompt en spec structuree via le LLM.
- Le `OpenClawOrchestrator` genere un plan multi-etapes (planning → execution → verification).
- Dans le chat, la mission s'affiche avec son goal et ses etapes.
- La status bar affiche "mission" comme status text.

### Verdict accessibilite : ⚠️ Partiellement accessible
### Probleme
- **P4** : Il n'y a **aucun panneau dedie aux missions**. Pas de `PanelKind::OwnStackMission`. La mission n'est visible que dans le chat, melangee aux messages. On ne peut pas voir la liste des missions passees, leur statut, ou naviguer entre elles. C'est un systeme back-end puissant (`MissionManager` avec persistence atomique, pub/sub) mais invisible dans l'UI.
- **P5** : Pas de moyen de creer une mission manuellement depuis la palette ou un menu. Il faut passer par le chat.

---

## TEST 4 — OwnStack Palette (AI Command Palette)

**Chemin d'acces** : Via un raccourci clavier (non visible par defaut), ou via la status bar.

### Actions
J'essaie `Ctrl+Shift+P` — c'est la palette Lapce standard. Pas l'OwnStack Palette.
J'essaie de trouver l'OwnStack Palette...

### Observations
- La palette existe en tant que `OwnStackPaletteData` avec un overlay full-screen, un input "Ask AI anything...", et 3 actions suggerees :
  1. "Analyze Active File"
  2. "Simulate Policy: npm publish"
  3. "Open Settings"
- Le design est soigne : fond semi-transparent, bordures bleues, glow, raccourcis Esc/Enter affiches.
- Tip en bas : "try '/plan' to switch agent to planning mode".
- Le filtrage par mots-cles fonctionne sur les actions suggerees.
- Quand rien ne match : "No matching actions — press Enter to send as prompt".

### Verdict accessibilite : ⚠️ Difficile a decouvrir
### Probleme
- **P6** : Pas de raccourci clavier visible ou documente pour ouvrir l'OwnStack Palette. Pas de bouton dans la toolbar, pas de mention dans le menu contextuel. C'est un composant fantome : parfaitement code, invisible a l'utilisateur.
- **P7** : Seulement 3 actions suggerees. Aucune action pour : Time Machine, Multivers, Browser, Healer, Vision, Specialists. Ces features back-end n'ont aucun point d'entree UI.

---

## TEST 5 — Audit Log Viewer

**Chemin d'acces** : Via la status bar (badge OwnStack) ou via `ownstack_audit`.

### Actions
Je trouve le lien audit dans la status bar et clique.

### Observations
- Le panneau affiche : timestamp, session ID, action, commande, decision de politique (Auto/Ask/Blocked), succes, nom de l'outil, duree en ms, chemins accedes.
- Filtres : All / SecurityOnly / FailuresOnly. Champ de recherche textuel.
- **Empty state** : icone bouclier vert, "No audit entries yet", description "All AI actions and tool calls will be recorded here."

### Verdict accessibilite : ✅ Accessible
### Probleme
- **P8** : Le panneau n'est pas dans la liste `PanelKind` — il n'a pas d'icone dans la sidebar. Il est accessible uniquement via la status bar. Un utilisateur pourrait ne jamais le trouver.

---

## TEST 6 — MCP Server Manager

**Chemin d'acces** : Panneau gauche bas (`PanelKind::OwnStackMcp`, position `LeftBottom`).

### Actions
Je clique sur l'icone MCP (icone extensions) dans la partie basse du panneau gauche.

### Observations
- Liste des serveurs MCP avec : nom, commande, args, statut (Available vert / Command not found rouge / Unknown jaune), source de config.
- Bouton "Add MCP Server" avec formulaire input.
- Le panneau lit les configs depuis `mcp-servers.json` dans workspace ou globalement.
- **Empty state** : "No MCP servers configured" avec bouton CTA "Add MCP Server" et indication des chemins recherches.

### Verdict accessibilite : ✅ Accessible
### Probleme mineur
- L'icone est la meme que Plugins (extensions). Confusion possible.

---

## TEST 7 — Status Bar OwnStack

**Chemin d'acces** : Toujours visible en bas de l'ecran.

### Actions
Je regarde la barre de statut.

### Observations
- A gauche : mode Vim, branche git, erreurs/warnings.
- **Elements OwnStack** :
  - Badge mode agent (Ask/Auto/Plan)
  - Etat bridge connected/disconnected
  - Nombre d'operations en attente (`pending_ops`)
  - Budget : tokens (current/max), steps (current/max), calls (current/max)
  - Bouton "Take UI Snapshot (Vision Bridge)"
- Le bouton snapshot est fonctionnel : il appelle `take_ui_snapshot()` qui ecrit `.ownstack/ui_snapshot.json` avec l'etat complet de l'UI.

### Verdict accessibilite : ✅ Accessible et informatif

---

## TEST 8 — Empty States

**Chemin d'acces** : Automatique quand les panneaux sont vides.

### Observations
4 empty states implementes :
1. **Editor** : diamant OwnStack, "AI-native code editor", bouton "Open Folder", hint "Ctrl+O"
2. **Chat** : icone AI avec glow, "Start a conversation", description
3. **MCP** : icone cercle, "No MCP servers configured", bouton CTA
4. **Audit** : bouclier, "No audit entries yet", description

Design coherent avec tokens partages : `TITLE_COLOR`, `DESC_COLOR`, `HINT_COLOR`, `BRAND_ACCENT`, `CTA_BG/BORDER/TEXT`.

### Verdict accessibilite : ✅ Excellent — pas d'ecran noir

---

## TEST 9 — Browser Toolkit

**Chemin d'acces** : ???

### Actions
Je cherche partout dans l'UI. Palette OwnStack : rien. Palette Lapce : rien. Menu : rien. Chat : je tape "browse https://example.com"...

### Observations
- Le `BrowserToolkit` est un toolkit agent-side. Il n'a aucun UI.
- Il expose un seul outil `browse_url` avec actions : navigate/click/type/screenshot.
- **Mais** l'implementation est un stub : elle retourne toujours `"Navigated to {url}. Page loaded successfully. Note: For full browser automation, use the Secure Browser toolkit."` sans reellement naviguer.
- L'utilisateur ne peut pas lancer de navigation web depuis l'IDE.

### Verdict accessibilite : ❌ Inaccessible directement
### Problemes
- **P9** : Pas de panneau "Browser" dans l'IDE. Pas de webview integree.
- **P10** : L'implementation est un stub. Le toolkit pretend naviguer mais ne fait rien. C'est trompeur pour l'agent qui croit avoir charge la page.

---

## TEST 10 — Time Machine (Git Snapshots)

**Chemin d'acces** : ???

### Actions
Je cherche "snapshot" ou "time" dans la palette... Rien. Status bar ? Non. Menu ? Non.

### Observations
- Le `TimeMachineToolkit` est un systeme complet et fonctionnel :
  - `create_snapshot` : cree un commit git avec prefix `[OwnStack Snapshot]`
  - `restore_snapshot` : checkout vers un commit (avec auto-snapshot de securite avant)
  - `list_snapshots` : liste les snapshots via `git log --grep`
  - `current_diff` : affiche le diff courant
- Les tests sont solides (init_test_repo, create, list, diff).
- **Mais il n'y a aucune UI** pour cette feature. Pas de panneau, pas de timeline visuelle, pas de bouton "Restore".

### Verdict accessibilite : ❌ Inaccessible directement
### Problemes
- **P11** : Feature la plus demandee par les utilisateurs d'IDE AI (undo agent), mais completement invisible. Pas de `PanelKind::TimeMachine`, pas de vue timeline, pas d'entree dans la palette OwnStack.
- Le seul moyen d'y acceder est via le chat en mode Auto — l'agent peut appeler `create_snapshot`/`restore_snapshot` en interne. L'utilisateur ne peut pas le faire manuellement.

---

## TEST 11 — Vision (Image Analysis + UI Capture)

**Chemin d'acces** : Partiellement via la status bar (bouton UI Snapshot).

### Actions
Je clique sur "Take UI Snapshot (Vision Bridge)" dans la status bar.

### Observations
- Le bouton fonctionne : il appelle `take_ui_snapshot()` qui serialise l'etat complet de l'IDE en JSON dans `.ownstack/ui_snapshot.json` (fichiers ouverts, onglets, panels, editeur actif, curseur, erreurs, git status...).
- Il tente aussi un screenshot via `ownstack_engine::vision::capture_active_window()`.
- Cote agent, le `VisionToolkit` expose :
  - `analyze_image` : charge une image, la convertit en base64, la passe au LLM multimodal
  - `capture_ui` : lit le snapshot JSON + screenshot
- Path validation + audit logging pour chaque operation.

### Verdict accessibilite : ⚠️ Partiellement accessible
### Problemes
- **P12** : Le bouton "Take UI Snapshot" est dans la status bar mais son role est cryptique. L'utilisateur ne comprend pas pourquoi il capturerait l'UI. Pas de feedback visuel apres le clic (pas de toast "Snapshot saved").
- `analyze_image` n'a aucun point d'entree UI. Pas de drag-and-drop d'image dans le chat, pas de bouton "Attach image".

---

## TEST 12 — Multivers (A/B Testing)

**Chemin d'acces** : ???

### Actions
Recherche dans toute l'UI. Rien.

### Observations
- Le `MultiversToolkit` est un systeme d'A/B testing impressionnant :
  - `multivers_run` : execute une commande avec N variantes en parallele (env vars, setup commands differents)
  - Scoring multi-objectifs : exit code (50pts), performance (20pts), warnings (20pts), clean stderr (10pts)
  - Early stop a 95 points (configurable via `OWNSTACK_MULTIVERS_EARLY_STOP_SCORE`)
  - Parallelisme configurable via `OWNSTACK_MULTIVERS_MAX_PARALLEL` (default 4)
  - Semaphore pour limiter la concurrence
  - Tie-break deterministe (ordre alphabetique)
  - Chaque variante s'execute dans le `ProcessSandbox` (sandbox.exec + SandboxLevel::Standard)
- Tests solides : deterministic tie-break, empty variants.

### Verdict accessibilite : ❌ Inaccessible directement
### Probleme
- **P13** : Zero UI. Pas de panneau "Multivers", pas de vue comparative des resultats, pas de bouton "Run A/B test". Feature completement back-end.

---

## TEST 13 — Healer (Self-Healing Agent)

**Chemin d'acces** : ???

### Actions
Je lance `cargo test` dans le terminal, ca echoue. Rien ne se passe automatiquement.

### Observations
- Le `HealerToolkit` est le plus gros toolkit (1043 lignes) :
  - `FailureAnalyzer` detecte les erreurs **Python** (ImportError, SyntaxError, TypeError), **Rust** (E0308, E0432, E0277, missing crate), **JavaScript/TypeScript** (ReferenceError, TS compilation, MODULE_NOT_FOUND), **Go** (undefined, missing package), et les test failures multi-langage.
  - `HealerToolkit.heal()` boucle : run → analyze → suggest fix (LLM ou heuristique) → apply fix in sandbox → re-run → verify.
  - Chaque fix passe par `PolicyEngine::evaluate()` avant application.
  - Max attempts configurable (default 5).
  - Extract helpers pour chaque langage (file path, line number, error code, crate name).

### Verdict accessibilite : ❌ Inaccessible directement
### Probleme
- **P14** : Pas de bouton "Auto-fix" dans le panneau Problems. Pas d'integration avec le terminal quand une commande echoue. L'utilisateur doit demander explicitement "heal" dans le chat. Un bouton "Try Auto-fix" a cote de chaque erreur serait transformatif.

---

## TEST 14 — Specialists (QA / Security / Designer / Docs / PM / Reviewer)

**Chemin d'acces** : ???

### Actions
Je cherche dans la palette, le menu, les panneaux.

### Observations
6 toolkits specialises :
- **QA** : `analyze_test_failure` (diagnostic des echecs de tests), `list_test_files` (scan recursif multi-langage)
- **Security** : audit de securite du code
- **Designer** : aide UI/UX
- **Docs** : generation de documentation
- **PM** : gestion de projet
- **Reviewer** : revue de code

### Verdict accessibilite : ❌ Inaccessible directement
### Probleme
- **P15** : Aucun de ces 6 specialistes n'a de point d'entree UI. Pas de menu "Request Code Review", pas de bouton "Generate Docs", pas d'action dans la palette. L'utilisateur doit deviner qu'ils existent et les invoquer manuellement dans le chat.

---

## TEST 15 — RepoMap (AST Codebase Graph)

**Chemin d'acces** : Invisible.

### Observations
- `RepoMap` parcourt le workspace et extrait les symboles (fonctions, classes, structs, traits, enums) par regex multi-langage (Rust, Python, JS/TS, Go).
- Genere un resume textuel compact injecte dans le prompt LLM.
- Detection automatique du langage par extension.

### Verdict accessibilite : ❌ Inaccessible (backend pur)
### Note : C'est un composant de contexte, pas un outil utilisateur. Mais une **vue "Codebase Map"** serait utile pour la decouverte. Pas critique.

---

## TEST 16 — Project Memory (rules.md)

**Chemin d'acces** : Via fichier `.ownstack/rules.md` ou `AGENTS.md`.

### Observations
- Sections structurees : Forbidden, Coding Style, Testing, Preferences, Libraries, Knowledge, Custom.
- Priorite par section (Forbidden=1.0 > Coding=0.8 > Testing=0.7 > ...).
- Boost par pertinence au contexte de la tache courante.
- Hot-reload par content hash.

### Verdict accessibilite : ⚠️ Accessible indirectement
### Probleme
- **P16** : Pas d'editeur dedie pour rules.md. L'utilisateur doit creer manuellement `.ownstack/rules.md` sans guidance. Pas de template, pas de bouton "Configure project rules".

---

## TEST 17 — Model Router (routing.json)

**Chemin d'acces** : Via fichier `.ownstack/routing.json`.

### Observations
- Permet de router les modeles par role (planner/worker/critic) ou par type de tache.
- Support OpenRouter provider preferences.
- Hot-reload, tolerant au BOM Windows.

### Verdict accessibilite : ⚠️ Accessible indirectement
### Probleme
- **P17** : Pas d'UI pour configurer le routing. Il faut editer manuellement le JSON sans schema visible.

---

## TEST 18 — InfraSense (System Metrics)

**Chemin d'acces** : Invisible.

### Observations
- Collecte RAM, Disk, CPU via APIs systeme (/proc/meminfo, df, Windows FFI).
- Alertes si RAM > 90% ou Disk > 95%.
- Support cross-platform (Linux, Windows, macOS).

### Verdict accessibilite : ❌ Inaccessible
### Probleme
- **P18** : Pas de widget "System Health" dans la status bar. Les alertes ne sont pas affichees. L'utilisateur ne sait jamais si son systeme est sous pression.

---

## TEST 19-24 — Features Infrastructure (Context Manager, Artifact Manager, Policy Engine, Sandbox, Resilience, Signed Toolkits)

Ces features sont purement back-end et n'ont pas besoin d'UI directe :
- **Context Manager** : Gestion de fenetre de contexte LLM (trim automatique). → Expose via `context_current/context_max` dans le chat ✅
- **Artifact Manager** : Extraction de `<artifact>` tags des reponses LLM. → Invisible ❌
- **Policy Engine** : Evaluation Auto/Ask/Blocked pour chaque commande. → Visible dans l'audit log ✅
- **Sandbox** : Execution isolee (Linux seccomp, Docker, macOS). → Transparent ✅
- **Resilience** : Retry HTTP avec backoff exponentiel. → Transparent ✅
- **Signed Toolkits** : Verification ed25519 des toolkits tiers. → Invisible mais correct ✅

---

## TEST 25 — UI Snapshot (Vision Bridge)

**Chemin d'acces** : Bouton dans la status bar + commande `ownstack.capture_ui_snapshot`.

### Observations
- `take_ui_snapshot()` serialise : fichiers ouverts, onglet actif, panels visibles, position curseur, diagnostics, git status, terminal state.
- Sauvegarde dans `.ownstack/ui_snapshot.json`.
- Le chat a un bouton "attach" qui declenche `UiSnapshotRequest`.

### Verdict accessibilite : ✅ Accessible via 2 chemins

---

## Matrice de synthese

| # | Feature | UI Panel | Palette | Status Bar | Chat | Fichier Config | Score |
|---|---------|----------|---------|------------|------|----------------|-------|
| 1 | Onboarding | ✅ overlay | - | - | - | - | ✅ |
| 2 | AI Chat | ✅ `RightTop` | - | - | ✅ | - | ✅ |
| 3 | Missions | - | - | ⚠️ texte | ⚠️ inline | - | ⚠️ |
| 4 | OwnStack Palette | ✅ overlay | ✅ | - | - | - | ⚠️ |
| 5 | Audit Log | ⚠️ pas de panel icon | - | ✅ lien | - | - | ⚠️ |
| 6 | MCP Manager | ✅ `LeftBottom` | - | - | - | ✅ json | ✅ |
| 7 | Status Bar | - | - | ✅ | - | - | ✅ |
| 8 | Empty States | ✅ auto | - | - | ✅ auto | - | ✅ |
| 9 | **Browser** | ❌ | ❌ | ❌ | ⚠️ stub | - | ❌ |
| 10 | **Time Machine** | ❌ | ❌ | ❌ | ⚠️ agent seul | - | ❌ |
| 11 | Vision | - | - | ✅ bouton | ✅ attach | - | ⚠️ |
| 12 | **Multivers** | ❌ | ❌ | ❌ | ⚠️ agent seul | ✅ env var | ❌ |
| 13 | **Healer** | ❌ | ❌ | ❌ | ⚠️ agent seul | - | ❌ |
| 14 | **Specialists** (x6) | ❌ | ❌ | ❌ | ⚠️ agent seul | - | ❌ |
| 15 | RepoMap | - (backend) | - | - | - | - | N/A |
| 16 | Project Memory | - | - | - | - | ⚠️ rules.md | ⚠️ |
| 17 | Model Router | - | - | - | - | ⚠️ routing.json | ⚠️ |
| 18 | **InfraSense** | ❌ | ❌ | ❌ | ❌ | - | ❌ |
| 19 | Context Manager | - | - | - | ✅ bar | - | ✅ |
| 20 | Artifact Manager | - (backend) | - | - | - | - | N/A |
| 21 | Policy Engine | - | - | - | - | ✅ audit | ✅ |
| 22 | Sandbox | - (transparent) | - | - | - | - | ✅ |
| 23 | Resilience | - (transparent) | - | - | - | - | ✅ |
| 24 | Signed Toolkits | - (transparent) | - | - | - | - | ✅ |
| 25 | UI Snapshot | - | ✅ cmd | ✅ bouton | ✅ attach | - | ✅ |

---

## Verdict : Taux d'accessibilite

- **Pleinement accessible** : 10/25 (40%)
- **Partiellement accessible** (UI indirecte ou degradee) : 7/25 (28%)
- **Inaccessible** (aucun chemin UI) : 5/25 (20%)
- **Backend pur** (pas besoin d'UI) : 3/25 (12%)

**Score d'accessibilite effectif : 68%** (accessible + partiel) sur les features qui necessitent une UI.

---

## Les 10 ameliorations les plus urgentes (classees par impact utilisateur)

| # | Priorite | Action | Feature concernee | Effort estime |
|---|----------|--------|-------------------|---------------|
| **1** | **Critique** | Creer un panneau Time Machine avec timeline visuelle, boutons Snapshot/Restore, diff entre snapshots | Time Machine | ~300 LOC, nouveau `PanelKind::TimeMachine` |
| **2** | **Critique** | Ajouter un raccourci clavier visible (ex: `Ctrl+Shift+A`) et un bouton toolbar pour la OwnStack Palette | OwnStack Palette | ~30 LOC |
| **3** | **Haute** | Enrichir la palette avec des actions pour chaque toolkit : "Create Snapshot", "Run A/B Test", "Auto-heal", "Request Code Review", "Generate Docs", "Security Audit" | Palette + tous toolkits | ~80 LOC (ajout de `SuggestedAction` entries) |
| **4** | **Haute** | Bloquer le chat quand `bridge_connected == false` + bandeau d'avertissement | Chat | ~60 LOC |
| **5** | **Haute** | Creer un panneau Mission dedie (`PanelKind::OwnStackMission`) avec liste, statut, historique, replay | Mission System | ~400 LOC |
| **6** | **Moyenne** | Ajouter un widget InfraSense dans la status bar (icone coeur/jauge) avec tooltip metriques | InfraSense | ~80 LOC |
| **7** | **Moyenne** | Ajouter un bouton "Auto-fix" dans le panneau Problems et dans le terminal apres une commande en echec | Healer | ~120 LOC |
| **8** | **Moyenne** | Ajouter l'icone Audit dans la sidebar (`PanelKind::OwnStackAudit`) pour une decouverte directe | Audit | ~20 LOC |
| **9** | **Basse** | Ajouter un template `.ownstack/rules.md` generee automatiquement au setup workspace + editeur guide | Project Memory | ~100 LOC |
| **10** | **Basse** | Implementer reellement le browser toolkit (webview ou headless chrome) ou le retirer pour eviter la confusion | Browser | Decision architecturale |

---

## Resume executif

**L'architecture back-end d'OwnStack est exceptionnelle** : 25 features, orchestrateur multi-agent, policy engine, sandbox, self-healing, A/B testing, time travel, 6 agents specialises, RepoMap, Project Memory. C'est l'une des architectures agent-IDE les plus completes du marche.

**Le probleme est un ecart massif entre le back-end et le front-end.** 5 des features les plus differenciantes (Time Machine, Multivers, Healer, Specialists, InfraSense) n'ont **aucun point d'entree UI**. L'utilisateur ne sait meme pas qu'elles existent. La palette ne contient que 3 actions sur ~15 possibles.

**La priorite #1 absolue est de rendre la Time Machine accessible** — c'est la feature qui rassure le plus l'utilisateur face a un agent AI autonome ("je peux toujours revenir en arriere"). Sans UI, la promesse de securite d'OwnStack est invisible.

**La priorite #2 est de rendre la palette decouvrable** — un raccourci clavier + un bouton visible + des actions pour chaque toolkit transformeraient immediatement la perception de richesse de l'IDE.
