# OwnStack IDE — État actuel et travail en cours

**Date** : 29 juin 2026  
**Branche** : `master` (commit `f878db73` + correctifs locaux non-committés)

---

## 1. Qu'est-ce que OwnStack IDE ?

OwnStack IDE est un éditeur de code natif (Rust) basé sur un fork de Lapce/Floem. Il intègre un agent IA capable de :
- Chat conversationnel (Ask mode)
- Planification de tâches (Plan mode)
- Exécution autonome de modifications (Auto/Project mode)
- Autocomplétion FIM (Fill-in-the-Middle)
- Support MCP (Model Context Protocol) pour connecter des outils externes
- Panneau d'audit pour tracer toutes les actions de l'IA

**Architecture** :
- `lapce-app/` — Interface IDE (Rust/Floem, rendu GPU via wgpu)
- `ownstack-agent/` — Agent IA (orchestrateur, toolkits, LLM streaming)
- `lapce-proxy/` — Proxy LSP + pont agent
- `lapce-rpc/` — Protocole RPC entre l'IDE et le proxy

**Provider IA** : OpenRouter (codex-everywhere.com) avec clé API configurable via l'UI d'onboarding.

---

## 2. Bugs résolus dans cette session

### Bug A : Cercle teal (artefact de rendu GPU) — RÉSOLU

**Symptôme** : Un grand cercle bleu-vert sombre recouvrait le bas-droite de l'IDE, par-dessus le panneau chat et la zone de saisie.

**Cause racine** : Le widget `scroll()` de Floem dans `ownstack_chat.rs` n'avait pas de background opaque. Sur Windows/wgpu, le GPU composite mal les régions scroll transparentes, créant un artefact circulaire.

**Fix** : Ajout de `.background(config.color(LapceColor::PANEL_BACKGROUND))` au style du scroll widget principal (ligne ~858).

**Fichiers modifiés** :
- `lapce-app/src/ownstack_chat.rs` — background scroll + suppression box shadows
- `lapce-app/src/ownstack_empty_state.rs` — suppression box shadows
- `lapce-app/src/ownstack_palette.rs` — suppression box shadows

**Leçon** : Toujours ajouter un background opaque aux widgets `scroll()` dans les panneaux OwnStack.

---

### Bug B : Réponses IA non affichées — EN COURS

**Symptôme** : L'utilisateur envoie un message, la bulle bleue (User) s'affiche, mais la réponse de l'IA (Assistant) n'apparaît jamais visuellement.

**Diagnostic** :
1. Les données SONT présentes (vérifié via l'endpoint E2E `get_chat` — 7 messages avec User, System, Assistant, Alert)
2. Le streaming fonctionne (le monitor affiche "Tool read: streaming")
3. Le panneau chat est extrêmement étroit (~30px), rendant les messages impossibles à voir
4. Le background des bulles Assistant (`SURFACE_0` = rgb(14,18,28)) était quasi-identique au `PANEL_BACKGROUND`

**Corrections appliquées** (non-committées) :
1. **Background amélioré** : `SURFACE_0` → `SURFACE_2` (rgb(28,34,48)) pour les bulles assistant — plus de contraste
2. **Bordure visible** : `SURFACE_0 * 0.59` → `BORDER` (rgba(120,140,180,70)) — bordure réellement visible
3. **Rendu texte** : `text()` → `label()` pour le contenu des messages — `text()` de Floem peut avoir des problèmes de layout sur Windows
4. **Largeur minimum panneau** : Le panneau droit a maintenant un minimum de 200px (au lieu de pouvoir être réduit à ~0px par drag)

**Fichiers modifiés** :
- `lapce-app/src/ownstack_chat.rs` — lignes 1345-1358 (couleurs bulles), ligne 1499 (text→label)
- `lapce-app/src/panel/view.rs` — ligne 451 (min_width 200px pour panneau droit)

**État** : Build en cours. Non encore vérifié visuellement.

---

## 3. Bugs connus non résolus

### Bug C : Repaint winit (request_redraw ne fonctionne pas)

Le IDE ne se rafraîchit qu'au resize de la fenêtre. Root cause : `winit::Window::request_redraw()` ne génère pas `RedrawRequested` sur cette machine Dell/Windows 11. Ce n'est PAS un bug OwnStack — c'est un problème winit/GPU driver.

**Impact** : Les mises à jour réactives (nouvelles messages, streaming) sont dans les données mais ne se redessinent pas automatiquement. L'utilisateur doit redimensionner la fenêtre pour voir les changements.

**Piste de résolution** : Mise à jour du driver GPU, mise à jour de winit, ou workaround avec un timer de repaint forcé.

### Bug D : Contenu Settings écrasé (B2 des sessions précédentes)

Le panneau Settings a un problème de layout où le contenu est compressé.

### Bug E : Palette IA — Esc ne ferme pas (B3 des sessions précédentes)

La touche Escape ne ferme pas la palette IA quand elle est ouverte.

---

## 4. Structure de l'interface chat

```
┌─────────────────────────────────────────────────────────────────┐
│ Hub bar : [Chat] [Tools] [Audit]                                │
├─────────────────────────────────────────────────────────────────┤
│ Mode bar : [Ask] [Plan] [Auto] [Project]                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│ scroll(                                                         │
│   empty_state (caché quand il y a des messages)                 │
│   current_mission (si actif)                                    │
│   dyn_stack(messages) ← User (bleu, droite) + Assistant (gris)  │
│   dyn_stack(streaming) ← contenu en cours de streaming          │
│   thinking_indicator ← "AI is thinking..."                      │
│ ) .background(PANEL_BACKGROUND)                                 │
│                                                                 │
├─────────────────────────────────────────────────────────────────┤
│ Monitor : [Output] [Problems]                                   │
│ "[18:37:24] system: Tool read: streaming"                       │
│ "Context: 213/128000 tokens used"                               │
├─────────────────────────────────────────────────────────────────┤
│ Input area : [+ Context] [__________________] [Send/Stop]       │
├─────────────────────────────────────────────────────────────────┤
│ Shortcut hints                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## 5. Flux de données des messages

1. **Envoi** : `send_message()` → ajoute User msg à `messages` signal → envoie `AiPrompt` via RPC au proxy
2. **Streaming** : Proxy relaie au agent → `receive_chunk()` → accumule dans `streaming_content` signal
3. **Fin** : `finish_reason` reçu → `receive_response()` → ajoute Assistant msg à `messages` + clear `streaming_content`
4. **Persistance** : Chaque ajout déclenche `db.save_ownstack_chat()` → fichier JSON dans le dossier workspace
5. **Restauration** : Au démarrage, `db.get_ownstack_chat()` charge les messages sauvegardés

---

## 6. Test E2E

L'IDE expose un serveur de contrôle JSON-RPC (activé via `OWNSTACK_E2E=1`, port affiché dans stdout).

**Méthodes disponibles** : ping, open_workspace, open_file, editor_set_text, save, undo, redo, find_replace, run_command, ai_prompt, get_chat, get_state, get_diagnostics, get_editor_text, wait_idle, screenshot

**Exemple** :
```bash
# Vérifier que l'IDE répond
curl -s http://localhost:61610 -d '{"jsonrpc":"2.0","id":1,"method":"ping"}'

# Obtenir l'état du chat
curl -s http://localhost:61610 -d '{"jsonrpc":"2.0","id":2,"method":"get_chat"}'

# Envoyer un prompt IA
curl -s http://localhost:61610 -d '{"jsonrpc":"2.0","id":3,"method":"ai_prompt","params":{"prompt":"Hello"}}'
```

---

## 7. Prochaines étapes

1. **Vérifier** que le build compile et que les messages Assistant s'affichent avec les nouvelles couleurs
2. **Résoudre** le bug de repaint winit (investigation GPU driver ou workaround timer)
3. **Résoudre** le bug Settings (B2) et Palette Esc (B3)
4. **Crédits API** : L'API OpenRouter retourne HTTP 402 (crédits insuffisants). Il faut recharger les crédits ou utiliser un autre provider.
5. **Commit** les correctifs une fois vérifiés

---

## 8. Commandes utiles

```powershell
# Build release
cargo build --release -p lapce-app

# Lancer l'IDE avec E2E
$env:OWNSTACK_E2E = "1"; .\target\release\ownstack-ide.exe

# Screenshot de la fenêtre IDE
.\scripts\ui_screenshot.ps1 -Window -Output "$env:TEMP\shot.png"
```
