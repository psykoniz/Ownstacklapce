# Plan d'amélioration — OwnStack IDE UX Issues

> Plan révisé après audit du code source. Les problèmes sont reclassés par impact réel.

---

## Problème 1 (Haute) — Chat silencieux quand le bridge est déconnecté

**Constat** : `ownstack_chat.rs` n'a aucune référence à `bridge_connected`. Le `send_message()` (l.224) met `is_loading = true` et envoie le message sans vérifier l'état du bridge. Si le bridge est down, le message est perdu silencieusement.

**Fichiers concernés** :
- `lapce-app/src/ownstack_chat.rs` — `send_message()` (~l.220)
- `lapce-app/src/ownstack_status.rs` — `bridge_connected: RwSignal<bool>` (l.32)

**Plan d'implémentation** :

1. **Injecter la référence au bridge status dans `OwnStackChatData`** :
   - Ajouter un champ `bridge_connected: ReadSignal<bool>` dans `OwnStackChatData`
   - Le passer depuis `WindowTabData` lors de la construction

2. **Bloquer l'envoi si déconnecté** dans `send_message()` :
   ```rust
   pub fn send_message(&self) {
       if !self.bridge_connected.get() {
           self.messages.update(|msgs| {
               msgs.push(ChatMessage {
                   role: ChatRole::Alert,
                   content: "Agent bridge is disconnected. Check the status bar.".into(),
                   ..
               });
           });
           return;
       }
       // ... existing logic
   }
   ```

3. **Griser visuellement le champ input** dans `ownstack_chat_panel()` (~l.900) :
   ```rust
   .style(move |s| {
       s.apply_if(!bridge_connected.get(), |s| {
           s.opacity(0.5)
            .cursor(CursorStyle::NotAllowed)
       })
   })
   ```

4. **Ajouter un bandeau d'avertissement** au-dessus de la zone de saisie quand déconnecté :
   - Petit `label("⚠ Bridge disconnected")` en rouge/ambre, conditionnel sur `!bridge_connected.get()`

**Estimation** : ~60 lignes modifiées, 2 fichiers.

---

## Problème 2 (Haute) — Pas de notification quand le LSP est absent

**Constat** : Quand aucun language server n'est disponible pour un langage donné, le completion dropdown (`completion.rs`) reste simplement vide (`CompletionStatus::Inactive`). Aucun feedback visuel.

**Fichiers concernés** :
- `lapce-app/src/proxy.rs` — communication avec le proxy LSP
- `lapce-app/src/plugin.rs` — gestion des plugins/volts LSP
- `lapce-app/src/window_tab.rs` — notifications et commandes internes

**Plan d'implémentation** :

1. **Détecter l'absence de LSP** : Dans le proxy, quand une completion request retourne parce qu'aucun serveur n'est disponible pour le `language_id`, émettre une `InternalCommand::ShowMessage` ou un nouveau signal.

2. **Ajouter une notification non-bloquante** :
   - Réutiliser le système d'alerte existant (la barre de notification en haut)
   - Message : `"No language server found for {language}. Check available plugins."`
   - Ne montrer qu'une fois par session/langage (stocker un `HashSet<String>` des langages déjà avertis)

3. **Ajouter un lien actionnable** vers le panneau plugins :
   - Le clic sur la notification ouvre `PanelKind::Plugin` avec un filtre pré-rempli sur le langage

**Estimation** : ~80 lignes, 3-4 fichiers.

---

## Problème 3 (Moyenne) — Breadcrumbs manquants dans l'éditeur

**Constat** : L'icône `BREADCRUMB_SEPARATOR` existe dans `config/icon.rs` (l.39) mais aucune breadcrumb bar n'est implémentée dans `editor/view.rs`. Le sticky header (lignes collantes en haut) compense partiellement mais ne montre pas le chemin fichier + hiérarchie de symboles.

**Fichiers concernés** :
- `lapce-app/src/editor/view.rs` — vue éditeur principale
- `lapce-app/src/config/editor.rs` — ajout d'un toggle `breadcrumbs_enabled`
- `lapce-app/src/config/icon.rs` — icône séparateur déjà présente

**Plan d'implémentation** :

1. **Créer un composant `EditorBreadcrumbs`** :
   - Nouveau fichier `lapce-app/src/editor/breadcrumbs.rs`
   - Affiche : `workspace_root / relative_path / Symbol > SubSymbol`
   - Utilise les symboles document LSP (`DocumentSymbol`) déjà requêtés par l'éditeur
   - Chaque segment est cliquable (fichier → ouvre file picker, symbole → scroll to)

2. **Intégrer dans la vue éditeur** :
   - Dans `editor/view.rs`, insérer la breadcrumb bar entre le header d'onglet et le viewport de l'éditeur
   - Hauteur fixe ~24px, fond `PANEL_BACKGROUND`, séparateurs avec `BREADCRUMB_SEPARATOR`

3. **Config toggle** :
   - Ajouter `editor.breadcrumbs` (bool, default `true`) dans `EditorConfig`
   - Respecter le toggle dans le rendu

**Estimation** : ~200 lignes nouveau fichier + ~30 lignes intégration, 3-4 fichiers.

---

## Problème 4 (Moyenne) — Pas de bouton Run/Debug visible

**Constat** : Le système de debug/run existe (`debug.rs`, `DapData`, `panel/debug_view.rs`, `SourceBreakpoint`). Mais il n'y a pas de bouton Run dans la toolbar ou l'éditeur. L'utilisateur doit connaître le raccourci ou utiliser la palette.

**Fichiers concernés** :
- `lapce-app/src/debug.rs` — `DapData`, configurations de lancement
- `lapce-app/src/panel/debug_view.rs` — vue du panneau debug
- `lapce-app/src/title.rs` — barre de titre (lieu naturel pour un bouton Run)
- `lapce-app/src/code_lens.rs` — code lens inline (boutons au-dessus des fonctions)

**Plan d'implémentation** :

1. **Ajouter un bouton Run dans la barre de titre** (`title.rs`) :
   - Icône play (▶) à droite de la zone centrale
   - Clic → lance la configuration de run par défaut (ou ouvre un sélecteur si plusieurs)
   - Dropdown au survol : liste des configurations disponibles
   - Bouton debug (🔴) à côté

2. **Améliorer les Code Lens** (`code_lens.rs`) :
   - Au-dessus de `fn main()`, `def main()`, `if __name__` : afficher un lien "▶ Run | 🐛 Debug"
   - Réutiliser le mécanisme de `CodeLens` LSP existant

3. **Raccourci clavier visible** :
   - `F5` pour Run, `Ctrl+F5` pour Debug (afficher dans le tooltip du bouton)

**Estimation** : ~120 lignes, 2-3 fichiers.

---

## Problème 5 (Basse) — Pas de tabs pinned

**Constat** : `EditorTabData` n'a aucun concept de "pin". Tous les onglets sont équivalents et se comportent de la même façon (fermeture, réorganisation).

**Fichiers concernés** :
- `lapce-app/src/editor_tab.rs` — `EditorTabData`, `EditorTabChild`
- `lapce-app/src/editor/view.rs` — rendu des onglets

**Plan d'implémentation** :

1. **Ajouter un flag `pinned: bool`** dans la structure de chaque tab child
2. **Comportement pinned** :
   - Les onglets pinned sont groupés à gauche
   - Taille réduite (icône seulement, pas de label complet)
   - Pas de bouton close visible (il faut Unpin d'abord)
   - Menu contextuel : "Pin Tab" / "Unpin Tab"
3. **Persistance** : Sauvegarder l'état pinned dans la DB workspace

**Estimation** : ~100 lignes, 2 fichiers.

---

## Problème 6 (Basse) — Onboarding : clarifier le mode "Skip"

**Constat** : Le bouton Skip existe déjà (`ownstack_onboarding.rs` l.401) mais `skip()` appelle `finish()` qui persiste l'état avec les valeurs par défaut (OpenRouter sans clé). L'IDE se retrouve configuré pour un provider sans credentials.

**Fichiers concernés** :
- `lapce-app/src/ownstack_onboarding.rs` — `skip()`, `finish()`

**Plan d'implémentation** :

1. **Créer un `skip_without_ai()`** distinct de `skip()` :
   ```rust
   pub fn skip_without_ai(&self) {
       // Mark onboarding complete but set provider to "None"
       self.chosen_provider.set("None".to_string());
       self.finish();
   }
   ```

2. **Ajouter un lien "Continue without AI"** dans le step ProviderSetup :
   - Texte discret sous les trois options de provider
   - Appelle `skip_without_ai()`

3. **Propager le mode "None"** :
   - Quand `chosen_provider == "None"`, le chat panel affiche un message statique invitant à configurer un provider
   - L'inline completion IA est désactivée

**Estimation** : ~40 lignes, 1-2 fichiers.

---

## Ordre de priorité recommandé

| Ordre | Problème | Effort | Impact UX |
|-------|----------|--------|-----------|
| 1 | P1 — Bridge déconnecté | Faible (~60 LOC) | Critique — perte silencieuse de messages |
| 2 | P2 — Notification LSP absent | Moyen (~80 LOC) | Haute — confusion premier usage |
| 3 | P6 — Onboarding skip sans IA | Faible (~40 LOC) | Haute — bloquant pour les non-utilisateurs IA |
| 4 | P4 — Bouton Run visible | Moyen (~120 LOC) | Moyenne — accessibilité |
| 5 | P3 — Breadcrumbs | Élevé (~230 LOC) | Moyenne — navigation |
| 6 | P5 — Tabs pinned | Moyen (~100 LOC) | Basse — confort |

**Total estimé** : ~630 lignes de code sur 10-15 fichiers.
