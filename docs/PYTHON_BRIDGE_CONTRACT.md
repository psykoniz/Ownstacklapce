# Contrat Rust ↔ Python Sidecar (JSON I/O)

Version active: `ownstack.bridge.jsonio@1`

## 1. Requête (Rust -> Python)

Chaque ligne envoyée sur `stdin` doit être un objet JSON-RPC 2.0:

```json
{
  "jsonrpc": "2.0",
  "id": 42,
  "method": "tools.exec",
  "params": { "command": "ls" },
  "contract": {
    "name": "ownstack.bridge.jsonio",
    "version": 1
  }
}
```

Champs obligatoires:
- `jsonrpc`: `"2.0"`
- `id`: entier positif unique par requête
- `method`: string non vide
- `params`: objet JSON (ou valeur JSON sérialisable)
- `contract.name`: `ownstack.bridge.jsonio`
- `contract.version`: `1`

## 2. Réponse (Python -> Rust)

```json
{
  "jsonrpc": "2.0",
  "id": 42,
  "result": { "status": "received" },
  "error": null,
  "contract": {
    "name": "ownstack.bridge.jsonio",
    "version": 1
  }
}
```

Si erreur:

```json
{
  "jsonrpc": "2.0",
  "id": 42,
  "result": null,
  "error": { "code": -32600, "message": "Invalid method" },
  "contract": {
    "name": "ownstack.bridge.jsonio",
    "version": 1
  }
}
```

## 3. Politique de compatibilité

- Le bridge Rust vérifie `jsonrpc`, `id`, et le couple `contract(name, version)`.
- Mode strict activé par défaut.
- Variable d'environnement de secours: `OWNSTACK_BRIDGE_STRICT_CONTRACT=false` pour tolérer des réponses legacy (sans `contract`) pendant une migration.

## 4. Mesures d'usage (Lot A)

Le bridge Rust enregistre chaque appel dans:

- `\.ownstack/python_bridge_metrics.jsonl`
- ou chemin custom via `OWNSTACK_BRIDGE_METRICS_PATH`

Événement JSONL:

```json
{
  "timestamp_ms": 1739999999999,
  "method": "tools.exec",
  "success": true,
  "latency_ms": 123,
  "error": null
}
```

## 5. Génération du tableau de dette

Commande:

```bash
python scripts/report_python_debt.py
```

Sortie:
- `docs/TOP_PYTHON_DEBTS.md`
- triée par score d'impact réel (fréquence + erreurs + latence).
