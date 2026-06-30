# 📤 WoWs Toolkit (version team) — Envoi auto Discord

Salut la team 👋
Voici le mode d'emploi de la version modifiée du **WoWs Toolkit** que je vous ai partagée.
En gros : **en fin de partie, l'appli poste automatiquement sur notre Discord** la vidéo
du replay (accélérée) + le tableau de stats de l'équipe. Plus besoin de rien faire à la main.

---

## ⚙️ Réglage le plus important : quelles parties sont envoyées

Par **défaut, SEULES les parties Clan Wars sont envoyées.** Vos randoms / co-op / brawls
ne partent **pas** sur le Discord — tranquille pour jouer en solo sans spammer le salon.

### Pour changer ça (2 endroits)

**1. Le plus rapide — le menu en haut de la fenêtre :**
En haut, à côté du bouton *Discord*, il y a un menu qui affiche l'état en clair :

- **📤 Discord : OFF** → rien n'est envoyé
- **📤 Discord : Clan Wars** → seules les Clan Wars partent *(réglage par défaut)*
- **📤 Discord : Toutes parties** → toutes vos parties partent

Cliquez dessus → vous pouvez :
- cocher/décocher **« Envoi automatique »** (le coupe complètement)
- choisir **« Clan Wars uniquement »** ou **« Toutes les parties »**

**2. Sinon, onglet *Paramètres* → section *Envoi automatique Discord*** : mêmes options
(case « Poster automatiquement » + choix Clan Wars / Toutes les parties + le lien webhook).

---

## 🔌 Mise en place (à faire une fois)

1. Ouvrir l'onglet **Paramètres → Envoi automatique Discord**
2. Coller le **lien webhook** du salon Discord dans *« Lien webhook Discord »*
   *(format `https://discord.com/api/webhooks/...` — je vous le file en privé)*
3. Choisir le **dossier des vidéos** (où elles sont enregistrées avant envoi)
4. Régler la **vitesse vidéo** (x20 = vidéo courte, x5 = plus longue)
5. Vérifier que **« Envoi automatique »** est coché

> 💡 La section affiche la taille du dossier vidéos et propose un bouton **« Vider le dossier »**
> dès que ça dépasse 1 Go — pensez-y de temps en temps.

---

## 🚫 IMPORTANT — refuser la mise à jour officielle

L'appli officielle se met à jour toute seule et **écrase notre version modifiée**
(l'envoi Discord disparaît alors). Donc :

- Au démarrage, si une mise à jour est proposée → **refusez-la**.
- Dans **Paramètres**, **décochez « Vérifier les mises à jour au démarrage »**.
- Si jamais vous perdez la fonction → c'est que la maj est passée, redemandez-moi le fichier.

---

## ❓ Récap express

| Je veux… | Je fais… |
|---|---|
| Ne rien envoyer (jouer peinard) | Menu haut → **📤 Discord : OFF** |
| Envoyer seulement les Clan Wars | **📤 Discord : Clan Wars** *(défaut)* |
| Tout envoyer | **📤 Discord : Toutes parties** |
| Mettre en place le salon | Paramètres → coller le **webhook** |
| Garder la modif | **Refuser** les mises à jour officielles |

Des questions → pingez-moi sur le Discord. GG 🚢
