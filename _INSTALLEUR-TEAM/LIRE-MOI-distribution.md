# 📦 Distribution de l'installeur aux potes

## Le fichier à envoyer

**`Installateur-WoWsToolkit-Team.exe`** (~83 Mo) — c'est le **seul fichier** à envoyer.
Il embarque la version modifiée du toolkit (envoi auto Discord + filtre Clan Wars,
auto-update neutralisé). Envoie-le par **WeTransfer** ou **Discord** (si la taille passe).

## Ce que le pote fait

1. Télécharge `Installateur-WoWsToolkit-Team.exe`
2. **Double-clique** dessus
   - Windows SmartScreen peut afficher un avertissement (fichier non signé) :
     → *« Informations complémentaires » » « Exécuter quand même »*
3. L'installateur :
   - copie l'appli dans `%LOCALAPPDATA%\WoWs Toolkit`
   - crée un raccourci **« WoWs Toolkit (team) »** sur le bureau
4. Le pote lance le jeu via le **raccourci du bureau**

## Important à dire aux potes

- ⛔ **Si l'appli propose une mise à jour → la REFUSER.** L'auto-update est déjà
  neutralisé dans cette version, mais qu'ils ne réinstallent pas la version officielle
  par-dessus.
- 📤 Par défaut, **seules les parties Clan Wars** sont envoyées sur Discord. Réglage
  via le menu **« 📤 Discord »** en haut de la fenêtre, ou l'onglet Paramètres.
- 🔌 Le **lien webhook** du salon Discord est à coller dans Paramètres (envoie-le en privé).

## Le brief complet pour eux

→ [BRIEF-TEAM_envoi-auto-discord.md](BRIEF-TEAM_envoi-auto-discord.md) (mode d'emploi détaillé)

---

## Pour régénérer l'installeur (toi, Pascal)

Si tu modifies le toolkit et veux un nouvel installeur :

1. Recompiler : `cargo build --release --bin wows_toolkit` (dans `C:\Games\WoWs-Toolkit`)
2. Recompiler l'installeur (il ré-embarque l'exe à jour) :
   `cargo build --release` dans le projet `installer-rs`
3. Récupérer le `.exe` produit et le renvoyer aux potes

*(Le code source de l'installeur est dans le scratchpad de la session ; demande à Claude
de le replacer dans le repo si tu veux le garder en permanence.)*
