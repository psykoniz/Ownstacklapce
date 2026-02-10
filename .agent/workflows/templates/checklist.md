# Checklist d'exploration de codebase

## Phase 1 — Exploration globale

### Identification du projet
- [ ] Type de projet identifié (Rust/Node/Python/Go/autre)
- [ ] Fichier de config principal lu (Cargo.toml/package.json/go.mod/pyproject.toml)
- [ ] README.md lu et compris

### Cartographie
- [ ] Nombre total de fichiers source comptés
- [ ] Estimation LOC obtenue
- [ ] Structure des répertoires (2 niveaux) documentée
- [ ] Fichiers de config listés

### Workspace/Monorepo (si applicable)
- [ ] Membres du workspace identifiés
- [ ] Dépendances internes mappées

---

## Phase 2 — Lecture ciblée

### Points d'entrée
- [ ] main.rs / index.ts / main.go / __main__.py localisé
- [ ] lib.rs / lib.ts / pkg/ identifié (si bibliothèque)
- [ ] Entry points documentés

### Dépendances
- [ ] Dépendances directes listées
- [ ] Versions vérifiées
- [ ] Dépendances manquantes identifiées

### Code custom/critique
- [ ] Modules importants identifiés
- [ ] Code custom/métier localisé

---

## Phase 3 — Vérification d'existence

### Fichiers référencés
- [ ] Tous les fichiers déclarés dans la config existent
- [ ] Tous les répertoires de workspace existent
- [ ] Pas de références cassées

### Modules
- [ ] Chaque `mod X` a son fichier X.rs / X/mod.rs
- [ ] Chaque `import` a sa cible
- [ ] Pas de fichiers orphelins

---

## Phase 4 — Analyse du chaînage

### Déclarations
- [ ] Modules correctement déclarés (mod/export)
- [ ] Visibilité correcte (pub/export)

### Utilisations
- [ ] Imports cohérents avec les déclarations
- [ ] Pas d'imports circulaires problématiques
- [ ] Chemins d'import corrects

---

## Phase 5 — Compilation/Validation

### Build
- [ ] cargo check / npm run build / go build exécuté
- [ ] Erreurs de compilation documentées
- [ ] Warnings notés

### Linting (optionnel)
- [ ] Linter configuré
- [ ] Pas d'erreurs bloquantes

### Tests (optionnel)
- [ ] Tests présents
- [ ] Tests passent

---

## Phase 6 — Synthèse

### Rapport
- [ ] Problèmes critiques listés avec preuves
- [ ] Avertissements documentés
- [ ] Points positifs notés
- [ ] Recommandations priorisées

### Livrables
- [ ] Rapport généré
- [ ] Actions prioritaires définies

---

## Notes

[Espace pour notes additionnelles]
