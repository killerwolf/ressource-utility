# Changelog

Toutes les évolutions notables du projet sont consignées ici.

Le format suit [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/) et le
projet respecte le [versionnage sémantique](https://semver.org/lang/fr/).

## [Unreleased]

## [0.1.0] - 2026-11-05

### Added

- Une icône dans la barre des menus affiche `CPU · RAM · disque libre` ; le menu détaille CPU, mémoire, disque et température, avec une mini-courbe de la dernière heure pour le CPU, la mémoire et la température.
- Un bilan en français donne un verdict par processus (✅ normal, 👀 à surveiller, 🛑 anormal, ❓ inconnu), grâce à une base locale de processus macOS, d'aides de navigateurs et d'apps Electron : aucun jeton consommé pour ce qui est connu.
- Une couche de sécurité locale repère les mineurs de cryptomonnaie connus, les noms système usurpés, les exécutables lancés depuis des emplacements suspects, les signatures douteuses et les éléments de démarrage automatique suspects. Le sous-menu Sécurité permet de les afficher dans le Finder ou de les forcer à quitter, après confirmation.
- Détection d'anomalies soutenues (CPU, mémoire, disque, température, processus gourmand) avec notification.
- Conseils par IA au choix : Anthropic, Gemini, OpenAI/Codex, OpenRouter (modèle gratuit) et LM Studio (100 % local). L'IA ne juge que les processus inconnus, sans appel périodique, avec un format de réponse strict et des verdicts mis en cache.
- Fenêtre Réglages native : fournisseur, modèle, clé API enregistrée dans le Trousseau macOS, adresse du serveur LM Studio, lancement au démarrage et bouton Tester.
- Le menu « Voir ce qui sera envoyé à l'IA » affiche le texte exact qui serait transmis.
