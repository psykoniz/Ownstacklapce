# OwnStack IDE — UI Polish Progress

## Objectif
Rendre l'IDE OwnStack visuellement clean, beau et ergonomique en identifiant et corrigeant tous les problèmes visuels.

---

## Corrections appliquées

### 1. Welcome Tab Placeholder (FAIT)
- **Fichier** : `lapce-app/src/app.rs:1468`
- **Problème** : Le tab Welcome affichait un simple `text("Welcome")` au lieu du branding OwnStack
- **Fix** : Remplacé par `empty_editor_placeholder()` qui affiche le diamant OwnStack, le nom de marque, et un bouton "Open Folder"

### 2. Préfixe `\\?\` dans le File Explorer (FAIT)
- **Fichier** : `lapce-app/src/file_explorer/view.rs:191`
- **Problème** : Sur Windows, les chemins affichaient le préfixe technique `\\?\` (extended path)
- **Fix** : Ajout de `strip_prefix(r"\\?\")` pour nettoyer l'affichage
- **Vérifié** : Fonctionne dans les screenshots du Doctor

### 3. Empty Editor Overlay pour Scratch Buffers (FAIT)
- **Fichier** : `lapce-app/src/editor/view.rs:1347-1371`
- **Problème** : Quand l'IDE ouvre un scratch buffer vide au démarrage, l'éditeur est complètement blanc
- **Fix** : Overlay conditionnel qui affiche `empty_editor_placeholder()` quand :
  - Le contenu est `DocContent::Scratch` ou `DocContent::Local`
  - Le buffer est vide (`rope_len <= 1`)
  - L'overlay utilise `z_index(10)` et `absolute` positioning
- **Import** : `DocContent` importé à la ligne 62

### 4. Doctor.py — Compatibilité Windows (FAIT, commit 814b53ce)
- Encodage UTF-8 au lieu de cp1252
- Extension `.exe` et nom `ownstack-ide` auto-détectés
- Screenshot via PowerShell/NET au lieu de xwd
- URL API configurable via `ANTHROPIC_BASE_URL`
- Timeout 1800s pour les builds cargo
- `_safe_print()` pour les caractères Unicode
- Null check dans `apply_fixes()` pour les issues sans fichier

---

## Problèmes — résolus / diagnostiqués

### Cercle bleu translucide (bottom-right) — ✅ RÉSOLU (pas un bug)
- **Symptôme** : Grand cercle teal/bleu (~500px) dans le coin bas-droite sur `iteration_1`.
- **Diagnostic final (2026-06-26)** :
  - Recherche exhaustive : **aucun élément UI** ne fait ~500px (plus gros `border_radius` = pills `999`, blur max `40`). **Aucun gradient / radial / brush** dans tout `lapce-app/src`.
  - **Test décisif** : capture `verify_rebuild.png` en fenêtre **maximisée** → **le cercle a disparu**. Présent uniquement en fenêtre flottante.
  - **Conclusion** : ce n'est pas notre UI. C'est le **wallpaper / compositeur Windows (coins arrondis + ombre DWM)** qui transparaît au bord de la fenêtre flottante. **Aucune correction code nécessaire.**

### Éditeur vide au démarrage — 🔧 VRAIE CAUSE TROUVÉE + FIX
- ❌ L'hypothèse initiale était fausse : au démarrage l'IDE ne crée **PAS** d'`EditorTabChild` Scratch.
  La capture `verify_rebuild.png` confirme l'**absence totale de barre d'onglets** → le **root split n'a aucun enfant**.
- Donc l'overlay de `editor/view.rs:1347-1371` (fix #3) ne peut pas se déclencher : il n'y a aucun éditeur à décorer.
  (Le fix #3 reste utile pour un éditeur Scratch *ouvert* mais vide.)
- ✅ **Vrai fix appliqué + VÉRIFIÉ** : `app.rs` → `main_split()`. La `split_list` est désormais enveloppée dans un `stack`
  avec un overlay `empty_editor_placeholder()` affiché quand `root_split.children.is_empty()`.
  → Capture `verify_rebuild.png` (06:08, binaire 04:34) : la zone éditeur affiche le diamant OwnStack,
  « OwnStack / AI-native code editor », le bouton **« Open Folder »** et le hint « Ctrl+O ». **Plus d'écran noir.**

---

## Architecture UI — Mapping fichiers ↔ éléments visuels

> ✅ **Classement terminé et vérifié contre le code (2026-06-26).** Toutes les vues
> OwnStack (`ownstack_*.rs`) et tous les fichiers UI core (y compris `panel/*_view.rs`,
> `terminal/`, popups) sont désormais répertoriés avec leur point d'entrée. Objectif
> « ne plus galérer à retrouver l'élément ↔ fichier » : atteint.

```
┌─────────────────────────────────────────────────────────┐
│  Title Bar                  title.rs                     │
│  left():28  middle():151  right():307                   │
├────────┬──────────────────────────────┬─────────────────┤
│ Left   │  Main Editor Area            │  Right Panel    │
│ Panel  │  app.rs:main_split():2091    │  panel/view.rs  │
│        │  editor_tab():1490           │                 │
│ File   │  editor_tab_content():1277   │  Chat Panel     │
│Explorer│  editor_container_view()     │  ownstack_      │
│view.rs │    editor/view.rs:1277       │  chat.rs:533    │
│        │                              │                 │
│        │  Empty State:                │  Tools Tab      │
│        │    ownstack_empty_state.rs   │  MCP Panel      │
│        │    :36 editor placeholder    │  ownstack_      │
│        │                              │  mcp.rs         │
│        │                              │                 │
│        │                              │  Audit Tab      │
│        │                              │  ownstack_      │
│        │                              │  audit.rs       │
├────────┴───────────────────┬──────────┴─────────────────┤
│  Bottom Panel              │                             │
│  Terminal / Output / Problems                            │
│  panel/view.rs:panel_container_view():210               │
├─────────────────────────────────────────────────────────┤
│  Status Bar                status.rs:32                  │
│  Mode badge(Ask/Plan/Auto) Budget indicator              │
└─────────────────────────────────────────────────────────┘
```

### Fichiers OwnStack personnalisés (classement complet — vérifié vs code)
| Fichier | Rôle | Vue ? | Point d'entrée vue |
|---------|------|-------|--------------------|
| `ownstack_theme.rs` | Tokens de design (couleurs, rayons, espacements) | ❌ tokens | — |
| `ownstack_empty_state.rs` | Placeholders vides partagés (éditeur, chat, MCP, audit) | ✅ | `empty_editor_placeholder()` |
| `ownstack_chat.rs` | Panel AI Chat avec modes Ask/Plan/Auto + cartes messages (dont SYSTEM) | ✅ | `chat_panel()` / `message_view` ~1340 |
| `ownstack_onboarding.rs` | Wizard de première utilisation | ✅ | `onboarding_view()` :245 |
| `ownstack_audit.rs` | Log d'audit des actions AI (panel + overlay plein écran) | ✅ | `audit_panel()` :253, `ownstack_audit_overlay()` :437 |
| `ownstack_mcp.rs` | Panel de serveurs MCP | ✅ | `mcp_panel()` :385 |
| `ownstack_palette.rs` | Palette de commandes OwnStack | ✅ | `ownstack_palette_view()` :225 |
| `ownstack_preview.rs` | Panel Web Preview (détecte un dev server Vite/Next…) | ✅ | module `//!` Web Preview panel |
| `ownstack_inline_edit.rs` | Édition AI inline dans l'éditeur (style Cmd+K) — état | 🟡 état | `InlineEditData`, rendu dans `editor/view.rs` |
| `ownstack_status.rs` | **Données** du status bar (run state, budget) — pas de vue | ❌ data | `OwnStackStatusData` |
| `ownstack_fim.rs` | Autocomplétion inline (Fill-in-the-Middle) — état client | ❌ logique | `FimRequest` |
| `ownstack_mentions.rs` | Parsing/expansion des `@`-mentions du chat | ❌ logique | — |
| `ownstack_tests.rs` | Tests — pas d'UI | ❌ tests | — |

### Fichiers Lapce core UI (classement complet — vérifié vs code)

**Chrome principal (toujours visible)**
| Fichier | Rôle |
|---------|------|
| `app.rs` | Layout principal, composition des vues, empty state éditeur |
| `title.rs` | Barre de titre (menu, navigation, settings, badge notif) |
| `status.rs` | Barre de statut (mode Ask/Plan/Auto, budget, git) |
| `panel/view.rs` | Conteneurs de panels (Left, Bottom, Right) + `panel/{mod,data,kind,position,style}.rs` (infra) |

**Zone éditeur**
| Fichier | Rôle |
|---------|------|
| `editor/view.rs` | Vue de l'éditeur (gutter, contenu, find, overlay vide) |
| `editor_tab.rs` | Onglets éditeur (enum `EditorTabChild`) |
| `main_split.rs` | Gestion des splits et tabs |
| `editor/diff.rs` | Vue diff |
| `code_action.rs` | Menu code actions | 
| `code_lens.rs` / `wave.rs` | Code lens / soulignement ondulé (erreurs) |
| `text_input.rs` / `text_area.rs` / `focus_text.rs` | Widgets de saisie |

**Panels latéraux / bas** (`panel/*_view.rs`)
| Fichier | Rôle |
|---------|------|
| `file_explorer/view.rs` | Arborescence de fichiers (label racine, préfixe `\\?\`) |
| `panel/terminal_view.rs` + `terminal/view.rs` + `terminal/panel.rs` | Terminal |
| `panel/problem_view.rs` | Panel Problèmes |
| `panel/debug_view.rs` | Panel Debug |
| `panel/plugin_view.rs` + `plugin.rs` | Panel Extensions |
| `panel/document_symbol.rs` | Outline / symboles |
| `panel/source_control_view.rs` + `source_control.rs` | Git / SCM |
| `panel/global_search_view.rs` + `global_search.rs` | Recherche globale |
| `panel/{call_hierarchy,references,implementation}_view.rs` | Navigation LSP |

**Popups / overlays**
| Fichier | Rôle |
|---------|------|
| `palette.rs` | Palette de commandes (core) |
| `about.rs` | Popup À propos (rend `logo_svg`) |
| `alert.rs` | Alertes / dialogues |
| `rename.rs` | Popup de renommage |
| `settings.rs` | UI des paramètres |
| `keymap.rs` | UI des raccourcis clavier |
| `web_link.rs` | Liens cliquables |

---

## Tests
- 324 tests agent passent
- 34 tests app passent (20 filtrés)
- Build incrémental : ~2-3 secondes

---

## Prochaines étapes
1. ✅ Classement complet des éléments UI ↔ fichiers (fait 2026-06-26)
2. ✅ Fichier `nul` — déjà absent du workspace
3. ✅ Disque libéré (suppression cache `target/debug/incremental`, +4.4 GB)
4. 🔄 Rebuild en cours pour vérifier placeholder + overlay éditeur vide
5. ⬜ Re-lancer le Doctor pour une capture **propre** (les `iteration_2..10` sont des états brouillons obsolètes)
6. ⬜ Corriger : label racine File Explorer tronqué + carte SYSTEM du chat coupée à droite
7. ⬜ Cercle bleu/gris bas-droite : **aucun élément UI ne fait ~500px** (pas de gradient/brush dans le code) → hypothèse confirmée d'artefact compositeur Windows / wallpaper. Vérifier sur capture fraîche si le cercle bouge avec la fenêtre.

### Note disque (issue critique Doctor)
- C: à 99% (était 5.7 GB libre, → ~8-11 GB après nettoyage incrémental).
- ⚠️ Ne PAS passer `debug=1` quand le disque est plein : ça invalide les 12 GB de deps → full rebuild risqué. À faire seulement avec de la marge.
