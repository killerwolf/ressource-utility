# Ressource Utility

Utilitaire léger en Rust pour la barre des menus macOS qui surveille CPU, mémoire, disque et température et donne des conseils via IA.

## Fonctionnalités

- Affichage `CPU · RAM · disque libre` directement dans la barre des menus
- Détection d'anomalies soutenues :
  - CPU > 85 % pendant 15 s
  - mémoire > 90 %
  - disque < 10 % ou < 10 Go libres
  - température > 90 °C
  - processus > 80 % CPU
- Conseils IA courts, en français
- Explication d'un processus inconnu en un clic (réponse mise en cache, jamais payée deux fois)
- Choix du fournisseur (Anthropic, Gemini, OpenAI/Codex) et du modèle
- Clé API stockée dans le Trousseau macOS
- Lancement au démarrage
- Aucune icône dans le Dock

## Économie de tokens

- Aucun appel périodique au LLM.
- Un appel n'a lieu que sur demande manuelle, sur clic sur un processus, ou lorsqu'une anomalie persiste.
- Au maximum 1 appel automatique toutes les 30 minutes, et une même anomalie n'est pas réanalysée avant 2 heures.
- Prompt compact et sortie limitée à 200 tokens.

## Installation

### Depuis une release

1. Téléchargez le fichier `Ressource-Utility-<version>-macos.zip` depuis la page [Releases](../../releases) du dépôt GitHub.
2. Décompressez-le et déplacez `Ressource Utility.app` dans `~/Applications`.
3. Au premier lancement, faites un clic droit sur l'application puis choisissez **Ouvrir**. L'application est signée en ad hoc, macOS demande donc une confirmation supplémentaire.

### Depuis les sources

```sh
./scripts/bundle.sh --install
```

## Configuration

- Ouvrez le menu de la barre des menus puis :
  - **Fournisseur IA** : choisissez Anthropic, Gemini ou OpenAI/Codex.
  - **Configurer la clé API…** : enregistre la clé dans le Trousseau macOS.
  - **Changer le modèle…** : choisissez un autre modèle.
- Modèles par défaut : `claude-haiku-5-5`, `gemini-2.5-flash`, `gpt-5-mini`.
- Fichier de configuration : `~/Library/Application Support/ressource-utility/config.json`

## Développement

```sh
cargo run              # lance l'application dans la barre des menus
cargo run -- --dump    # affiche un instantané des métriques dans le terminal
cargo test             # tests unitaires
cargo clippy           # analyse statique
```

## CI / Release

- Chaque push et pull request exécute sur GitHub Actions : `cargo fmt --check`, `cargo clippy`, `cargo test` et `cargo build --release`.
- Pour publier une version, poussez un tag `vX.Y.Z` : GitHub Actions construit le `.app` universel (Intel et Apple Silicon) et publie le zip dans une release.
