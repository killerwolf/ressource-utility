# Contribuer

Merci de votre intérêt ! Ce guide est court, et sert aussi d'aide-mémoire au mainteneur.

## Environnement

- macOS 12 ou plus (l'application ne fonctionne que sur macOS)
- Rust stable (`rustup`), avec `rustfmt` et `clippy`
- Outils en ligne de commande d'Apple (`xcode-select --install`) ; Xcode n'est pas nécessaire

```bash
git clone https://github.com/killerwolf/ressource-utility.git
cd ressource-utility
cargo run
```

## Structure

```
ressource-utility/
├── .github/workflows/   # CI, release et page web
├── macos/               # Info.plist et icône de l'application
├── scripts/bundle.sh    # construit « Ressource Utility.app » et le zip
├── site/                # page d'accueil (GitHub Pages)
└── src/                 # code Rust
```

## Commandes utiles

| Commande | Rôle |
| --- | --- |
| `cargo run -- --dump` | affiche un instantané des métriques et le bilan local |
| `cargo run -- --security` | audit de sécurité unique (processus et démarrage automatique) |
| `cargo run -- --bench 60` | mesure le temps CPU de la boucle d'échantillonnage |
| `cargo test` | tests unitaires |
| `cargo clippy --all-targets -- -D warnings` | analyse statique |
| `cargo fmt --check` | vérifie le format |
| `./scripts/bundle.sh` | construit l'application dans `target/bundle/` |

## Proposer un changement

1. Créez une branche depuis `main`.
2. Faites le changement, avec un test quand c'est testable.
3. Lancez `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` et `cargo test` : la CI exécute les mêmes.
4. Ajoutez une entrée sous `## [Unreleased]` dans `CHANGELOG.md` si le changement se voit côté utilisateur.
5. Ouvrez une pull request qui explique quoi et pourquoi.

## Publier une version

1. Déplacez les entrées de `## [Unreleased]` dans une nouvelle section datée.
2. Mettez à jour la version dans `Cargo.toml`.
3. Commit, puis fusion dans `main`.
4. Créez et poussez le tag : `git tag vX.Y.Z && git push origin vX.Y.Z`.
5. Le workflow de release construit l'application universelle et publie le zip dans une release GitHub.

## Code de conduite

Soyez courtois. Signalez tout problème via une issue.
