# You are encouraged to suggest edits to this file on GitHub by either filing
# an issue, editing the file directly and creating a pull request, or just
# mentioning it in the Discord.
## meta
meta-language_name = Français

# -- Dialogue de sélection de langue --

## dialog
dialog-continue_in_english = Continuer en anglais
dialog-continue_in_language = Continuer en français
dialog-select_language = Choisir la langue
dialog-machine_translation_warning = La plupart des traductions ont été générées automatiquement et peuvent être inexactes.

# -- Titres des onglets --

## ui.tabs
ui-tabs-unpacker = Extracteur de ressources
ui-tabs-settings = Paramètres
ui-tabs-replay_parser = Inspecteur de replays
ui-tabs-player_tracker = Suivi des joueurs
ui-tabs-armor_viewer = Visualiseur de blindage
ui-tabs-model_viewer = Visualiseur de modèles
ui-tabs-stats = Statistiques
ui-tabs-directory_stats = Stats de { $dir }
ui-tabs-close_directory_stats = Fermer les stats du répertoire
ui-tabs-search = Recherche
ui-tabs-search_these_replays_unavailable = Ces replays n'ont pas encore été indexés
ui-tabs-zoom_in_panel = Agrandir
ui-tabs-zoom_out_panel = Réduire
ui-tabs-close_panel = Fermer
ui-tabs-close_tab = Fermer
ui-tabs-close_other_tabs = Fermer les autres
ui-tabs-close_tabs_to_the_right = Fermer les onglets à droite

# -- Boutons communs --

## ui.buttons
ui-buttons-yes = Oui
ui-buttons-no = Non
ui-buttons-send_replays = Envoyer les replays
ui-buttons-send_build_data = Envoyer uniquement les builds
ui-buttons-send_nothing = Ne rien envoyer
ui-buttons-switch_to_replays = Passer aux replays
ui-buttons-keep_current = Conserver le réglage actuel
ui-buttons-stop = Arrêter
ui-buttons-copy = Copier
ui-buttons-cancel = Annuler
ui-buttons-continue_ = Continuer
ui-buttons-install_update = Installer la mise à jour
ui-buttons-view_release = Voir la version
ui-buttons-clear_settings = Réinitialiser les paramètres
ui-buttons-more_options = Plus d'options
ui-buttons-create_issue = Créer un ticket
ui-buttons-discord = Discord
ui-buttons-github = GitHub
ui-buttons-view_github = Voir le projet sur GitHub
ui-buttons-choose = Choisir...
ui-buttons-reset = Réinitialiser
ui-buttons-clear = Effacer
ui-buttons-select_all = Tout sélectionner
ui-buttons-download = Télécharger
ui-buttons-retry = Réessayer
ui-buttons-refresh_persisted_data_confirm = Actualiser

# -- Éléments de menu --

## ui.menu
ui-menu-file = Fichier
ui-menu-check_updates = Vérifier les mises à jour
ui-menu-about = À propos
ui-menu-quit = Quitter

# -- Titres des fenêtres --

## ui.windows
ui-windows-build_consent = Consentement de collecte de builds
ui-windows-replay_migration = Passer au partage de replays
ui-windows-crash_detected = Plantage de l'application détecté
ui-windows-update_available = Mise à jour disponible
ui-windows-network_warning = Avertissement réseau
ui-windows-download_game_data = Télécharger les données de jeu
ui-windows-error = Erreur
ui-windows-software_encode = Aucun encodeur vidéo GPU
ui-windows-about = À propos
ui-windows-refresh_persisted_data = Actualiser les données de replay enregistrées
ui-windows-tactics_board = Tableau tactique
ui-windows-minimize = Réduire
ui-windows-maximize = Agrandir
ui-windows-restore = Restaurer
ui-windows-close = Fermer

# -- Texte des dialogues / messages --

## ui.dialogs
ui-dialogs-build_consent_message = Souhaitez-vous envoyer les informations de builds des joueurs issues des batailles classées et aléatoires au développeur ? Cette collecte de données aide la communauté à analyser en masse les builds des joueurs. Vous pouvez vous désinscrire à tout moment dans les paramètres.
ui-dialogs-replay_migration_message = Toolkit peut désormais partager des replays complets, ce qui permet une analyse du matchmaking bien plus riche que les seules données de build. Les batailles où vous jouez un navire de test ne sont pas partagées. Souhaitez-vous passer au partage des replays bruts ?
ui-dialogs-crash_message = Il semble que WoWs Toolkit ait planté lors de sa dernière exécution. Si vous souhaitez signaler ce problème, veuillez créer un ticket sur GitHub ou rejoindre le serveur Discord et fournir les informations ci-dessous.
ui-dialogs-crash_clear_settings = Si des données persistantes de l'application pourraient être responsables, vous pouvez essayer de réinitialiser les paramètres en appuyant sur le bouton ci-dessous. Cela effacera tous les paramètres. Les joueurs suivis sont conservés. Vos replays et données WoWs seront préservés.
ui-dialogs-update_message = La version { $tag } de WoWs Toolkit est disponible
ui-dialogs-update_notes = Notes de version
ui-dialogs-update_asset_pending = Le téléchargement Windows n'est pas encore disponible. Consultez la version publiée pour connaître l'état de la compilation.
ui-dialogs-update_windows_only = Mise à jour disponible, mais seul Windows est pris en charge pour le moment.
ui-dialogs-p2p_warning = Les sessions collaboratives utilisent un réseau pair-à-pair. Les autres utilisateurs de la session peuvent voir votre adresse IP.
ui-dialogs-software_encode_message = Aucun encodeur vidéo GPU compatible n'a été trouvé, cet export sera donc encodé de manière logicielle. C'est beaucoup plus lent, et une longue bataille peut prendre plusieurs minutes.
ui-dialogs-download_game_data_intro = Certains replays nécessitent des données de jeu qui ne sont pas installées. Choisissez les builds à télécharger.
ui-dialogs-download_build_row = Version { $version } (build { $build })
ui-dialogs-download_replays_needing = requis par { $count ->
    [one] { $count } replay
   *[other] { $count } replays
}
ui-dialogs-download_availability_exact = publié
ui-dialogs-download_availability_nearest = aucune correspondance exacte, la plus proche est { $version } (build { $build })
ui-dialogs-download_availability_unpublished = jamais publié
ui-dialogs-download_availability_unreachable = publié, mais sa liste de fichiers n'a pas pu être lue
ui-dialogs-download_plan_pending = Détermination des éléments à récupérer...
ui-dialogs-download_objects_to_fetch = Objets à récupérer pour les builds cochés : { $count }
ui-dialogs-download_plan_failed = Impossible de déterminer les éléments à récupérer
ui-dialogs-download_plan_no_cache_dir = Aucun répertoire de cache des données de jeu n'est défini. Choisissez-en un dans les Paramètres, puis réessayez.
ui-dialogs-refresh_persisted_data_message = Analyse à nouveau chaque replay de votre répertoire de replays et réécrit ses données enregistrées. Les entrées enregistrées avec une correspondance de données de replay qui ne correspond pas à leur version du jeu sont laissées telles quelles, sauf le Personal Rating et le mode de jeu, recalculés pour chaque replay à partir des valeurs attendues actuelles. Cela peut prendre un certain temps.
ui-dialogs-file_dialog_failed = Impossible d'ouvrir la boîte de dialogue de fichiers : { $reason }

# -- Libellés --

## ui.labels
ui-labels-error_occurred = Une erreur est survenue :
ui-labels-made_by = Créé par landaire.
ui-labels-credits = Merci à Trackpad, TTaro, lkolbly pour leurs contributions.
ui-labels-pr_credits = Les données et la formule de calcul de la cote personnelle (PR) sont fournies par WoWs Numbers.
ui-labels-suppress_warning = Ne plus afficher ce message
ui-labels-more_info = Plus d'informations
ui-labels-app_version = Version { $version }
ui-labels-project_license = WoWs Toolkit est distribué sous la licence { $license }.
ui-labels-third_party_notices = Mentions de tiers
ui-labels-notices_search_hint = Rechercher dans les mentions
ui-labels-no_matching_notices = Aucune mention ne correspond à la recherche.

# -- Messages toast --

## ui.messages
ui-messages-bug_report_info_copied = Les informations du rapport de bug ont été copiées.
ui-messages-update_downloading = Téléchargement de la mise à jour : { $read } sur { $total }
ui-messages-update_downloading_unknown = Téléchargement de la mise à jour : { $read } pour l'instant
ui-messages-app_up_to_date = L'application est à jour
ui-messages-checking_app_updates = Recherche de mises à jour de l'application...
ui-messages-update_starting = Téléchargement de la mise à jour...
ui-messages-update_check_failed = Échec de la vérification des mises à jour
ui-messages-game_data_loaded = Données de jeu chargées avec succès
ui-messages-no_replays_detected = Aucun replay détecté. Votre répertoire WoWs est-il correctement configuré ?
ui-messages-game_data_builds_downloaded = Données de jeu téléchargées pour { $count ->
    [one] { $count } version
   *[other] { $count } versions
}
ui-messages-game_data_download_failed = Impossible de télécharger les données de jeu
ui-messages-game_data_up_to_date = Les données de jeu sont à jour
ui-messages-game_data_updates_available = { $count ->
    [one] { $count } build en cache dispose de données plus récentes
   *[other] { $count } builds en cache disposent de données plus récentes
}
ui-messages-game_data_cache_valid = Le cache des données de jeu est valide et correspond au dépôt distant
ui-messages-game_data_cache_invalid = { $count ->
    [one] { $count } build en cache est obsolète ou corrompu
   *[other] { $count } builds en cache sont obsolètes ou corrompus
}
ui-messages-replay_load_failed = Échec du chargement du replay
ui-messages-twitch_token_invalid = Le jeton Twitch est invalide ou expiré. Veuillez le mettre à jour dans les Paramètres.
ui-messages-wows_dir_invalid = Le répertoire World of Warships n'est pas valide. Vérifiez le chemin dans les Paramètres.
ui-messages-wows_dir_not_set = Le répertoire de World of Warships n'est pas défini
ui-messages-constants_version_mismatch =
    La version du fichier de correspondance des replays ne correspond pas à la version du jeu.
    Les résultats d'après-bataille peuvent être inexacts. Veuillez patienter pendant que les mainteneurs du projet mettent à jour le fichier sur le serveur.
ui-messages-session_started = Session démarrée
ui-messages-user_joined = { $name } a rejoint
ui-messages-user_timeout = { $name } a expiré
ui-messages-user_left = { $name } a quitté
ui-messages-session_ended = Session terminée
ui-messages-session_error = Erreur de session : { $msg }
ui-messages-connected_to_session = Connecté à la session
ui-messages-host_opened_replay = L'hôte a ouvert un replay : { $name }
ui-messages-host_closed_replay = L'hôte a fermé un replay
ui-messages-session_rejected = Session refusée : { $reason }
ui-messages-drop_not_a_replay = Ce n'est pas un fichier replay
ui-messages-drop_not_replays = Ce ne sont pas des fichiers de replay
ui-messages-drop_skipped = { $count ->
    [one] { $count } fichier déposé ignoré : ni replay ni dossier
   *[other] { $count } fichiers déposés ignorés : ni replays ni dossiers
}
ui-messages-log_copied = { $bytes } Ko copiés de { $file } vers le presse-papiers
ui-messages-log_copy_failed = Impossible de lire un fichier journal : { $error }
ui-messages-log_not_found = Aucun fichier journal trouvé dans { $dir }

# -- Noms des statistiques --

## stat
stat-damage = Dégâts
stat-spotting_damage = Dégâts de repérage
stat-frags = Éliminations
stat-raw_xp = XP brut
stat-base_xp = XP de base
stat-win_rate = Taux de victoire
stat-personal_rating = Cote personnelle
stat-match_contribution = Contribution au combat
stat-capture_points = Points de capture
stat-planes_shot_down = Avions abattus
stat-anti_air_damage = Dégâts antiaériens
stat-survival_rate = Taux de survie
stat-torpedoes_spotted = Torpilles repérées
stat-avg_pr = PR moy.
stat-avg_prefix = { $name } moy.

## chart
chart-combined = Combiné
chart-combined_suffix = (Combiné)
chart-rolling_average_suffix = (Moyenne glissante)

# -- Dialogues de confirmation --

## confirm
confirm-open_in_game = Cela va lancer World of Warships. Continuer ?
confirm-clear_all_session_stats = Cela va effacer toutes les statistiques de session. Continuer ?
confirm-set_as_session_stats = Cela va remplacer vos statistiques de session actuelles. Continuer ?

# -- Messages d'erreur --

## error
error-file_watcher_creation = Échec de la création du surveillant de fichiers : { $error }
error-replay_dir_watch = Échec de la surveillance du répertoire de replays : { $error }

# -- Paramètres --

## ui.settings.app
ui-settings-app-check_for_updates = Vérifier les mises à jour au démarrage
ui-settings-app-enable_logging = Activer la journalisation (prend effet au redémarrage)
ui-settings-app-enable_logging_tooltip = Écrire les journaux de débogage dans un fichier du répertoire de données de l'application. Utile pour le dépannage.
ui-settings-app-data_sharing_mode = Partage de données (batailles classées et aléatoires)
ui-settings-app-theme = Thème
ui-settings-app-theme_system = Système
ui-settings-app-theme_dark = Sombre
ui-settings-app-theme_light = Clair
ui-settings-app-code_integrity = Bloquer le code non signé (prend effet au redémarrage)
ui-settings-app-code_integrity_tooltip = Refuse de charger du code qui n'a pas été signé par Microsoft, ce qui tient les surcouches, les outils de capture et les utilitaires audio à l'écart de cette application. Ils sont une cause fréquente de plantages et de blocages pendant le rendu. Les pilotes graphiques sont signés via Windows et continuent de fonctionner. Ce réglage ne peut plus être désactivé une fois la session démarrée, un changement s'applique donc au prochain lancement. Automatique le passe si une méthode de saisie tierce est installée, car elle serait sinon bloquée. Notez que les autres protections restent actives quel que soit ce réglage ; lancez l'application avec --no-hardening pour toutes les désactiver.
ui-settings-app-code_integrity_automatic = Automatique
ui-settings-app-code_integrity_always = Toujours
ui-settings-app-code_integrity_never = Jamais
ui-settings-app-language = Langue
ui-settings-app-open_data_dir_tooltip = Ouvrir le dossier de données de l'application (paramètres, journaux, données en cache) dans votre gestionnaire de fichiers.
ui-settings-app-proxy_url = Proxy HTTP
ui-settings-app-proxy_url_hint_live = hôte:port, ou laisser vide pour utiliser les paramètres proxy du système

## ui.settings.wows
ui-settings-wows-description = Chemin vers votre installation de World of Warships
ui-settings-wows-directory_hint = Répertoire de World of Warships
ui-settings-wows-browse = Parcourir...

## ui.settings.wows.cache
ui-settings-wows-cache-heading = Cache des données de jeu
ui-settings-wows-cache-description = Données de jeu extraites, pour que les replays des anciennes versions s'ouvrent toujours
ui-settings-wows-cache-auto_dump = Mettre en cache les données du jeu pour la compatibilité des anciens replays
ui-settings-wows-cache-browse = Parcourir
ui-settings-wows-cache-open_folder = Ouvrir le dossier
ui-settings-wows-cache-delete_old = Supprimer les anciennes versions
ui-settings-wows-cache-stats = Cache des données : { $size } dans { $count ->
    [one] { $count } version
   *[other] { $count } versions
}
ui-settings-wows-cache-check_updates = Vérifier les mises à jour
ui-settings-wows-cache-actions = Actions du cache
ui-settings-wows-cache-actions_tooltip = Rechercher des mises à jour ou vérifier les versions en cache
ui-settings-wows-cache-update_all = Tout mettre à jour
ui-settings-wows-cache-updates_available = { $count ->
    [one] { $count } build en cache dispose de données plus récentes
   *[other] { $count } builds en cache disposent de données plus récentes
}
ui-settings-wows-cache-validate = Valider le cache
ui-settings-wows-cache-repair = Réparer
ui-settings-wows-cache-repair_needed = { $count ->
    [one] { $count } build en cache est obsolète ou corrompu
   *[other] { $count } builds en cache sont obsolètes ou corrompus
}

## ui.settings.replay
ui-settings-replay-show_raw_xp = Afficher le XP brut
ui-settings-replay-show_entity_id = Afficher l'ID d'entité
ui-settings-replay-show_observed_damage = Afficher les dégâts observés
ui-settings-replay-enable_previews = Afficher l'aperçu de la minicarte au survol
ui-settings-replay-enable_previews_tooltip = Survoler un replay le lit et le rejoue en arrière-plan pour construire un aperçu animé de la minicarte. Désactivez cette option sur un stockage lent.
ui-settings-replay-auto_export_data = Export automatique des données
ui-settings-replay-export_path_hint = Répertoire d'export automatique
ui-settings-replay-export_path_missing = Le répertoire n'existe pas. Aucune donnée ne peut y être exportée.
ui-settings-replay-index_all_replays = Indexer tous les replays
ui-settings-replay-show_heals = Afficher les soins
ui-settings-replay-current_path = Chemin actuel des replays

## ui.settings.session
ui-settings-session-display_name = Nom d'affichage
ui-settings-session-suppress_ip_warning = Ne plus afficher l'avertissement d'adresse IP P2P
ui-settings-session-ip_warning_tooltip = Les sessions collaboratives utilisent un réseau pair-à-pair. Les autres utilisateurs de la session peuvent voir votre adresse IP.
ui-settings-session-disable_auto_open = Désactiver l'ouverture automatique des fenêtres de session
ui-settings-session-auto_open_tooltip = Lorsque activé, les fenêtres de session (replays, tableaux tactiques) ne s'ouvriront pas automatiquement. Vous pouvez toujours les ouvrir depuis la fenêtre contextuelle de session.

## ui.settings.twitch
ui-settings-twitch-get_token = Obtenir un jeton de connexion
ui-settings-twitch-get_token_tooltip = Nous utilisons la page de connexion de Chatterino car elle fournit un jeton avec les permissions nécessaires (essentiellement un jeton modérateur avec permissions de chat), et cela évite au développeur de WoWs Toolkit d'héberger sa propre page de connexion qui aurait le même résultat.
ui-settings-twitch-paste_token_no_token = Coller le jeton (Aucun jeton défini)
ui-settings-twitch-paste_token_tooltip = Lit l'identifiant depuis le presse-papiers
ui-settings-twitch-paste_token_valid = Coller le jeton (Jeton valide)
ui-settings-twitch-paste_token_unvalidated = Vérification du jeton
ui-settings-twitch-paste_token_invalid = Coller le jeton (Jeton invalide / Expiré)
ui-settings-twitch-monitored_channel = Chaîne surveillée (Par défaut : soi-même)
ui-settings-twitch-signed_in_as = Connecté en tant que { $who }

# -- Inspecteur de replays --

## ui.replay
ui-replay-copy_bug_report_info = Copier les informations du rapport de bug
ui-replay-match_age = il y a { $age }
ui-replay-listing_caption = Replays
ui-replay-loading_game_data_version = Chargement des données de jeu pour { $version }...
ui-replay-spectator = Spectateur
ui-replay-loading = Chargement...
ui-replay-preview_loading = Construction de l'aperçu...
ui-replay-preview_no_game_data = Aucune donnée de jeu pour la version client { $value }
ui-replay-preview_unreadable_because = Ce replay n'a pas pu être lu: { $value }
ui-replay-preview_unwalkable = Impossible de parcourir la bataille de ce replay : { $value }
ui-replay-preview_unknown_version = Ce replay indique une version client illisible : { $value }
ui-replay-preview_no_map = Aucune donnée de carte pour { $value }
ui-replay-preview_render_failed = L'aperçu n'a pas pu être affiché : { $value }
ui-replay-incomplete_results = Résultats de match incomplets
ui-replay-incomplete_results_tooltip = Le replay ne contient pas encore les résultats de fin de match. Les données seront automatiquement rechargées lorsque le match se terminera et que les résultats seront ajoutés au replay.
ui-replay-no_chat = Aucun message de chat n'a été envoyé dans ce replay
ui-replay-chat_copy_all = Tout copier
ui-replay-chat_save_to_file = Enregistrer dans un fichier
ui-replay-chat_save_title = Enregistrer le chat de la partie
ui-replay-export_results_title = Exporter les résultats
ui-replay-results_exported = Résultats exportés
ui-replay-chat_saved = Chat enregistré dans { $path }
ui-replay-chat_save_failed = Le chat n'a pas été enregistré : { $error }
ui-replay-team_damage = Dégâts d'équipe :{" "}
ui-replay-team_share = { $pct } du total de l'équipe ({ $team })
ui-replay-hide_my_stats = Masquer les stats de mes navires de test
ui-replay-open_manually = Ouvrir un fichier replay manuellement...
ui-replay-open_directory = Ouvrir un répertoire de replays...
ui-replay-search_these_replays = Rechercher dans ces replays
ui-replay-import_constants = Importer un fichier constants.json...
ui-replay-refresh_persisted_data = Actualiser les données de replay enregistrées...
ui-replay-contributing = Contribution de { $done } sur { $total ->
    [one] { $total } replay
   *[other] { $total } replays
}
ui-replay-contributed = { $sent } sur { $total ->
    [one] { $total } replay envoyé
   *[other] { $total } replays envoyés
}
ui-replay-contribute_nothing_shared = Choisissez d'abord ce que vous partagez dans les Paramètres
ui-replay-contribute_none_listed = Aucun replay listé à envoyer
ui-replay-contribute_not_ready = Les replays pourront être envoyés une fois les données du jeu et les paramètres chargés
ui-replay-contribute_already_running = Un envoi est déjà en cours
ui-replay-auto_export_failed = Une bataille n'a pas pu être exportée : { $error }
ui-replay-auto_exported = Bataille exportée vers { $file }
ui-replay-opened_new_battle = Nouvelle bataille ouverte : { $battle }
ui-replay-new_battle_listed = Nouvelle bataille : { $battle }
ui-replay-results_arrived = Les résultats de la bataille { $battle } sont arrivés
ui-replay-constants_written = { $count ->
    [one] Correspondance des résultats téléchargée pour { $count } version
   *[other] Correspondances des résultats téléchargées pour { $count } versions
}
ui-replay-constants_import_title = Importer un constants.json
ui-replay-constants_imported = La correspondance des résultats pour la version { $build } a été importée
ui-replay-constants_import_failed = Cette correspondance n'a pas été importée : { $reason }
ui-replay-constants_no_build = Chargez d'abord une version du jeu
ui-replay-constants_latest_written = La correspondance des résultats la plus récente a été téléchargée
ui-replay-constants_check_failed = Les correspondances des résultats n'ont pas pu être vérifiées : { $reason }
ui-replay-constants_build_label = build { $build }
ui-replay-constants_generating = Mise à jour des constantes du jeu pour { $version }...
ui-replay-constants_generated = Constantes du jeu pour { $version } générées en { $seconds } s
ui-replay-constants_generation_failed = Impossible de mettre à jour les constantes du jeu pour { $version } : { $reason }. { $fallback }
ui-replay-constants_fallback_kept = Les constantes déjà enregistrées sont conservées.
ui-replay-constants_fallback_published = Utilisation des constantes publiées pour le build { $build }.
ui-replay-constants_fallback_embedded = Utilisation des constantes intégrées.
ui-replay-copy_latest_log = Copier le dernier journal dans le presse-papiers
ui-replay-load_alt_perspective = Charger la perspective de l'autre équipe...
ui-replay-load_alt_perspective_failed = Échec de la fusion du replay : { $error }
ui-replay-load_alt_perspective_limit = Cette bataille est déjà lue à travers { $limit ->
    [one] { $limit } autre enregistrement
   *[other] { $limit } autres enregistrements
}
ui-replay-consumable_charges_remaining = { $remaining } restante(s) sur { $total ->
    [one] { $total } charge
   *[other] { $total } charges
}
ui-replay-consumable_charges_unlimited = { $used } utilisées (charges illimitées)
ui-replay-consumable_header_remaining = Restantes
ui-replay-consumable_header_total = Total
ui-replay-consumable_charges_infinite = ∞
ui-replay-autoload_latest = Charger automatiquement le dernier replay
ui-replay-autoload_latest_tooltip = Ouvrir une partie dès que le jeu a fini de l'écrire
ui-replay-anonymize_names = Anonymiser les noms des joueurs
ui-replay-anonymize_names_tooltip = Afficher des noms générés à la place des noms de joueurs et des tags de clan. Les exports conservent les vrais noms.
ui-replay-group_win_rate_all_modes = Compter tous les modes de jeu dans le taux de victoire des groupes
ui-replay-group_win_rate_all_modes_tooltip = Par défaut, les en-têtes de groupe ne comptent que les batailles aléatoires et classées. Une fois activé, tous les modes de jeu sont comptés et le taux de victoire n'est pas coloré, car les couleurs sont calibrées pour les batailles aléatoires.
ui-replay-select_replay = Sélectionnez un replay
ui-replay-drop_hint = Déposez un replay ici pour l'ouvrir, ou un dossier pour lister ses replays
ui-replay-scanning = Analyse des replays...
ui-replay-scanning_count = Lecture du replay { $done } sur { $total }
ui-replay-no_replays_in = Aucun replay dans { $folder }
ui-replay-drop_folder_hint = Déposez un dossier sur la fenêtre pour le lister dans son propre onglet
ui-replay-new_tag = Nouveau
ui-replay-loading_game_data = Chargement des données de jeu...
ui-replay-game_data_failed = Échec du chargement des données de jeu : { $error }
ui-replay-group_label = Regroupement des replays
ui-replay-column_filters = Filtres de colonnes
ui-replay-row_elements = Éléments de ligne
ui-replay-row_element_personal_rating = Cote personnelle
ui-replay-row_element_match_contribution = Contribution au combat
ui-replay-row_element_division = Division
ui-replay-row_element_twitch = Chat Twitch
ui-replay-row_element_damage = Dégâts
ui-replay-row_element_kills = Destructions
ui-replay-row_element_time = Heure de la bataille
ui-replay-listing_options = Options d'affichage
ui-replay-open = Ouvrir
ui-replay-open_file = Ouvrir un fichier replay...
ui-replay-check_game_data = Vérifier les données de jeu
ui-replay-check_game_data_tooltip = Vérifier les données de jeu requises par les replays de ce répertoire
ui-replay-close_side_panel = Fermer le panneau
ui-replay-nda = NDA
ui-replay-chat = Chat
ui-replay-game_chat = Chat en jeu
ui-replay-save_to_file = Enregistrer dans un fichier
ui-replay-chat_copied = Chat copié
ui-replay-copy_message = Copier le message
ui-replay-actions = Actions
ui-replay-row_actions_hint = Actions du joueur (ou clic droit sur la ligne)
ui-replay-section_match = Match
ui-replay-section_export = Export
ui-replay-section_debug = Débogage
ui-replay-timeline = Chronologie du match
ui-replay-row_not_indexed = non indexé
ui-replay-hover_survived = Survécu
ui-replay-hover_sunk = Coulé
ui-replay-hover_not_indexed = Non indexé
ui-replay-row_kills = { $count ->
    [one] { $count } élimination
   *[other] { $count } éliminations
}
ui-replay-row_survived = en vie
ui-replay-row_sunk = coulé
ui-replay-row_division = Division : { $members }
ui-replay-export_results_json = Résultats : JSON
ui-replay-export_results_cbor = Résultats : CBOR
ui-replay-export_results_csv = Résultats : CSV
ui-replay-timeline_filter_all = Tous
ui-replay-timeline_filter_none = Aucun
ui-replay-timeline_search_hint = Rechercher un navire ou un joueur
ui-replay-timeline_no_events = Aucun événement significatif détecté.
ui-replay-timeline_no_matches = Aucun événement ne correspond au filtre actuel.
ui-replay-timeline_parsing = Analyse des événements...
ui-replay-timeline_failed = Échec de l'extraction de la chronologie du match : { $error }
ui-replay-timeline_retry = Réessayer
ui-replay-timeline_export_tooltip = S'applique aux événements actuellement visibles après filtrage.
ui-replay-timeline_copied = Chronologie copiée
ui-replay-timeline_event_copied = Événement copié
ui-replay-timeline_saved = Chronologie enregistrée dans { $path }
ui-replay-timeline_save_failed = La chronologie n'a pas été enregistrée : { $error }
ui-replay-timeline_kind_health_lost = Dégâts importants
ui-replay-timeline_kind_death = Détruit
ui-replay-timeline_kind_cap_contested = Point contesté
ui-replay-timeline_kind_cap_flipped = Point capturé
ui-replay-timeline_kind_cap_being_captured = Point en cours de capture
ui-replay-timeline_kind_radar = Radar utilisé
ui-replay-timeline_kind_advantage = Changement d'avantage
ui-replay-timeline_kind_disconnected = Déconnecté

## ui.replay.context
ui-replay-context-open_in_new_tab = Ouvrir dans un nouvel onglet
ui-replay-context-copy_cell = Copier la cellule
ui-replay-context-copy_row = Copier la ligne
ui-replay-context-copy_table = Copier le tableau
ui-replay-context-pin_column = Épingler la colonne
ui-replay-context-unpin_column = Désépingler la colonne
ui-replay-context-copied = Copié
ui-replay-context-copy_path = Copier le chemin
ui-replay-context-copy_replays = { $count ->
    [one] Copier le replay
   *[other] Copier { $count } replays
}
ui-replay-context-copy_paths = { $count ->
    [one] Copier le chemin
   *[other] Copier { $count } chemins
}
ui-replay-context-show_in_explorer = Afficher dans l'explorateur de fichiers
ui-replay-context-show_replay_controls = Afficher les contrôles de replay
ui-replay-context-open_in_game = Ouvrir dans le jeu
ui-replay-context-render_replay = Générer le rendu du replay
ui-replay-render = { ui-replay-context-render_replay }
ui-replay-context-set_session_stats = Définir comme stats de session ({ $count ->
    [one] { $count } replay
   *[other] { $count } replays
})
ui-replay-context-add_session_stats = Ajouter aux stats de session ({ $count ->
    [one] { $count } replay
   *[other] { $count } replays
})
ui-replay-context-render_to_video = { $count ->
    [one] Exporter en vidéo
   *[other] Exporter { $count } replays en vidéo
}
ui-replay-context-render_to_clipboard = { $count ->
    [one] Exporter dans le presse-papiers
   *[other] Exporter { $count } replays dans le presse-papiers
}

## ui.replay.column
ui-replay-column-player_name = Nom du joueur
ui-replay-column-base_xp = XP de base
ui-replay-column-ship_name = Nom du navire
ui-replay-column-hits = Coups
ui-replay-column-heals = Soins
ui-replay-column-heals_tooltip = Nombre d'activations de l'Équipe de réparation observées pour ce joueur. Peut être inexact pour les navires non affichés à l'écran (c.-à-d. les navires ennemis jamais repérés).
ui-replay-column-heals_no_repair_tooltip = Ce navire ne dispose pas du consommable Équipe de réparation.
ui-replay-column-kills = Éliminations
ui-replay-column-observed_damage = Dégâts observés
ui-replay-column-actual_damage = Dégâts réels
ui-replay-column-potential_damage = Dégâts potentiels
ui-replay-column-time_lived = Temps de survie
ui-replay-column-fires = Incendies
ui-replay-column-floods = Inondations
ui-replay-column-citadels = Citadelles
ui-replay-column-crits = Critiques
ui-replay-column-received_damage = Dégâts reçus
ui-replay-column-distance_traveled = Distance parcourue
ui-replay-column-skills = Compétences
ui-replay-column-personal_rating = PR
ui-replay-column-match_contribution = MC

## ui.rating
ui-rating-personal_rating = Cote personnelle
ui-rating-match_contribution = Contribution au combat
ui-rating-versus_average = par rapport à la moyenne
ui-rating-damage = Dégâts
ui-rating-kills = Destructions
ui-rating-wins = Victoire
ui-rating-team_damage_share = Part des dégâts de l'équipe
ui-rating-tanking = Tirs encaissés
ui-rating-spotting = Repérage
ui-rating-objectives = Objectifs
ui-rating-anti_air = Antiaérien
ui-rating-torpedo_warnings = Alertes torpilles
ui-rating-critical_bursts = Rafales critiques
ui-rating-unrated_early_exit = Pas de MC : l'enregistrement s'est terminé avant la bataille
ui-rating-mc_unrated_battle_type = Pas de MC : la contribution au combat ne s'applique qu'aux batailles aléatoires et classées

## ui.rating.detail
ui-rating-detail-versus_expected = { $actual } pour { $expected } attendu
ui-rating-detail-wins = { $actual } gagnée(s) pour { $expected } attendu
ui-rating-detail-weighted_damage = { $actual } pour { $expected } attendu, dégâts aux destroyers et sous-marins x1,2
ui-rating-detail-team_damage_share = { $share } % des dégâts de l'équipe aux ennemis, une part égale est de { $even } %
ui-rating-detail-tanking = { $potential } dégâts potentiels, { $class }x la moyenne de la classe, { $team }x une part égale de ceux de l'équipe
ui-rating-detail-spotting = { $damage } dégâts sur repérage (moyenne de la classe { $baseline }), { $ships } PV de navires repérés (moyenne { $ships_baseline })
ui-rating-detail-objectives = Points : { $capture } en capture, { $block } en blocage, { $reset } en réinitialisation, { $battle_end } tenus à la fin, { $hold } en maintien (comptés pour un quart)
ui-rating-detail-anti_air_planes = { $strike ->
    [one] { $strike } avion d'attaque
   *[other] { $strike } avions d'attaque
}, { $support ->
    [one] { $support } chasseur ou éclaireur
   *[other] { $support } chasseurs et éclaireurs
}
ui-rating-detail-anti_air_damage = { $damage } dégâts AA
ui-rating-detail-torpedo_warnings = { $threatening ->
    [one] { $threatening } torpille menaçante repérée
   *[other] { $threatening } torpilles menaçantes repérées
}
ui-rating-detail-critical_bursts = { $count ->
    [one] { $count } rafale critique
   *[other] { $count } rafales critiques
} : { $bursts }. Visibles uniquement pour les navires proches du joueur enregistré
ui-rating-detail-critical_burst = { $share } % de { $ship } en { $seconds } s
ui-rating-detail-critical_burst_at_once = { $share } % de { $ship } d'un coup

## ui.personal_rating
ui-personal_rating-unrated_hover = La cote personnelle ne s'applique qu'aux batailles aléatoires et classées.

## ui.rating.basis
ui-rating-basis-static = Statique
ui-rating-basis-dynamic = Dynamique
ui-rating-basis-static_explained = Statique : mesuré par rapport à une constante fixe ou à une référence de classe
ui-rating-basis-dynamic_explained = Dynamique : mesuré par rapport aux valeurs attendues de wows-numbers ou aux autres joueurs de cette bataille

## ui.replay.group
ui-replay-group-date = Date
ui-replay-group-ship = Navire
ui-replay-group-none = Aucun

## ui.replay.player
ui-replay-player-hidden_profile = Le joueur a un profil masqué
ui-replay-player-never_connected = Le joueur ne s'est jamais connecté au match

## ui.replay.build
ui-replay-build-open_shiptool = Ouvrir le build dans ShipTool
ui-replay-build-open_in_ship_builder = Ouvrir le build dans WoWs SB
ui-replay-build-copy_link = Copier le lien du build
ui-replay-build-copy_short_link = Copier le lien court du build
ui-replay-build-link_copied = Lien du build copié
ui-replay-build-open_wows_numbers = Ouvrir la page WoWs Numbers
ui-replay-build-not_spotted = Ce navire n'a jamais été repéré. Informations de build indisponibles.

## ui.replay.sections
ui-replay-sections-achievements = Succès
ui-replay-sections-ribbons = Rubans
ui-replay-sections-modules = Modules :
ui-replay-sections-modules_none = Aucun module
ui-replay-sections-signals = Pavillons :
ui-replay-sections-loadout = Configuration :
ui-replay-sections-loadout_none = Aucune configuration
ui-replay-sections-captain_skills = Compétences du commandant :
ui-replay-sections-captain_skills_none = Aucune compétence de commandant
ui-replay-sections-consumables = Consommables :
ui-replay-sections-fire_chance = Chance d'incendie effective
ui-replay-sections-fire_chance_expected = attendu
ui-replay-sections-fire_chance_fires = { $fires ->
    [one] { $fires } incendie
   *[other] { $fires } incendies
}
ui-replay-sections-fire_chance_eligible_hits = { $hits ->
    [one] { $hits } coup ayant pu en déclencher un
   *[other] { $hits } coups ayant pu en déclencher un
}
ui-replay-sections-fire_chance_ships = sur { $ships ->
    [one] { $ships } navire ciblé
   *[other] { $ships } navires ciblés
}
ui-replay-sections-fire_chance_expected_fires = { $fires } incendies attendus
ui-replay-sections-fire_chance_per_ship = Par navire ciblé
ui-replay-sections-fire_chance_no_eligible_hits = aucun coup n'aurait pu déclencher un incendie
ui-replay-sections-fire_chance_formula = Formule de chance d'incendie de l'attaquant
ui-replay-sections-fire_chance_formula_base = burnProb de base
ui-replay-sections-fire_chance_formula_clamped = borné
ui-replay-sections-fire_chance_breakdown = Détail
ui-replay-sections-fire_chance_shells_fired = { $count ->
    [one] obus HE tiré
   *[other] obus HE tirés
}
ui-replay-sections-fire_chance_he_hits = { $count ->
    [one] coup HE sur un navire
   *[other] coups HE sur un navire
}
ui-replay-sections-fire_chance_no_target_ship = { $count ->
    [one] coup HE non attribuable à un navire ciblé
   *[other] coups HE non attribuables à un navire ciblé
}
ui-replay-sections-fire_chance_eligible = auraient pu déclencher un incendie
ui-replay-sections-fire_chance_ribbons = { $count ->
    [one] ruban SetFire
   *[other] rubans SetFire
}
ui-replay-sections-fire_chance_ribbons_credited = attribués à un obus
ui-replay-sections-fire_chance_ribbons_uncredited = non attribués
ui-replay-sections-fire_chance_unattributed_no_burn_state = aucun changement d'état d'incendie auquel le rattacher
ui-replay-sections-fire_chance_unattributed_already_credited = un incendie antérieur a déjà utilisé le coup proche
ui-replay-sections-fire_chance_unattributed_secondary_contest = un de nos propres secondaires aurait tout aussi bien pu le déclencher
ui-replay-sections-fire_chance_unattributed_hit_contest = chacun de nos coups proches était ambigu avec un autre
ui-replay-sections-fire_chance_unattributed_all_excluded = chacun de nos coups proches était déjà exclu
ui-replay-sections-fire_chance_unattributed_none_could_burn = aucun de nos coups proches n'aurait pu déclencher un incendie
ui-replay-sections-fire_chance_unattributed_no_hit = aucun de nos coups n'a touché dans la fenêtre
ui-replay-sections-fire_chance_not_applicable = victime déjà coulée, non applicable
ui-replay-sections-fire_chance_copied = Détail de la chance d'incendie copié
ui-replay-sections-fire_chance_click_to_copy = Cliquez pour copier ce détail
ui-replay-sections-fire_chance_exclusion_already_burning = section déjà en feu
ui-replay-sections-fire_chance_exclusion_victim_build_unknown = build de la victime inconnu, les zones d'incendie peuvent être fusionnées
ui-replay-sections-fire_chance_exclusion_damage_control_active = Équipe de réparation d'urgence active
ui-replay-sections-fire_chance_exclusion_damage_control_unknown = état de la réparation d'urgence inconnu
ui-replay-sections-fire_chance_exclusion_observation_gap = lacune d'observation
ui-replay-sections-fire_chance_exclusion_consumable_unreliable = modèle d'activation des consommables peu fiable
ui-replay-sections-fire_chance_exclusion_victim_fate_unknown = sort de la victime inconnu
ui-replay-sections-fire_chance_exclusion_hit_type_does_not_roll = ce type de coup ne tire pas au sort d'incendie
ui-replay-sections-fire_chance_exclusion_no_geometry = aucune géométrie de section d'incendie
ui-replay-sections-fire_chance_exclusion_impact_unplaceable = impossible de situer l'impact sur le navire auquel il a été associé
ui-replay-sections-fire_chance_exclusion_victim_pose_unknown = position de la victime inconnue au moment de l'impact
ui-replay-sections-fire_chance_exclusion_ambiguous_hit = ambigu avec un autre de nos coups
ui-replay-sections-damage_events = Événements de dégâts

## ui.replay.debug
ui-replay-debug-view_raw_metadata = Voir les métadonnées brutes du joueur
ui-replay-debug-raw_metadata = Métadonnées brutes
ui-replay-debug-raw_results = Résultats bruts
ui-replay-debug-mapped_results = Résultats interprétés
ui-replay-debug-player_metadata = Métadonnées du joueur
ui-replay-debug-search = Rechercher
ui-replay-debug-previous_match = Résultat précédent (Shift+Enter)
ui-replay-debug-next_match = Résultat suivant (Enter)

## ui.replay.controls
ui-replay-controls-window_title = Contrôles de replay
ui-replay-controls-unavailable = Contrôles non disponibles (commands.scheme.xml introuvable dans les fichiers du jeu).

# -- Collaboration / Session --

## ui.collab
ui-collab-session = Session
ui-collab-starting = Démarrage de la session...
ui-collab-session_active = Session active
ui-collab-connected_to_session = Connecté à la session
ui-collab-session_token = Jeton de session :
ui-collab-toggle_visibility = Basculer la visibilité du jeton
ui-collab-copy_token = Copier le jeton
ui-collab-token_copied = Jeton copié dans le presse-papiers
ui-collab-copy_web_link = Copier le lien web
ui-collab-web_link_copied = Lien web copié dans le presse-papiers
ui-collab-copy_localhost_link = Copier le lien localhost
ui-collab-localhost_copied = Lien localhost copié dans le presse-papiers
ui-collab-connected_count = { $count } connecté(s)
ui-collab-promote_cohost = Promouvoir en co-hôte
ui-collab-permissions = Permissions
ui-collab-lock_annotations = Verrouiller les annotations
ui-collab-lock_settings = Verrouiller les paramètres
ui-collab-reset_overrides = Réinitialiser les remplacements client
ui-collab-reset_overrides_tooltip = Réinitialiser toutes les modifications des paramètres d'affichage client
ui-collab-leave = Quitter
ui-collab-role_host = Hôte
ui-collab-role_cohost = Co-hôte
ui-collab-you = (vous)
ui-collab-display_name = Nom d'affichage :
ui-collab-display_name_hint = Votre nom...
ui-collab-enter_display_name = Entrez d'abord un nom d'affichage
ui-collab-host_session = Héberger une session
ui-collab-start_session = Démarrer la session
ui-collab-join_session = Rejoindre une session
ui-collab-paste_and_join = Coller le jeton et rejoindre
ui-collab-clipboard_empty = Le presse-papiers est vide
ui-collab-invalid_token = Jeton invalide : { $error }
ui-collab-no_runtime = La session n'a pas pu démarrer : aucun environnement d'exécution
ui-collab-shared_windows = Fenêtres partagées
ui-collab-watching = En observation
ui-collab-waiting_for_frames = En attente de la bataille
ui-collab-open = Ouvrir
ui-collab-open_for_everyone = Ouvrir pour tout le monde
ui-collab-tactics_board = Tableau tactique

# -- Texte des résultats de bataille --

## battle
battle-victory = VICTOIRE
battle-defeat = DÉFAITE
battle-draw = MATCH NUL
battle-battle_starts_in = LA BATAILLE COMMENCE DANS

# -- Descriptions du type de fin --

## finish_type
finish_type-unknown = Bataille terminée
finish_type-extermination = Tous les navires ennemis détruits
finish_type-base_captured = Base capturée
finish_type-timeout = Temps écoulé
finish_type-failure = Mission échouée
finish_type-technical = Fin technique
finish_type-score = Limite de score atteinte
finish_type-score_on_timeout = Avance au score à la limite de temps
finish_type-pve_success = Mission accomplie
finish_type-pve_failed = Mission échouée
finish_type-score_zero = Score épuisé
finish_type-score_excess = Limite de score dépassée

# -- Onglet Statistiques --

## ui.stats
ui-stats-overview = Aperçu
ui-stats-sample_notice = Données d'exemple, pas des parties enregistrées
ui-stats-div_all = Tous
ui-stats-div_solo = Solo
ui-stats-div_div = Div
ui-stats-filters = Filtres
ui-stats-own_filters = Filtrer ce graphique séparément
ui-stats-own_filters_tooltip = Affiner ce graphique sans modifier ce qu'affichent les autres volets
ui-stats-mode_label = Mode :
ui-stats-clear = Effacer
ui-stats-pr = PR :
ui-stats-mc = MC :
ui-stats-best_frags = Meilleur : { $ship } ({ $count ->
    [one] { $count } élimination
   *[other] { $count } éliminations
})
ui-stats-max_damage = Dégâts max : { $ship } ({ $damage })
ui-stats-copy_markdown = Copier en Markdown
ui-stats-copy_image = Copier comme image
ui-stats-copy_image_done = Graphique copié dans le presse-papiers
ui-stats-copy_image_failed = Le graphique n'a pas pu être copié
ui-stats-copy_csv = Copier en CSV
ui-stats-remove_games_hint = Supprimer toutes les parties de { $ship } (Ctrl+Click pour passer la confirmation)
ui-stats-settings = Paramètres
ui-stats-options = Options
ui-stats-ships = Navires
ui-stats-filter_ships = Filtrer les navires du graphique
ui-stats-all_ships = Tous
ui-stats-no_ships = Aucun
ui-stats-search_ships = Rechercher des navires...
ui-stats-no_ship_matches = Aucun navire ne correspond à cette recherche
ui-stats-search_statistics = Rechercher des statistiques
ui-stats-no_statistic_matches = Aucune statistique ne correspond à cette recherche
ui-stats-ship_selection_count = { $selected } sur { $total } sélectionnés
ui-stats-all_ships_tooltip = Sélectionner tous les navires correspondants
ui-stats-no_ships_tooltip = Effacer la sélection des navires correspondants
ui-stats-limit_to_recent = Limiter aux parties récentes
ui-stats-limit_to_recent_tooltip = La limite s'applique par navire dans Navires et Graphiques, et à l'ensemble des parties dans Vue d'ensemble.
ui-stats-division_label = Division :
ui-stats-add_chart = Ajouter un graphique
ui-stats-clear_tooltip = Oublier toutes les parties enregistrées
ui-stats-clear_failed = Non effacé : { $reason }
ui-stats-clear_ship_confirm = Appuyez à nouveau pour oublier les parties de ce navire
ui-stats-copy_table = Copier ce tableau
ui-stats-copied = Copié dans le presse-papiers
ui-stats-no_games = Aucune partie enregistrée pour les filtres actuels
ui-stats-nothing_to_plot = Rien à afficher pour les filtres actuels
ui-stats-column_ship = Navire
ui-stats-column_battles = Batailles
ui-stats-column_win_rate = Taux de victoire
ui-stats-column_avg_damage = Dégâts moy.
ui-stats-column_avg_frags = Destructions moy.
ui-stats-frags = { $count ->
    [one] { $count } destruction
   *[other] { $count } destructions
}
ui-stats-statistic = Statistique
ui-stats-open_directory_stats = Stats du répertoire
ui-stats-open_directory_stats_tooltip = Ouvrir les stats de tous les replays de ce répertoire et de ses sous-répertoires
ui-stats-directory_stats_unavailable = Les données de jeu sont encore en chargement
ui-stats-directory_reading_task = Lecture des replays pour les stats du répertoire...
ui-stats-directory_walking = Recherche des replays...
ui-stats-directory_scanning = Vérification de { $done } sur { $total ->
    [one] { $total } replay...
   *[other] { $total } replays...
}
ui-stats-directory_reading = Lecture de { $done } sur { $total ->
    [one] { $total } replay...
   *[other] { $total } replays...
}
ui-stats-directory_reload = Recharger
ui-stats-directory_reload_tooltip = Relire les replays de ce répertoire
ui-stats-directory_skipped = { $count ->
    [one] { $count } replay ignoré
   *[other] { $count } replays ignorés
}
ui-stats-directory_skipped_detail =
    Illisibles : { $unreadable }
    Aucune donnée de jeu pour leur version : { $missing_build }
    Aucun résultat de bataille : { $no_results }
ui-stats-chart = Graphique
ui-stats-combine_ships = Combiner les navires
ui-stats-running_average = Moyenne glissante
ui-stats-value_labels = Étiquettes de valeur
ui-stats-reset_view = Réinitialiser la vue
ui-stats-keep_current_layout = Conserver la disposition actuelle
ui-stats-session_title = Stats de session
ui-stats-session_manual = Manuelle
ui-stats-session_since_opened = Depuis l'ouverture de l'application
ui-stats-session_manual_tooltip = La session contient les batailles que vous jouez et les replays que vous définissez ou ajoutez depuis la liste des replays. Elle est conservée entre les lancements.
ui-stats-session_since_opened_tooltip = La session contient chaque bataille commencée à { $time } ou après, à l'ouverture de l'application. Votre session manuelle est conservée et revient lorsque vous passez en Manuelle.
ui-stats-session_battles = { $count ->
    [one] { $count } bataille
   *[other] { $count } batailles
}
ui-stats-session_chip_manual = Session : { $battles }
ui-stats-session_chip_since = Session depuis { $time } : { $battles }
ui-stats-session_empty_title = Aucune bataille dans cette session pour l'instant
ui-stats-session_empty_manual = Les batailles que vous jouez rejoignent la session à leur fin. Vous pouvez aussi définir ou ajouter des replays depuis le menu de la liste des replays.
ui-stats-session_empty_since = Les batailles commençant après { $time } rejoignent la session à leur fin.
ui-stats-session_set_add_unavailable = La session contient les batailles depuis { $time }. Passez en Manuelle pour définir ou ajouter des replays.
ui-stats-clear_unavailable_since = Une session depuis l'ouverture de l'application n'est pas enregistrée. Passez en Manuelle pour effacer la session enregistrée.
ui-stats-charts_save_failed = Paramètres des graphiques non enregistrés : { $reason }
ui-stats-layout_save_failed = Disposition des graphiques non enregistrée : { $reason }
ui-stats-axis_game = Partie
ui-stats-chart_hover_game = Partie { $number }, { $time }
ui-stats-series_average = { $stat } (moyenne)
ui-stats-series_running = { $stat } (glissante)
ui-stats-series_more = +{ $count } de plus
ui-stats-mode_random = Aléatoires
ui-stats-mode_ranked = Classées
ui-stats-mode_other = Autres
ui-stats-win_rate_rated_only = Le taux de victoire ne compte que les batailles aléatoires et classées
ui-stats-win_rate_breakdown_line = { $mode } : { $rate } (batailles : { $battles })
ui-stats-matches = Parties
ui-stats-column_time = Heure
ui-stats-column_mode = Mode
ui-stats-column_result = Résultat
ui-stats-keep_game = Compter cette partie dans les statistiques
ui-stats-leave_out_game = Exclure cette partie des statistiques
ui-stats-leave_out_ship = Exclure toutes les parties avec { $ship }
ui-stats-leave_out_ship_hint = Exclure { $ship } des statistiques
ui-stats-keep_all_games = Compter toutes les parties
ui-stats-keep_all_games_tooltip = Compter à nouveau toutes les parties exclues manuellement
ui-stats-left_out_only = Exclues uniquement
ui-stats-match_count = { $shown } sur { $total ->
    [one] { $total } partie
   *[other] { $total } parties
}, { $left_out } exclue(s)
ui-stats-no_matches = Aucune partie ne correspond aux filtres actuels
ui-stats-search_matches = Rechercher par navire...
ui-stats-ship_filter_tooltip = Choisir quels navires comptent dans les statistiques
ui-stats-keep_ships_tooltip = Compter tous les navires correspondants
ui-stats-leave_out_ships_tooltip = Exclure tous les navires correspondants des statistiques
ui-stats-dates = Dates
ui-stats-all_dates = Toutes les dates
ui-stats-clear_dates = Afficher toutes les dates
ui-stats-games_left_out = { $count ->
    [one] { $count } partie exclue
   *[other] { $count } parties exclues
}
ui-stats-reset_filters = Réinitialiser les filtres
ui-stats-reset_filters_tooltip = Afficher toutes les parties : sans limite, période, division, mode, ni navires et parties exclus

## ui.stats.table
ui-stats-table-min = Min
ui-stats-table-max = Max
ui-stats-table-total = Total
ui-stats-table-average = Moyenne

# -- Suivi des joueurs --

## ui.player_tracker
ui-player_tracker-clear_stats = Effacer les stats
ui-player_tracker-populate_from_replays = Remplir les données depuis les replays
ui-player_tracker-find_matches = Trouver les matchs
ui-player_tracker-open_wows_numbers = Ouvrir sur wows-numbers
ui-player_tracker-open_shipbuilds = Ouvrir les builds sur shipbuilds
ui-player_tracker-subtab_players = Joueurs
ui-player_tracker-filter_hint = Filtrer par joueur ou clan...
ui-player_tracker-time_period = Période
ui-player_tracker-allies = Alliés
ui-player_tracker-enemies = Ennemis
ui-player_tracker-live_no_directory = Définissez le répertoire de World of Warships pour surveiller les batailles
ui-player_tracker-live_checking = Recherche d'une bataille en cours...
ui-player_tracker-live_none = Aucune bataille en cours
ui-player_tracker-live_loading_build = Chargement des données des navires de cette version ; les noms et classes s'afficheront une fois chargées.
ui-player_tracker-waiting_for_index = En attente de l'index des replays
ui-player_tracker-loading_clans = Chargement des clans...
ui-player_tracker-loading_players = Chargement des joueurs...
ui-player_tracker-index_failed = Impossible de lire l'index : { $reason }
ui-player_tracker-no_players = Aucun joueur indexé pour cette période
ui-player_tracker-note_not_saved = Non enregistré : { $reason }
ui-player_tracker-subtab_current_match = Match actuel
ui-player_tracker-encounters_hover = Vu { $total ->
    [one] { $total } fois
   *[other] { $total } fois
} au total, { $range } dans la période sélectionnée. Vu pour la dernière fois { $last }.
ui-player_tracker-never_encountered = Jamais vu auparavant
ui-player_tracker-copy_wg_id = Cliquez pour copier l'ID WG
ui-player_tracker-copied_wg_id = ID WG { $id } copié
ui-player_tracker-aliases_hover = Également connu sous : { $names }
ui-player_tracker-notes_hint = Notes sur ce joueur
ui-player_tracker-arena_count = { $count ->
    [one] { $count } match enregistré
   *[other] { $count } matchs enregistrés
}
ui-player_tracker-last_encountered_exact = Dernière rencontre { $timestamp }
ui-player_tracker-subtab_clans = Clans
ui-player_tracker-clan_no_data = Aucune rencontre de clan enregistrée pour l'instant.
ui-player_tracker-find_clan_matches = Trouver les matchs mentionnant ce tag. L'index n'a pas de champ réservé aux clans, les noms de joueurs contenant le tag correspondent donc aussi.
ui-player_tracker-clan_sightings_hover = { $range } d'entre elles se situent dans la période sélectionnée.
ui-player_tracker-clan_member_matches = vu dans { $count ->
    [one] { $count } match
   *[other] { $count } matchs
}
ui-player_tracker-show_division_mates = Afficher les coéquipiers de division
ui-player_tracker-show_division_mates_hover = Compter les matchs que vous avez joués en division. Désactivé par défaut, car vous y avez croisé ces joueurs parce que vous les aviez invités. Les autres matchs contre les mêmes joueurs sont toujours comptés.
ui-player_tracker-seen_in_twitch_chat = Vu dans le chat Twitch
ui-player_tracker-seen_in_twitch_chat_hover = Afficher uniquement les joueurs dont le nom correspondait à un identifiant du chat Twitch pendant un match avec vous.
ui-player_tracker-twitch_seen_hover = Vu dans le chat Twitch sous { $logins } dans { $count ->
    [one] { $count } match
   *[other] { $count } matchs
} avec vous
ui-player_tracker-win_rate_overall = Global
ui-player_tracker-win_rate_ship = Navire
ui-player_tracker-view_compact = Compact
ui-player_tracker-view_detailed = Détaillé
ui-player_tracker-ship_stats = Stats du navire
ui-player_tracker-stats_resolving = Lecture de la liste des joueurs du match...
ui-player_tracker-stats_fetching = Récupération des stats des joueurs...
ui-player_tracker-stats_unavailable = Stats indisponibles : { $reason }
ui-player_tracker-stats_hidden = Le profil de ce joueur est privé
ui-player_tracker-stats_no_data = Aucune stat pour ce joueur
ui-player_tracker-pr_overall_hover = PR du compte sur tous les navires
ui-player_tracker-pr_ship_hover = PR avec ce navire
ui-player_tracker-win_rate_hover_line = { $label } : { $rate } sur { $count ->
    [one] { $battles } bataille
   *[other] { $battles } batailles
}
ui-player_tracker-win_rate_hover_line_damage = { $label } : { $rate } sur { $count ->
    [one] { $battles } bataille
   *[other] { $battles } batailles
}, { $damage } de dégâts moyens
ui-player_tracker-team_average_win_rate = WR moy. : { $rate }
ui-player_tracker-team_players = { $count ->
    [one] { $count } joueur
   *[other] { $count } joueurs
}
ui-player_tracker-debug_replay_picker = Débogage : traiter un replay comme le match actuel
ui-player_tracker-debug_replay_picker_hover = Charge la liste des joueurs d'un replay terminé dans cet onglet afin de tester le chemin des stats en dehors d'une bataille.
ui-player_tracker-debug_replay_none = Choisir un replay

## ui.player_tracker.column
ui-player_tracker-column-win_rate = WR
ui-player_tracker-column-personal_rating = PR
ui-player_tracker-column-battles = Batailles
ui-player_tracker-column-avg_damage = Dég.
ui-player_tracker-column-encounters = Vu
ui-player_tracker-column-clan = Clan
ui-player_tracker-column-player_name = Nom du joueur
ui-player_tracker-column-total_encounters = Rencontres totales
ui-player_tracker-column-encounters_in_range = Rencontres dans la période
ui-player_tracker-column-last_encountered = Dernière rencontre
ui-player_tracker-column-members = Membres
ui-player_tracker-column-matches = Matchs
ui-player_tracker-column-sightings = Observations

## ui.player_tracker.period
ui-player_tracker-period-past_hour = Dernière heure
ui-player_tracker-period-past_six_hours = 6 dernières heures
ui-player_tracker-period-past_day = 24 dernières heures
ui-player_tracker-period-past_week = Dernière semaine
ui-player_tracker-period-past_month = Dernier mois
ui-player_tracker-period-all_time = Depuis toujours

# -- Gestionnaire de mods --

## ui.twitch
ui-twitch-possible_name = Nom de stream possible : { $name }
ui-twitch-seen_minutes = Vu : { $minutes } minutes après le début du match
ui-twitch-click_to_copy = Cliquez pour copier
ui-twitch-copied = { $name } copié

# -- Rendu de replay --

## ui.renderer
ui-renderer-preparing_export = Préparation de l'export...
ui-renderer-encoding = Encodage
ui-renderer-muxing = Multiplexage

## ui.renderer.advantage
ui-renderer-advantage-breakdown = Détail de l'avantage
ui-renderer-advantage-team_eliminated = Une équipe a été éliminée
ui-renderer-advantage-score_projection = Projection du score
ui-renderer-advantage-fleet_power = Puissance de la flotte
ui-renderer-advantage-strategic_threat = Menace stratégique
ui-renderer-advantage-total = Total
ui-renderer-advantage-hp_incomplete = Données HP/navire incomplètes. Seuls les facteurs de score sont affichés

## ui.renderer.context
ui-renderer-context-show_trail = Afficher la trace
ui-renderer-context-disable_other_trails = Désactiver toutes les autres traces
ui-renderer-context-ranges = Portées
ui-renderer-context-detection = Détection
ui-renderer-context-main_battery = Batterie principale
ui-renderer-context-secondary = Secondaire
ui-renderer-context-torpedo = Torpille
ui-renderer-context-radar = Radar
ui-renderer-context-hydro = Hydro
ui-renderer-context-enable_all = Tout activer
ui-renderer-context-disable_all = Tout désactiver
ui-renderer-context-disable_other_ranges = Désactiver toutes les autres portées
ui-renderer-context-enable_all_ranges = Activer les portées de tous les navires
ui-renderer-context-realtime_armor = Afficher le blindage en temps réel

## ui.renderer.controls
ui-renderer-controls-jump_to_start = Aller au début
ui-renderer-controls-previous_event = Événement précédent (Shift+Left)
ui-renderer-controls-back_10s = Reculer de 10s (Left)
ui-renderer-controls-forward_10s = Avancer de 10s (Right)
ui-renderer-controls-next_event = Événement suivant (Shift+Right)
ui-renderer-controls-jump_to_end = Aller à la fin
ui-renderer-controls-speed = Vitesse de lecture

## ui.renderer.settings
ui-renderer-settings-title = Paramètres d'affichage
ui-renderer-settings-hp_bars = Barres de PV
ui-renderer-settings-armament = Armement
ui-renderer-settings-dead_ships = Navires coulés
ui-renderer-settings-dead_ship_names = Noms des navires coulés
ui-renderer-settings-player_names = Noms des joueurs
ui-renderer-settings-ship_ranges = Portées des navires
ui-renderer-settings-ship_names = Noms des navires
ui-renderer-settings-camera_direction = Direction des tourelles
ui-renderer-settings-heat_trail = Trace thermique
ui-renderer-settings-speed_trails = Traces de vitesse
ui-renderer-settings-dead_ship_trails = Traces des navires coulés
ui-renderer-settings-buildings = Bâtiments
ui-renderer-settings-capture_points = Points de capture
ui-renderer-settings-consumables = Consommables
ui-renderer-settings-planes = Avions
ui-renderer-settings-smoke = Fumigène
ui-renderer-settings-torpedoes = Torpilles
ui-renderer-settings-tracers = Traceurs
ui-renderer-settings-battle_result = Résultat de bataille
ui-renderer-settings-buff_counters = Compteurs de buffs
ui-renderer-settings-chat_label = Chat
ui-renderer-settings-kill_feed = Fil d'éliminations
ui-renderer-settings-score_label = Score
ui-renderer-settings-score_timers = Chronomètres de score
ui-renderer-settings-team_advantage = Avantage d'équipe
ui-renderer-settings-team_rosters = Compositions des équipes
ui-renderer-settings-stats_panel = Panneau des statistiques
ui-renderer-settings-timer = Chronomètre
ui-renderer-settings-export_settings = Paramètres d'export
ui-renderer-settings-prefer_cpu = Préférer l'encodeur CPU
ui-renderer-settings-prefer_cpu_tooltip = Utiliser l'encodeur logiciel (CPU) au lieu du GPU pour l'export vidéo
ui-renderer-settings-include_pre_battle = Inclure la phase d'avant-bataille
ui-renderer-settings-include_pre_battle_tooltip = Inclure l'apparition et le compte à rebours précédant le début de la bataille dans la vidéo exportée
ui-renderer-settings-export_timing = Minutage de la vidéo
ui-renderer-settings-export_timing_length = Ajuster à la durée
ui-renderer-settings-export_timing_speed = Vitesse
ui-renderer-settings-export_timing_result_speed = Lecture à environ { $speed }x
ui-renderer-settings-export_timing_result_length = La vidéo dure { $length }
ui-renderer-settings-codec = Codec
ui-renderer-settings-codec_auto = Auto
ui-renderer-settings-save_defaults = Sauvegarder les valeurs par défaut
ui-renderer-settings-save_defaults_done = Enregistré comme paramètres d'ouverture du rendu
ui-renderer-settings-save_defaults_failed = Les paramètres par défaut n'ont pas pu être enregistrés
ui-renderer-settings-event_timeline = Chronologie des événements

# -- Extracteur de fichiers --

## ui.unpacker
ui-unpacker-assets_bin = Assets.bin
ui-unpacker-loading_assets = Chargement de assets.bin...
ui-unpacker-load_failed = Échec du chargement : { $error }
ui-unpacker-no_game_data = Aucune donnée de jeu chargée. Définissez votre répertoire World of Warships dans les Paramètres.
ui-unpacker-filter_files = Filtrer les fichiers...
ui-unpacker-search_in_files = Rechercher dans les fichiers
ui-unpacker-search_hint = Rechercher dans les fichiers...
ui-unpacker-path_filter_hint = Filtre de chemin (ex. *.xml)
ui-unpacker-queue_all_files = Mettre tous les fichiers en file
ui-unpacker-view_as_json = Voir en JSON
ui-unpacker-extract_as_json = Extraire en JSON
ui-unpacker-view_contents = Voir le contenu
ui-unpacker-version = Version :
ui-unpacker-decode_prototypes = Décoder les prototypes en JSON
ui-unpacker-decode_prototypes_tooltip = Décoder les prototypes Assets.bin pris en charge (.visual, .model, .mfm) en JSON lors de l'extraction
ui-unpacker-extraction_queue = File d'extraction
ui-unpacker-collapse_folders = Réduire l'arborescence
ui-unpacker-expand_folders = Développer l'arborescence
ui-unpacker-clear_all = Tout effacer
ui-unpacker-browse = Parcourir
ui-unpacker-output_dir_hint = Répertoire de sortie...
ui-unpacker-remove_from_queue = Retirer de la file
ui-unpacker-queue_folder = Mettre tout le dossier en file
ui-unpacker-queue_file = Ajouter ce fichier à la file
ui-unpacker-extraction_output = Extraire vers
ui-unpacker-queue_empty = Cochez un fichier ou un dossier dans la liste pour l'ajouter à la file
ui-unpacker-packages = Paquets
ui-unpacker-item_count = { $count ->
    [one] { $count } élément
   *[other] { $count } éléments
}
ui-unpacker-search_title = Recherche : { $query }
ui-unpacker-search_in = dans { $source }
ui-unpacker-image_decode_failed = Impossible de décoder cette image : { $reason }
ui-unpacker-find = Rechercher
ui-unpacker-find_previous = Résultat précédent
ui-unpacker-find_next = Résultat suivant
ui-unpacker-find_close = Fermer la recherche
ui-unpacker-find_count = { $current } sur { $total }
ui-unpacker-find_none = Aucun résultat
ui-unpacker-find_match_case = Respecter la casse
ui-unpacker-queue = File
ui-unpacker-reveal_in_tree = Afficher dans l'arborescence des paquets
ui-unpacker-hit_offset = à { $offset }
ui-unpacker-queued_count = { $count } en file
ui-unpacker-counting_files = Comptage des fichiers...
ui-unpacker-extracting = Extraction { $done } sur { $total }
ui-unpacker-extract_failed = Échec de l'extraction : { $reason }
ui-unpacker-extracted = { $count ->
    [one] { $count } fichier extrait
   *[other] { $count } fichiers extraits
}
ui-unpacker-cancelled = Annulé après { $count ->
    [one] { $count } fichier
   *[other] { $count } fichiers
}
ui-unpacker-extract = { $count ->
    [0] Extraire
    [one] Extraire { $count } élément
   *[other] Extraire { $count } éléments
}
ui-unpacker-choose_directory = Choisissez d'abord un répertoire de destination
ui-unpacker-dump_parameters = Exporter GameParams
ui-unpacker-dump_parameters_tooltip = Exporter le GameParams.data de cette version
ui-unpacker-load_a_build = Chargez d'abord une version
ui-unpacker-parameters_written = Paramètres écrits
ui-unpacker-save_parameters_title = Enregistrer les paramètres du jeu
ui-unpacker-writing = Écriture...
ui-unpacker-prototype_written = Prototype écrit
ui-unpacker-whole_tree = Arborescence complète
ui-unpacker-base_only = Paramètres de base uniquement

## ui.unpacker.column
ui-unpacker-column-name = Nom
ui-unpacker-column-size = Taille
ui-unpacker-column-type_col = Type

# -- Visualiseur de blindage --

## ui.armor
ui-armor-load_settings = Chargez les données de jeu dans les Paramètres pour utiliser le visualiseur de blindage.
ui-armor-load_failed = Échec du chargement des données du navire : { $error }
ui-armor-empty = Vide
ui-armor-mirror_cameras = Miroir des caméras
ui-armor-sync_options = Options de synchronisation
ui-armor-show_armor_by_default = Afficher le blindage par défaut
ui-armor-sync_tooltip = Garder la visibilité du blindage/coque et les surcouches d'affichage (centre du navire, ligne de flottaison, anneaux de caméra) synchronisées entre tous les panneaux
ui-armor-common_settings = Paramètres communs
ui-armor-tier_filter = Rangs { $lowest } à { $highest }
ui-armor-fortifications = Fortifications
ui-armor-show_hidden_ships = Afficher les navires masqués
ui-armor-show_hidden_ships_tooltip = Lister aussi les navires que la liste omet, comme les navires de location de clan
ui-armor-compare_split = Comparer dans un nouveau panneau
ui-armor-open_in_new_tab = Ouvrir dans un nouvel onglet
ui-armor-export_model = Exporter le modèle du navire
ui-armor-export_disclaimer = Ces modèles 3D et textures sont la propriété intellectuelle de Wargaming et toute utilisation de ces modèles doit être conforme aux lois de votre pays.
ui-armor-export_button = Exporter
ui-armor-hull_toggle = Coque
ui-armor-splash_tooltip = Basculer la visibilité de la zone de splash
ui-armor-show_hidden = Afficher les cachés
ui-armor-show_hidden_tooltip = Basculer la visibilité des panneaux (possiblement) cachés
ui-armor-gaps = Espaces
ui-armor-gaps_tooltip = Mettre en évidence les bords de limite où les panneaux de blindage ne se connectent pas (espaces potentiels)
ui-armor-display = Affichage
ui-armor-marker_opacity = Opacité des marqueurs
ui-armor-export_tooltip = Exporter le modèle du navire en fichier OBJ
ui-armor-pen_check = Test de pénétration
ui-armor-trajectory = Trajectoire
ui-armor-trajectory_range = Distance
ui-armor-trajectory_clear = Tout effacer
ui-armor-arc_heading = Arc { $number }
ui-armor-arc_summary = { $hits ->
    [one] { $hits } impact
   *[other] { $hits } impacts
}, { $armor } mm à { $range } km
ui-armor-arc_delete = Supprimer cet arc
ui-armor-arc_isolate_plates = Isoler les plaques
ui-armor-arc_isolate_plates_tooltip = Afficher uniquement les plaques de blindage traversées par cet arc
ui-armor-arc_isolate_zones = Isoler les zones
ui-armor-arc_isolate_zones_tooltip = Afficher toutes les parties des zones de blindage atteintes par cet arc
ui-armor-arc_simulation_caveat = Cette simulation repose sur des données obtenues par rétro-ingénierie et peut différer du calcul balistique du jeu.
ui-armor-arc_no_shell = Aucun obus tiré ; les plaques traversées sont affichées sans obus
ui-armor-arc_detonated_in = Explose dans { $zone }
ui-armor-arc_detonated_past_armor = Explose après tout le blindage
ui-armor-arc_ricochet = Ricochet sur { $plate }
ui-armor-arc_shatter = Fragmentation sur { $plate }
ui-armor-arc_stopped = Arrêté par { $plate }
ui-armor-arc_overpen = Surpénétration
ui-armor-arc_overpen_unarmed = Surpénétration (fusée jamais armée)
ui-armor-arc_plate_unknown = une plaque non répertoriée
ui-armor-angle_good = moins de 30 deg
ui-armor-angle_mid = 30 à 45 deg
ui-armor-angle_bad = plus de 45 deg
ui-armor-trajectory_ship = Navire :
ui-armor-trajectory_select_ship = Sélectionner un navire
ui-armor-trajectory_ship_tooltip = Choisir le navire qui fournit l'obus de trajectoire
ui-armor-trajectory_add_ship = Ajouter un navire tireur
ui-armor-trajectory_add_ship_tooltip = Ouvrir le Test de pénétration pour rechercher et ajouter un navire
ui-armor-trajectory_shell = Obus :
ui-armor-trajectory_select_shell = Sélectionner un obus
ui-armor-trajectory_shell_tooltip = Choisir l'obus utilisé pour les tirs de trajectoire
ui-armor-continue_ricochet = Continuer après le ricochet
ui-armor-splash_mode = Splash
ui-armor-splash_no_data = Pas de données de splash pour ce navire
ui-armor-splash_no_shells = Ajoutez un navire avec des obus HE/SAP à la liste de comparaison
ui-armor-splash_tooltip_mode = Cliquez sur le blindage pour visualiser le volume de dégâts de splash HE
ui-armor-select_ship = Sélectionnez un navire dans la liste
ui-armor-all_btn = Tous
ui-armor-none_btn = Aucun
ui-armor-opaque = Opaque
ui-armor-upgrade = Amélioration :
ui-armor-camo_none = D'origine (aucun)
ui-armor-camo_group_universal = Camouflages universels
ui-armor-camo_group_expendable = Camouflages consommables
ui-armor-camo_group_other = Autres camouflages
ui-armor-show_armor_thickness = Afficher la légende d'épaisseur du blindage
ui-armor-plate_edges = Bords des plaques
ui-armor-waterline = Ligne de flottaison
ui-armor-camera_rings = Anneaux de caméra
ui-armor-realtime_readout = { $health } PV  -  { $hits ->
    [one] { $hits } impact
   *[other] { $hits } impacts
}
ui-armor-show_camera_rings = Afficher les anneaux de caméra
ui-armor-camera_fov = FOV de la caméra
ui-armor-camera_height = Hauteur de la caméra
ui-armor-show_camera_zoom_path = Afficher le trajet du zoom
ui-armor-zoom_path_regular_fov = FOV normal
ui-armor-zoom_path_max_fov = FOV max
ui-armor-camera_perspective = Perspective de la caméra
ui-armor-perspective_fov = FOV de la vue
ui-armor-perspective_projection = Projection
ui-armor-perspective_proj_game = Jeu
ui-armor-perspective_proj_center = Par le centre
ui-armor-ship_center = Centre du navire
ui-armor-grid = Grille
ui-armor-length_marks = Repères de longueur
ui-armor-length_readout = Longueur { $length }, largeur { $beam }
ui-armor-length_metres = { $value } m
ui-armor-length_centimetres = { $value } cm
ui-armor-filter_options = Filtrer les options...
ui-armor-filter_zones = Filtrer les zones...
ui-armor-filter_parts = Filtrer les pièces...
ui-armor-tree_matches = Correspondances : { $count }
ui-armor-tree_no_matches = Aucune correspondance
ui-armor-no_matching_options = Aucune option ne correspond au filtre
ui-armor-opacity = Opacité
ui-armor-opaque_hull = Coque opaque
ui-armor-lighting = Éclairage de la coque
ui-armor-lighting_enabled = Activer l'éclairage
ui-armor-lighting_preset_ingame = En jeu
ui-armor-lighting_preset_flat = Plat
ui-armor-lighting_preset_studio = Studio
ui-armor-lighting_flat = Plat (ambiant)
ui-armor-lighting_intensity = Intensité lumineuse
ui-armor-lighting_azimuth = Azimut de la lumière
ui-armor-lighting_elevation = Élévation de la lumière
ui-armor-lighting_rim = Lumière de contour
ui-armor-lighting_specular = Spéculaire
ui-armor-lighting_shininess = Brillance
ui-armor-lighting_key_color = Couleur principale
ui-armor-lighting_ambient_color = Couleur ambiante
ui-armor-zero_mm_plates = Plaques 0 mm
ui-armor-armor_opacity = Opacité du blindage
ui-armor-ballistic_disclaimer = Les résultats balistiques sont basés sur de l'ingénierie inverse et des estimations du fonctionnement de la simulation
ui-armor-roll = Roulis
ui-armor-reset_plates = Réinitialiser les plaques
ui-armor-armor_options = Blindage
ui-armor-no_ship_loaded = Aucun navire chargé
ui-armor-lod = LOD :
ui-armor-lod_highest = 0 (maximum)
ui-armor-camouflage = Camouflage
ui-armor-ships = Navires
ui-armor-compare = Comparer
ui-armor-compare_tooltip = Ouvrir un nouveau volet de comparaison, ou clic droit sur un navire pour le comparer
ui-armor-mirror_cameras_tooltip = Déplacer ensemble les caméras de tous les volets
ui-armor-show_armor_by_default_tooltip = Ouvrir les modèles avec le blindage affiché plutôt que la coque
ui-armor-xray_highlight = Surbrillance rayons X au survol de la vue
ui-armor-xray_always = Toujours
ui-armor-xray_while_held = En maintenant { $key }
ui-armor-xray_off = Désactivée
ui-armor-stack_panes = Empiler les volets
ui-armor-stack_panes_tooltip = Disposer les volets de comparaison les uns au-dessus des autres plutôt que côte à côte
ui-armor-viewports = Vues
ui-armor-options_panel = Panneau d'options
ui-armor-options_pane = Volet { $number } sur { $count }
ui-armor-splash_boxes = Zones d'éclaboussure
ui-armor-cast_here_hint = Maintenez { $key } et cliquez pour tirer un obus ici
ui-armor-trajectory_next_cast = Prochain tir
ui-armor-arc_pick_hint = Cliquez sur un marqueur pour modifier son arc.
ui-armor-arc_marker_tooltip = Arc { $number }. Cliquez pour le modifier. Clic droit pour le supprimer.
ui-armor-zone_toggle_tooltip = Affiche ou masque toutes les plaques de cette zone. Maintenez { $key } pour afficher uniquement cette zone.
ui-armor-mode_solo_zone = zone seule
ui-armor-mode_cast_trajectory = tirer une trajectoire
ui-armor-loading_catalog = Chargement du catalogue des navires...
ui-armor-legend_collapse = Réduire
ui-armor-legend_hide = Masquer la légende
ui-armor-armor_thickness_title = Épaisseur du blindage
ui-armor-initializing = Initialisation de la vue 3D...
ui-armor-init_failed = Échec de l'initialisation de la vue du blindage : { $reason }
ui-armor-loading_named_ship = Chargement de { $ship }...
ui-armor-options = Options

## ui.armor.compass
ui-armor-compass-north = N
ui-armor-compass-east = E
ui-armor-compass-south = S
ui-armor-compass-west = O

## ui.armor.realtime
ui-armor-realtime-incoming_fire = Tir entrant
ui-armor-realtime-attacker_filter = Filtre d'attaquant
ui-armor-realtime-all_enemies = Tous les ennemis
ui-armor-realtime-show_secondaries = Afficher l'armement secondaire
ui-armor-realtime-seek_to_salvo = Aller à la salve
ui-armor-realtime-no_data = Données d'obus plus disponibles
ui-armor-realtime-sim_agrees = Simulation concorde
ui-armor-realtime-sim_unchecked = { $count ->
    [one] { $count } obus n'a pas pu être simulé
   *[other] { $count } obus n'ont pas pu être simulés
}
ui-armor-realtime-sim_rng_zone = Zone aléatoire ({ $angle } deg dans { $from }-{ $to })
ui-armor-realtime-sim_disagrees = Sim : { $sim } / Serveur : { $server }
ui-armor-realtime-no_armor_hit = Aucune plaque de blindage touchée
ui-armor-realtime-tracked = { $salvos ->
    [one] { $salvos } salve
   *[other] { $salvos } salves
}, { $shells ->
    [one] { $shells } obus suivi
   *[other] { $shells } obus suivis
}
ui-armor-realtime-nothing_landed = Rien n'a encore touché ce navire
ui-armor-realtime-shells = { $count }x
ui-armor-realtime-salvo_shell = { $caliber } mm { $shell }
ui-armor-realtime-salvo_range = { $km } km
ui-armor-realtime-unknown_attacker = Navire { $id }
ui-armor-realtime-server_outcome = Serveur : { $outcome }
ui-armor-realtime-not_cast = Cet obus ne peut pas être suivi dans la coque : sa trajectoire ou la position du navire n'a pas été enregistrée
ui-armor-realtime-pick_hint = Cliquez sur une salve ou un obus pour le suivre dans la coque

## ui.armor.plates
ui-armor-plates-heading = Plaques touchées ({ $count })
ui-armor-plates-arrival = { $velocity } m/s à l'impact, { $time } s de vol, chute à { $fall } deg
ui-armor-plates-overmatch = Surcalibrage : { $penetration } mm de pénétration à { $velocity } m/s
ui-armor-plates-penetrated = Pénètre : { $penetration } mm contre { $effective } mm effectifs à { $velocity } m/s
ui-armor-plates-ricochet = Ricochet à { $angle } deg
ui-armor-plates-shatter = Se brise : { $penetration } mm contre { $effective } mm effectifs
ui-armor-plates-fuse_armed = Fusée armée
ui-armor-plates-detonation = Explose dans { $zone }, { $distance } m après la plaque #{ $number }
ui-armor-plates-detonation_past_armor = Explose au-delà de tout blindage, { $distance } m après la plaque #{ $number }
ui-armor-plates-flat_penetrates = { $ammo } pénètre : { $penetration } mm contre { $thickness } mm
ui-armor-plates-flat_shatters = { $ammo } se brise : { $penetration } mm contre { $thickness } mm
ui-armor-plates-server_angle = serveur { $angle } deg

# -- Niveaux d'avantage --

## advantage
advantage-absolute = Absolu
advantage-strong = Fort
advantage-moderate = Modéré
advantage-weak = Faible

## ui.search
ui-search-edit_value = Modifier la valeur
ui-search-add_filter = Ajouter un filtre
ui-search-pill_select = Sélectionner
ui-search-pill_deselect = Désélectionner
ui-search-pill_group_all = Grouper avec et
ui-search-pill_group_any = Grouper avec ou
ui-search-pill_ungroup = Dissocier
ui-search-pill_negate = Inverser
ui-search-pill_flip_connector = Basculer et/ou
ui-search-pill_delete = Supprimer
ui-search-match_count_truncated = { $count }+ matchs
ui-search-outcome_win = Victoire
ui-search-outcome_loss = Défaite
ui-search-outcome_draw = Match nul
ui-search-outcome_unknown = Inconnu
ui-search-bool_true = vrai
ui-search-bool_false = faux
ui-search-match_count = { $count ->
    [one] { $count } match
   *[other] { $count } matchs
}
ui-search-row_actions_hint = Actions du replay (ou clic droit sur la ligne)
ui-search-looking_up_values = Recherche des valeurs...
ui-search-type_a_query = Saisissez une requête et appuyez sur Entrée
ui-search-parse_failed = Requête invalide : { $reason }
ui-search-searching = Recherche...
ui-search-failed = La recherche a échoué : { $reason }
ui-search-no_matches = Aucun résultat
ui-search-game_mode_gap_hint = { $count ->
    [one] { $count } match indexé n'a pas de mode de jeu enregistré. Utilisez Réindexer tous les replays dans les Paramètres pour le compléter.
   *[other] { $count } matchs indexés n'ont pas de mode de jeu enregistré. Utilisez Réindexer tous les replays dans les Paramètres pour les compléter.
}
ui-search-open = Ouvrir
ui-search-open_missing = Le fichier replay n'existe plus
ui-search-free_text = contient "{ $text }"
ui-search-quant_any = au moins un
ui-search-quant_none = aucun
ui-search-quant_ge = { $n } ou plus
ui-search-quant_eq = exactement { $n }
ui-search-quant_le = au plus { $n }
ui-search-quant_gt = plus de { $n }
ui-search-quant_lt = moins de { $n }
ui-search-quant_ne = pas { $n }
ui-search-scope_self = Moi :
ui-search-scope_ally = Allié :
ui-search-scope_enemy = Ennemi :
ui-search-scope_div = Div :

## ui.search.suggest
ui-search-suggest-preset_divmate_test_ship = Quelqu'un de ma division jouait un navire de test
ui-search-suggest-preset_no_enemy_cv = Aucun porte-avions ennemi
ui-search-suggest-preset_all_enemies_died = Tous les ennemis ont été détruits
ui-search-suggest-preset_high_damage_enemies = 3 ennemis ou plus au-dessus de 100k
ui-search-suggest-preset_stream_sniper = Contient un stream sniper
ui-search-suggest-preset_i_survived = J'ai survécu
ui-search-suggest-preset_i_disconnected = J'ai été déconnecté
ui-search-suggest-my_damage = Mes dégâts
ui-search-suggest-enemy_ship = Navire ennemi
ui-search-suggest-allied_ship = Navire allié
ui-search-suggest-player_in_match = Le joueur était dans le match
ui-search-suggest-someone_in_division = Quelqu'un de ma division
ui-search-suggest-scope_self = Moi :
ui-search-suggest-scope_ally = Allié :
ui-search-suggest-scope_enemy = Ennemi :
ui-search-suggest-scope_division = Division :
ui-search-suggest-scope_anyone = Tout joueur :

## ui.search.field
ui-search-field-outcome = Résultat
ui-search-field-map = Carte
ui-search-field-class = Classe
ui-search-field-tier = Rang
ui-search-field-date = Date
ui-search-field-player_present = Joueur
ui-search-field-group = Groupe de replays
ui-search-field-stat_damage = Dégâts
ui-search-field-stat_kills = Éliminations
ui-search-field-stat_spotting = Repérage
ui-search-field-stat_potential = Dégâts potentiels
ui-search-field-stat_received = Dégâts reçus
ui-search-field-stat_pr = PR
ui-search-field-rating = Cote
ui-search-field-stat_survived = Survécu
ui-search-field-stat_disconnected = Déconnecté
ui-search-field-game_type = Type de bataille
ui-search-field-game_mode = Mode de jeu
ui-search-field-match_group = Groupe de match
ui-search-field-build = Build
ui-search-field-results_available = Résultats disponibles
ui-search-field-relation = Relation
ui-search-field-division = Division
ui-search-field-name = Nom
ui-search-field-clan = Clan
ui-search-field-realm = Serveur
ui-search-field-ship = Navire
ui-search-field-ship_index = Index du navire
ui-search-field-nation = Nation
ui-search-field-test_ship = Navire de test
ui-search-field-stream_sniper = Stream sniper
ui-search-field-sniper_login = Identifiant Twitch du sniper

## ui.search.op
ui-search-op-contains = contient
ui-search-op-equals = égal à
ui-search-op-not_equals = différent de
ui-search-op-eq = =
ui-search-op-ne = !=
ui-search-op-gt = >
ui-search-op-ge = >=
ui-search-op-lt = <
ui-search-op-le = <=
ui-search-op-is = est
ui-search-op-is_not = n'est pas
ui-search-op-is_set = est défini
ui-search-op-is_not_set = n'est pas défini

## ui.search.value
ui-search-value-relation_self = Soi-même
ui-search-value-relation_ally = Allié
ui-search-value-relation_enemy = Ennemi
ui-search-value-division_mine = La mienne
ui-search-value-division_any = N'importe laquelle
ui-search-value-division_none = Aucune
ui-search-value-class_air_carrier = Porte-avions
ui-search-value-class_battleship = Cuirassé
ui-search-value-class_cruiser = Croiseur
ui-search-value-class_destroyer = Destroyer
ui-search-value-class_submarine = Sous-marin
ui-search-value-class_auxiliary = Auxiliaire
ui-search-value-rating_bad = Mauvais
ui-search-value-rating_below_average = Sous la moyenne
ui-search-value-rating_average = Moyen
ui-search-value-rating_good = Bon
ui-search-value-rating_very_good = Très bon
ui-search-value-rating_great = Excellent
ui-search-value-rating_unicum = Unicum
ui-search-value-rating_super_unicum = Super Unicum
ui-search-value-game_mode_test = Test
ui-search-value-game_mode_standard = Standard
ui-search-value-game_mode_singlebase = Base unique
ui-search-value-game_mode_domination = Domination
ui-search-value-game_mode_tutorial = Tutoriel
ui-search-value-game_mode_megabase = Mégabase
ui-search-value-game_mode_forts = Forts
ui-search-value-game_mode_standard_domination = Domination standard
ui-search-value-game_mode_epicenter = Épicentre
ui-search-value-game_mode_assault_defense = Assaut et défense
ui-search-value-game_mode_pve = PVE
ui-search-value-game_mode_arms_race = Course à l'armement
ui-search-value-game_mode_epicenter_ring = Anneau d'épicentre
ui-search-value-game_mode_anti_standard = Anti-standard
ui-search-value-game_mode_attack_defense = Attaque et défense
ui-search-value-game_mode_torpedo_beat = Duel de torpilles
ui-search-value-game_mode_team_battle_royale = Battle royale par équipes
ui-search-value-game_mode_escape_to_portal = Fuite vers le portail
ui-search-value-game_mode_domination_asymm = Domination asymétrique
ui-search-value-game_mode_key_battle = Bataille clé
ui-search-value-game_mode_portal_2021 = Portail 2021
ui-search-value-game_mode_team_battle_royale_2021 = Battle royale par équipes 2021
ui-search-value-game_mode_convoy_event = Événement Convoi
ui-search-value-game_mode_convoy_airship = Convoi dirigeable
ui-search-value-game_mode_two_teams_battle_royale = Battle royale à deux équipes
ui-search-value-game_mode_pinata_event = Événement Pinata
ui-search-value-game_mode_respawns = Réapparitions
ui-search-value-game_mode_respawns_sectors = Réapparitions par secteurs

## ui.search.column
ui-search-column-date = Date
ui-search-column-map = Carte
ui-search-column-mode = Mode
ui-search-column-ship = Votre navire
ui-search-column-result = Résultat
ui-search-column-damage = Dégâts
ui-search-column-kills = Éliminations
ui-search-column-pr = PR
ui-search-column-mc = MC

## ui.settings.index
ui-settings-index-heading = Index des replays
ui-settings-index-description = L'index consultable de chaque replay de votre répertoire
ui-settings-index-build = Construire l'index
ui-settings-index-rebuild = Tout réindexer
ui-settings-index-rebuild_hint = Relit chaque replay, pour corriger les lignes mal analysées auparavant. « Construire l'index » ne lit que ce qui manque.
ui-settings-index-progress = { $done } sur { $total } lus, { $failed } illisibles
ui-settings-index-progress_skipped = { $done } sur { $total } lus, { $skipped } déjà présents, { $failed } illisibles
ui-settings-index-built = { $indexed ->
    [one] { $indexed } replay indexé
   *[other] { $indexed } replays indexés
}, { $failed } illisible(s)
ui-settings-index-built_with_skipped = { $indexed ->
    [one] { $indexed } replay indexé
   *[other] { $indexed } replays indexés
}, { $skipped ->
    [one] { $skipped } déjà présent
   *[other] { $skipped } déjà présents
}, { $failed } illisible(s)

## ui.settings
ui-settings-loading = Chargement des paramètres...
ui-settings-load_failed = Échec du chargement des paramètres
ui-settings-not_set = (non défini)

## ui.settings.panel
ui-settings-panel-mcp = Serveur MCP
ui-settings-panel-open = Ouvrir les paramètres
ui-settings-panel-search_placeholder = Rechercher dans les paramètres
ui-settings-panel-count = { $count ->
    [one] { $count } paramètre
   *[other] { $count } paramètres
}
ui-settings-panel-no_matches = Aucun paramètre ne correspond à la recherche
ui-settings-panel-close = Fermer les paramètres
ui-settings-panel-in_error = { $name } : un paramètre requiert votre attention
ui-settings-panel-general = Général
ui-settings-panel-appearance = Apparence
ui-settings-panel-game = World of Warships
ui-settings-panel-replays = Replays
ui-settings-panel-collaboration = Collaboration
ui-settings-panel-twitch = Twitch
ui-settings-panel-language_description = Langue de l'interface
ui-settings-panel-data_sharing_description = Ce qui est envoyé à ShipBuilds.com après les batailles classées et aléatoires
ui-settings-panel-code_integrity_description = Si du code non signé par Microsoft peut être chargé dans cette application
ui-settings-panel-check_for_updates_description = Rechercher une version plus récente à chaque démarrage de l'application
ui-settings-panel-data_folder = Dossier de données de l'application
ui-settings-panel-theme_description = Couleurs de l'interface. Survolez un choix pour le prévisualiser.
ui-settings-panel-zoom_description = Taille de tout le texte et des commandes
ui-settings-panel-zoom = Zoom
ui-settings-panel-zoom_shortcuts = { $zoom_in } agrandit, { $zoom_out } réduit, { $reset } revient à 1x
ui-settings-panel-zoom_changed = Zoom : { $factor }x
ui-settings-panel-cache_directory = Répertoire du cache
ui-settings-panel-cache_directory_description = Emplacement des builds mis en cache
ui-settings-panel-cached_builds = Builds en cache
ui-settings-panel-current_path_description = Le fichier de replay ouvert le plus récemment
ui-settings-panel-show_raw_xp_description = Ajouter une colonne d'XP brute au tableau des résultats
ui-settings-panel-show_observed_damage_description = Ajouter une colonne de dégâts observés au tableau des résultats
ui-settings-panel-show_entity_id_description = Ajouter une colonne d'ID d'entité au tableau des résultats
ui-settings-panel-show_heals_description = Ajouter une colonne de soins au tableau des résultats
ui-settings-panel-auto_export_description = Écrire les données de chaque bataille dans un fichier au format et dans le répertoire choisis
ui-settings-panel-display_name_description = Le nom que voient les autres dans une session
ui-settings-panel-get_token_description = Ouvrir une page de connexion qui délivre un jeton de chat
ui-settings-panel-token = Jeton de connexion
ui-settings-panel-monitored_channel_description = Chaîne à surveiller. Laissez vide pour surveiller la vôtre.
ui-settings-panel-armor_defaults = Paramètres par défaut enregistrés
ui-settings-panel-armor_saved_description = Enregistré depuis les options du Visualiseur de modèles

## ui.settings.licenses
ui-settings-licenses-title = À propos et licences
ui-settings-licenses-back = Retour aux paramètres
ui-settings-licenses-third_party_intro = Cette application contient les composants tiers listés ci-dessous. Sélectionnez une entrée pour afficher les paquets qui l'utilisent et le texte complet de sa licence.

## ui.settings.armor
ui-settings-armor-no_defaults = (aucun paramètre par défaut enregistré)

## ui.app
ui-app-status_cache_checking = Vérification du cache des données de jeu
ui-app-status_cache_validating = Validation du cache des données de jeu
ui-app-status_cache_downloading = Téléchargement des données de jeu
ui-app-status_cache_planning = Recherche des données de jeu publiées
ui-app-drop_to_load = Déposer pour charger { $file }
ui-app-drop_to_list = Déposer pour lister les replays de { $folder }
ui-app-drop_to_list_replays = Déposer pour lister { $count ->
    [one] { $count } replay
   *[other] { $count } replays
}
ui-app-drop_to_list_folders = Déposer pour lister les replays { $count ->
    [one] du dossier
   *[other] des { $count } dossiers
}
ui-app-drop_to_open_and_list = Déposer pour ouvrir { $file } et lister les replays de { $folders }
ui-app-drop_to_list_replays_and_folders = Déposer pour lister { $count ->
    [one] { $count } replay
   *[other] { $count } replays
} et les replays de { $folders }
ui-app-drop_folder_count = { $count ->
    [one] { $count } dossier
   *[other] { $count } dossiers
}
ui-app-reading_replay = Lecture de { $replay }
ui-app-reading_replays = Lecture de { $count ->
    [one] { $count } replay
   *[other] { $count } replays
}
ui-app-status_constants_generating = Mise à jour des constantes du jeu pour { $version }
ui-app-worker_constants = Le processus des constantes du jeu
ui-app-worker_live_match = La surveillance des batailles
ui-app-worker_replay_loader = Le chargeur de replays
ui-app-worker_restarting = { $worker } s'est arrêté et redémarre
ui-app-worker_stalled = { $worker } ne répond pas
ui-app-worker_stopped = { $worker } s'est arrêté
ui-app-status_indexing = Indexation des replays
ui-app-status_of = { $done } sur { $total }
ui-app-status_tasks = { $count ->
    [one] { $count } tâche en cours
   *[other] { $count } tâches en cours
}
ui-app-status_show_panel = Afficher le panneau : { $panel }
ui-app-status_hide_panel = Masquer le panneau : { $panel }
ui-app-panel_left = Gauche
ui-app-panel_right = Droite
ui-app-panel_bottom = Bas
ui-app-debug_build = Version de débogage

## ui.palette
ui-palette-title = Commandes
ui-palette-placeholder = Saisissez une commande ou recherchez...
ui-palette-go_to = Aller à : { $tab }
ui-palette-advanced_search = Recherche avancée...
ui-palette-set_theme = Thème : { $theme }
ui-palette-games_i_died_in = Parties où j'ai été coulé
ui-palette-games_i_won = Parties gagnées
ui-palette-send_all_replays = Envoyer tous les replays à ShipBuilds
ui-palette-send_all_replays_again = Envoyer tous les replays à ShipBuilds (ignorer le registre)
ui-palette-search_players = Rechercher des joueurs...
ui-palette-my_matches_in_ship = Mes parties avec un navire...
ui-palette-open_ship_in_armor_viewer = Ouvrir un navire dans le Visualiseur de modèles...
ui-palette-open_ship_in_shiptool = Ouvrir un navire dans ShipTool...
ui-palette-players_title = Rechercher des joueurs
ui-palette-my_ships_title = Mes parties avec un navire
ui-palette-armor_ships_title = Ouvrir un navire dans le Visualiseur de modèles
ui-palette-shiptool_ships_title = Ouvrir un navire dans ShipTool
ui-palette-players_placeholder = Rechercher des joueurs par nom...
ui-palette-my_ships_placeholder = Rechercher parmi les navires joués...
ui-palette-armor_ships_placeholder = Rechercher n'importe quel navire par nom...
ui-palette-armor_ship_row = { $name } -- { $class } de rang { $tier }, { $nation }
ui-palette-back_hint = Esc : retour
ui-palette-ships_loading = Chargement de la liste des navires...
ui-palette-ships_unavailable = Aucune liste de navires : { $reason }
ui-palette-matches_found = { $count ->
    [one] { $count } résultat
   *[other] { $count } résultats
}
ui-palette-sample_stats = Débogage : afficher des données d'exemple dans Statistiques
ui-palette-restore_stats = Débogage : afficher les données enregistrées dans Statistiques

## ui.replay.hover
ui-replay-hover-ship = Navire
ui-replay-hover-map = Carte
ui-replay-hover-mode = Mode
ui-replay-hover-blank_mode = Vide
ui-replay-hover-played = Jouée
ui-replay-hover-damage = Dégâts
ui-replay-hover-kills = Destructions
ui-replay-hover-result = Résultat
ui-replay-hover-division = Division
ui-replay-hover-twitch = Chat Twitch

## ui.replay.renderer
ui-replay-renderer-export_video = Rendu en vidéo
ui-replay-renderer-export_clipboard = Rendu dans le presse-papiers
ui-replay-renderer-export_saved = Vidéo enregistrée dans { $path }
ui-replay-renderer-export_copied = Vidéo copiée dans le presse-papiers
ui-replay-renderer-export_not_written = Impossible de faire le rendu de la vidéo : { $reason }
ui-replay-renderer-pop_out = Ouvrir dans une fenêtre séparée
ui-replay-renderer-export_path_unusable = Impossible d'écrire à cet emplacement
ui-replay-renderer-batch_started = Rendu de { $count ->
    [one] { $count } replay
   *[other] { $count } replays
} dans { $dir }
ui-replay-renderer-batch_finished = { $written ->
    [one] { $written } replay rendu
   *[other] { $written } replays rendus
} ; { $failed } n'ont pas pu être rendus
ui-replay-renderer-batch_all_written = { $written ->
    [one] { $written } replay rendu
   *[other] { $written } replays rendus
}
ui-replay-renderer-batch_copied = { $written ->
    [one] { $written } replay rendu et placé dans le presse-papiers
   *[other] { $written } replays rendus et placés dans le presse-papiers
}
ui-replay-renderer-batch_not_copied = Rendu dans { $dir }, mais le presse-papiers les a refusés : { $reason }
ui-replay-renderer-batch_progress = Rendu { $done } sur { $total } : { $replay }
ui-replay-renderer-baking = Lecture du replay et parcours de la bataille...
ui-replay-renderer-failed = Ce replay n'a pas pu être lu
ui-replay-renderer-play = Lecture
ui-replay-renderer-pause = Pause

## ui.renderer.annotations
ui-renderer-annotations-title = Outils de dessin
ui-renderer-annotations-undo = Annuler
ui-renderer-annotations-redo = Rétablir
ui-renderer-annotations-ship_hint = Trouver un navire
ui-renderer-annotations-no_ship = Aucun navire choisi
ui-renderer-annotations-nib = Épaisseur du trait
ui-renderer-annotations-thinner = Trait plus fin
ui-renderer-annotations-thicker = Trait plus épais
ui-renderer-annotations-shortcuts_title = Raccourcis clavier
ui-renderer-annotations-shortcuts_tools = Outils
ui-renderer-annotations-shortcuts_actions = Actions
ui-renderer-annotations-tool_select = Sélection
ui-renderer-annotations-tool_arrow = Flèche
ui-renderer-annotations-tool_freehand = Main levée
ui-renderer-annotations-tool_line = Ligne
ui-renderer-annotations-tool_circle = Cercle
ui-renderer-annotations-tool_rectangle = Rectangle
ui-renderer-annotations-tool_triangle = Triangle
ui-renderer-annotations-tool_measurement = Mesure
ui-renderer-annotations-tool_eraser = Gomme
ui-renderer-annotations-action_undo = Annuler
ui-renderer-annotations-action_multi_select = Sélection multiple
ui-renderer-annotations-action_stroke_width = Épaisseur du trait
ui-renderer-annotations-action_delete = Supprimer
ui-renderer-annotations-action_cancel = Annuler / Désélectionner
ui-renderer-annotations-toggle_fill = Cliquez à nouveau pour activer ou désactiver le remplissage
ui-renderer-annotations-clear_all = Tout effacer
ui-renderer-annotations-edit = Modifier le dessin
ui-renderer-annotations-size = Taille
ui-renderer-annotations-color = Couleur
ui-renderer-annotations-custom_ink = Couleur personnalisée
ui-renderer-annotations-filled = Rempli
ui-renderer-annotations-locked = Annotations verrouillées par l'hôte

## ui.armor.cast
ui-armor-cast-prompt = { $key }+clic sur la coque pour ajouter un point d'impact
ui-armor-cast-empty = Aucun point d'impact pour l'instant
ui-armor-cast-miss_nothing_under_pointer = Aucune plaque de blindage sous le pointeur
ui-armor-cast-miss_thickness_unknown = Les plaques de ce modèle n'ont pas d'épaisseur connue contre laquelle tirer
ui-armor-cast-miss_no_plate_on_path = La trajectoire de l'obus sous cet angle ne traverse aucune plaque de blindage

## ui.armor.pen
ui-armor-pen-search_hint = Rechercher des navires à comparer...
ui-armor-pen-add_a_ship = Recherchez et ajoutez des navires ci-dessus pour comparer la pénétration
ui-armor-pen-hover_a_plate = Survolez une plaque du navire pour la tester
ui-armor-pen-awaiting_plate = survolez une plaque
ui-armor-pen-clear_all = Tout effacer
ui-armor-pen-remove = Retirer de la comparaison
ui-armor-pen-firing_ship_tooltip = Utiliser les obus de ce navire pour les tirs de trajectoire
ui-armor-pen-use_for_trajectory = Utiliser pour la trajectoire
ui-armor-pen-no_shells = { $ship } n'a pas d'obus de batterie principale
ui-armor-pen-ifhe = IFHE
ui-armor-pen-ifhe_tooltip = Amorce à inertie pour obus HE augmente la pénétration des obus HE d'un quart
ui-armor-pen-penetrates = pénètre
ui-armor-pen-shatters = se fragmente
ui-armor-pen-angle_dependent = dépend de l'angle
ui-armor-pen-no_data = aucune donnée de pénétration

## ui.armor.splash
ui-armor-splash-hint = Cliquez sur le blindage en mode splash pour voir quelles zones une explosion atteint
ui-armor-splash-half_extent = Demi-étendue de l'explosion : { $value } unités du modèle
ui-armor-splash-triangles = Triangles de blindage dans l'explosion : { $total }, pénétrés : { $penetrated }
ui-armor-splash-no_zones = Aucune zone d'éclaboussure ne se chevauche à cet endroit
ui-armor-splash-zones = Zones dans l'explosion
ui-armor-splash-direct_hit = Coup direct
ui-armor-splash-hp = { $value } PV
ui-armor-splash-section = Zones d'éclaboussure

## ui.armor.export
ui-armor-export-hull = Coque
ui-armor-export-contents = Contenu
ui-armor-export-contents_mesh = Maillage
ui-armor-export-contents_armor = Blindage
ui-armor-export-contents_both = Maillage et blindage
ui-armor-export-lod = Détail du maillage
ui-armor-export-lod_tooltip = 0 correspond au niveau de détail le plus élevé. Le blindage n'a pas de niveaux de détail et n'est pas affecté.
ui-armor-export-res = Résolution des textures
ui-armor-export-res_full = Complète
ui-armor-export-res_2048 = 2048 px
ui-armor-export-res_1024 = 1024 px
ui-armor-export-res_512 = 512 px
ui-armor-export-camos = Camouflages
ui-armor-export-camo_select_none = Ne rien sélectionner
ui-armor-export-camo_reset = Rétablir par défaut
ui-armor-export-loading_details = Chargement...
ui-armor-export-size_estimate = Taille estimée du fichier : { $size }
ui-armor-export-size_breakdown = Géométrie { $geometry } (exacte), textures { $textures } (estimées)
ui-armor-export-size_incomplete = Impossible d'évaluer { $count ->
    [one] { $count } camouflage
   *[other] { $count } camouflages
} ; la taille peut être incomplète
ui-armor-export-exporting = Export de { $ship }...
ui-armor-export-already_running = Un export est déjà en cours
ui-armor-export-exported = { $ship } exporté
ui-armor-export-exported_with_size = { $ship } exporté ({ $size })
ui-armor-export-vehicle_not_found = Navire introuvable
ui-armor-export-export_failed = Échec de l'export : { $error }

## ui.tactics
ui-tactics-a_peers_board = Un tableau partagé par un autre participant de cette session
ui-tactics-map = Carte
ui-tactics-map_hint = Trouver une carte
ui-tactics-mode = Mode
ui-tactics-pick_a_map = Choisissez une carte sur laquelle dessiner
ui-tactics-drawing = Dessin de la carte...
ui-tactics-reset_view = Réinitialiser la vue
ui-tactics-add_cap = Ajouter un point
ui-tactics-add_cap_tooltip = Placer un point de capture au prochain clic sur la carte
ui-tactics-selected_cap = Point { $letter }
ui-tactics-cap_radius = { $km } km
ui-tactics-cap_team = Équipe
ui-tactics-cap_team_tooltip = Changer l'équipe du point de capture sélectionné
ui-tactics-team_neutral = Neutre
ui-tactics-team_one = Équipe 1 (verte)
ui-tactics-team_two = Équipe 2 (rouge)
ui-tactics-delete_cap = Supprimer
ui-tactics-delete_cap_tooltip = Supprimer le point de capture sélectionné
ui-tactics-cap_replay_delete_tooltip = Un point de capture chargé depuis un replay ne peut pas être supprimé
ui-tactics-cap_narrow_tooltip = Réduire le rayon du point de capture sélectionné
ui-tactics-cap_widen_tooltip = Augmenter le rayon du point de capture sélectionné
ui-tactics-undo_tooltip = Annuler la dernière modification du tableau ({ $shortcut })
ui-tactics-redo_tooltip = Rétablir la dernière modification annulée ({ $shortcut } ou { $alternate })
ui-tactics-clear_caps = Effacer les points ajoutés
ui-tactics-clear_added_caps_tooltip = Supprimer les points de capture ajoutés par l'utilisateur
ui-tactics-preset_name = Nom du tableau
ui-tactics-preset_save = Enregistrer
ui-tactics-preset_delete = Supprimer
ui-tactics-reset_board = Réinitialiser le tableau
ui-tactics-reset_board_tooltip = Restaurer les points du mode sélectionné et effacer les dessins
ui-tactics-preset_needs_a_name = Donnez d'abord un nom au tableau
ui-tactics-preset_needs_a_map = Choisissez d'abord une carte pour le tableau
ui-tactics-preset_saved = { $name } enregistré
ui-tactics-preset_save_failed = Le tableau n'a pas pu être enregistré : { $error }
ui-tactics-preset_load_failed = Le tableau n'a pas pu être ouvert : { $error }
ui-tactics-preset_delete_failed = Le tableau n'a pas pu être supprimé : { $error }
ui-tactics-presets = Tableaux enregistrés
ui-tactics-preset_open = Ouvrir
ui-tactics-clear_drawing = Effacer le dessin
ui-tactics-scan_replays = Remplir les points depuis les replays
ui-tactics-scan_replays_tooltip = Lire les replays pour obtenir la disposition des points de capture des modes joués
ui-tactics-scan_needs_game_data = Chargez d'abord une version du jeu
ui-tactics-scan_running = Lecture des replays...
ui-tactics-scan_progress = Lecture des replays... { $read } / { $total }
ui-tactics-scan_done = { $total ->
    [one] { $total } replay lu
   *[other] { $total } replays lus
}, { $added ->
    [one] { $added } nouvelle disposition trouvée
   *[other] { $added } nouvelles dispositions trouvées
}
ui-tactics-session_boards_full = La session a déjà { $limit ->
    [one] { $limit } tableau ouvert
   *[other] { $limit } tableaux ouverts
}, ce tableau reste donc sur sa carte
ui-tactics-placing = Placement de { $ship }
ui-tactics-side_friendly = Allié
ui-tactics-side_enemy = Ennemi
ui-tactics-ink_white = Encre blanche
ui-tactics-ink_gray = Encre grise
ui-tactics-ink_red = Encre rouge
ui-tactics-ink_orange = Encre orange
ui-tactics-ink_yellow = Encre jaune
ui-tactics-ink_green = Encre verte
ui-tactics-ink_blue = Encre bleue
ui-tactics-ink_purple = Encre violette
ui-tactics-ink_pink = Encre rose
ui-tactics-no_map_art = Cette version ne contient aucune image pour cette carte
ui-tactics-ship_config = Configuration du navire
ui-tactics-hull = Coque
ui-tactics-hull_default = Par défaut
ui-tactics-modifiers = Modificateurs
ui-tactics-concealment_expert = Expert en dissimulation
ui-tactics-gun_range_mod = Mod. de portée des canons
ui-tactics-secondary_range_mod = Mod. de portée secondaire
ui-tactics-ship_ranges = Cercles de portée
ui-tactics-shape = Forme
ui-tactics-shape_size = Taille
ui-tactics-shape_size_km = Taille (km)
ui-tactics-shape_filled = Remplie
ui-tactics-shape_smaller = Réduire la forme
ui-tactics-shape_larger = Agrandir la forme

## ui.armor.outcome
ui-armor-outcome-penetration = Pénétration
ui-armor-outcome-citadel = Citadelle
ui-armor-outcome-ricochet = Ricochet
ui-armor-outcome-shatter = Fragmentation
ui-armor-outcome-overpenetration = Surpénétration
ui-armor-outcome-underwater = Sous l'eau
ui-armor-outcome-unknown = Non reconnu ({ $raw })
ui-armor-outcome-stopped = Arrêté dans le blindage

## ui.armor.sim
ui-armor-sim-overmatch_rules_out_ricochet = Overmatch, donc pas de ricochet
ui-armor-sim-always_ricochet_band = Ricochet, dans la plage de ricochet garanti
ui-armor-sim-ricochet_angle_too_low = Ricochet impossible, angle trop faible

## ui.launcher
ui-launcher-rescan = Réanalyser
ui-launcher-add_install = Ajouter une installation...
ui-launcher-add_install_title = Répertoire d'installation de World of Warships
ui-launcher-no_installs = Aucune installation trouvée. Ajoutez-en une, ou installez un jeu ci-dessous.
ui-launcher-managed_by_steam = géré par Steam
ui-launcher-checking_updates = recherche de mises à jour
ui-launcher-up_to_date = À jour
ui-launcher-update_available = Mise à jour disponible : { $parts }
ui-launcher-not_installed = non installé : { $parts }
ui-launcher-check_failed = échec de la recherche de mises à jour : { $error }
ui-launcher-recheck = Revérifier
ui-launcher-download = Télécharger / Mettre à jour...
ui-launcher-download_title = Emplacement d'installation de la version
ui-launcher-download_hint = Une nouvelle installation pèse plusieurs dizaines de Go ; les mises à jour différentielles ne sont pas encore prises en charge, donc une partie modifiée est retéléchargée entièrement.
ui-launcher-pause = Pause
ui-launcher-cancel = Annuler
ui-launcher-resume = Reprendre
ui-launcher-paused = Téléchargement en pause ; les fichiers préparés sont conservés.
ui-launcher-installed = Installé
ui-launcher-installed_skipped = Installé ({ $count ->
    [one] { $count } partie différentielle ignorée
   *[other] { $count } parties différentielles ignorées
} : pas encore prises en charge, téléchargées mais non appliquées)
ui-launcher-download_failed = Échec du téléchargement : { $error }
ui-launcher-update_in_place = Une mise à jour s'installe dans le dossier de l'installation elle-même. Choisissez { $path } pour la mettre à jour.
ui-launcher-cancelled = Téléchargement annulé.
ui-launcher-launch_failed = Échec du lancement : { $error }
ui-launcher-use_for_toolkit = Utiliser pour le toolkit
ui-launcher-install_heading = Installer un jeu
ui-launcher-install_hint = Une nouvelle installation pèse plusieurs dizaines de Go.
ui-launcher-install_title = Emplacement d'installation du jeu
ui-launcher-install = Installer...
ui-launcher-checking_version = vérification de la version disponible
ui-launcher-version = version { $version }
ui-launcher-version_unreported = version du client non communiquée
ui-launcher-unavailable = indisponible : { $error }
ui-launcher-retry = Réessayer

## ui.launcher.account
ui-launcher-account-sign_in = Se connecter
ui-launcher-account-add_title = Ajouter un compte
ui-launcher-account-again_title = Se reconnecter
ui-launcher-account-memory_only = Connecté pour cette session uniquement. La connexion n'est pas enregistrée ; vous devrez vous reconnecter après un redémarrage.
ui-launcher-account-password_note = Votre mot de passe est envoyé uniquement à Wargaming et n'est jamais stocké.
ui-launcher-account-realm_fixed = Région : { $realm }
ui-launcher-account-email = E-mail
ui-launcher-account-password = Mot de passe
ui-launcher-account-code = Code à deux facteurs
ui-launcher-account-code_required = Code à deux facteurs requis.
ui-launcher-account-use_backup_code = Utiliser un code de secours
ui-launcher-account-submit_code = Envoyer le code
ui-launcher-account-captcha = Cette connexion nécessite un CAPTCHA, que ce lanceur ne peut pas encore afficher. Connectez-vous une fois via le Wargaming Game Center officiel ou wargaming.net dans un navigateur, puis réessayez.

## ui.launcher.sign_in_error
ui-launcher-sign_in_error-wrong_credentials = Adresse e-mail ou mot de passe incorrect.
ui-launcher-sign_in_error-two_factor_required = Ce compte exige une validation à deux facteurs. Approuvez la connexion dans Wargaming Auth ou avec le code reçu par e-mail, puis reconnectez-vous.
ui-launcher-sign_in_error-two_factor_invalid = Le code à deux facteurs est incorrect ou expiré. Saisissez un nouveau code.
ui-launcher-sign_in_error-captcha_required = Wargaming demande un CAPTCHA. Connectez-vous une fois dans le Game Center officiel ou sur wargaming.net, puis réessayez.
ui-launcher-sign_in_error-rate_limited = Trop de tentatives de connexion. Patientez quelques minutes, puis réessayez.
ui-launcher-sign_in_error-account_banned = Ce compte est banni.
ui-launcher-sign_in_error-network_unreachable = Impossible de joindre Wargaming. Vérifiez votre connexion Internet.
ui-launcher-sign_in_error-service_unavailable = Le service de connexion de Wargaming est indisponible. Réessayez plus tard.
ui-launcher-sign_in_error-session_expired = La connexion enregistrée a expiré. Reconnectez-vous.
ui-launcher-sign_in_error-keyring_locked = Votre trousseau système est verrouillé. Déverrouillez-le, puis reconnectez-vous.
ui-launcher-sign_in_error-not_saved = Connecté, mais la connexion n'a pas pu être enregistrée. Vous devrez vous reconnecter après un redémarrage.
ui-launcher-sign_in_error-unexpected = Échec de la connexion. Le journal contient les détails.

## ui.launcher.realm
ui-launcher-realm-eu = Europe
ui-launcher-realm-na = Amérique du Nord
ui-launcher-realm-asia = Asie

## ui.launcher.runner
ui-launcher-runner-looking = Recherche de Proton.
ui-launcher-runner-steam_proton = { $name } de Steam
ui-launcher-runner-umu = umu-run avec { $name }
ui-launcher-runner-umu_default = umu-run avec UMU-Proton
ui-launcher-runner-native = Le jeu s'exécute nativement.
ui-launcher-runner-first_run_prefix = Le premier lancement crée un préfixe Wine, ce qui prend quelques secondes.
ui-launcher-runner-first_run_download = Le premier lancement télécharge UMU-Proton et un environnement d'exécution Steam, environ 700 Mo.
ui-launcher-runner-choose = Choisir un dossier Proton
ui-launcher-runner-automatic = Rechercher automatiquement
ui-launcher-runner-detect = Rechercher à nouveau
ui-launcher-runner-needed = Proton est nécessaire pour lancer le jeu sous Linux. Installez Steam avec Proton, ou umu-launcher.

## ui.replay.damage_share
ui-replay-damage_share-mode = Dégâts en part du total de l'autre navire
ui-replay-damage_share-dealt = { $pct } des dégâts infligés par { $ship } sont allés à { $other }.
ui-replay-damage_share-dealt_inverse = { $pct } des dégâts subis par { $other } venaient de { $ship }.
ui-replay-damage_share-received = { $pct } des dégâts subis par { $ship } venaient de { $other }.
ui-replay-damage_share-received_inverse = { $pct } des dégâts infligés par { $other } sont allés à { $ship }.
ui-replay-damage_share-hold = Maintenez { $key } pour inverser.
ui-replay-damage_share-release = Relâchez { $key } pour revenir.

## ui.reindex
ui-reindex-context = { $count ->
    [one] Forcer la réindexation
   *[other] Forcer la réindexation de { $count } replays
}
ui-reindex-palette_by_version = Forcer la réindexation des replays : par version
ui-reindex-palette_all = Forcer la réindexation des replays : tous
ui-reindex-versions_title = Forcer la réindexation des replays : par version
ui-reindex-versions_placeholder = Filtrer les versions du jeu...
ui-reindex-version_row = { $version } -- { $count ->
    [one] { $count } replay
   *[other] { $count } replays
}
ui-reindex-no_versions = Aucun replay listé n'indique de version du jeu
ui-reindex-confirm_title = Forcer la réindexation des replays
ui-reindex-confirm_ok = Réindexer
ui-reindex-confirm_count = { $count ->
    [one] { $count } replay sera relu et son entrée d'index réécrite.
   *[other] { $count } replays seront relus et leurs entrées d'index réécrites.
}
ui-reindex-confirm_range = Joués du { $oldest } au { $newest }.
ui-reindex-confirm_undated = { $count ->
    [one] { $count } replay à lire n'a pas de date de bataille lisible et n'est pas dans cette période.
   *[other] { $count } replays à lire n'ont pas de date de bataille lisible et ne sont pas dans cette période.
}
ui-reindex-confirm_downloads = Les données de jeu de ces versions sont d'abord téléchargées :
ui-reindex-confirm_unobtainable = Aucune donnée de jeu n'est disponible pour ces versions, { $count ->
    [one] leur { $count } replay est donc ignoré :
   *[other] leurs { $count } replays sont donc ignorés :
}
ui-reindex-confirm_no_cache_dir = Aucun dossier de cache des données de jeu n'est défini, les données de jeu de ces versions ne peuvent donc pas être téléchargées et { $count ->
    [one] leur { $count } replay est ignoré :
   *[other] leurs { $count } replays sont ignorés :
}
ui-reindex-confirm_no_build = { $count ->
    [one] { $count } replay n'indique aucun build du jeu et est ignoré.
   *[other] { $count } replays n'indiquent aucun build du jeu et sont ignorés.
}
ui-reindex-confirm_no_version = { $count ->
    [one] { $count } replay n'indique aucune version du jeu lisible et est ignoré.
   *[other] { $count } replays n'indiquent aucune version du jeu lisible et sont ignorés.
}
ui-reindex-build_row = { $version } (build { $build }) : { $count ->
    [one] { $count } replay
   *[other] { $count } replays
}
ui-reindex-nothing_listed = La liste ne contient aucun replay à réindexer.
ui-reindex-nothing_readable = Aucun de ces replays ne peut être réindexé : aucune donnée de jeu n'est disponible ({ $count } ignorés).
ui-reindex-no_game_data = Les replays ne peuvent pas être lus tant que le dossier du jeu n'est pas défini.
ui-reindex-busy_index = Une indexation est déjà en cours.
ui-reindex-busy_cache = Une tâche de données de jeu est déjà en cours. Réessayez lorsqu'elle sera terminée.
ui-reindex-planning = Vérification des données de jeu disponibles...
ui-reindex-no_index = L'index des replays n'est pas ouvert, rien n'a été réindexé.
ui-reindex-done = { $indexed ->
    [one] { $indexed } replay réindexé
   *[other] { $indexed } replays réindexés
} ; { $skipped } ignoré(s), { $failed } illisible(s)
ui-reindex-stopped = Réindexation arrêtée : { $indexed } réindexé(s), { $skipped } ignoré(s), { $failed } illisible(s)
ui-reindex-stopped_before_reading = Réindexation arrêtée avant la lecture du moindre replay.

## ui.index_folder
ui-index_folder-tooltip = Indexer tous les replays de cette liste qui ne figurent pas encore dans l'index des replays
ui-index_folder-confirm_title = Indexer les replays
ui-index_folder-confirm_ok = Indexer
ui-index_folder-confirm_count = { $count ->
    [one] { $count } replay de cette liste ne figure pas encore dans l'index des replays et sera lu puis ajouté.
   *[other] { $count } replays de cette liste ne figurent pas encore dans l'index des replays et seront lus puis ajoutés.
}
ui-index_folder-note_live = Les replays indexés sont ajoutés à l'index des replays utilisé par la Recherche et le Suivi des joueurs.
ui-index_folder-note_folder = Les replays indexés sont ajoutés à l'index des replays sous la source propre à ce répertoire ({ $dir }). La Recherche et la liste des joueurs du Suivi des joueurs les incluent. La liste des clans du Suivi des joueurs ne lit que les replays du jeu.
ui-index_folder-nothing_listed = Cette liste ne contient aucun replay à indexer.
ui-index_folder-nothing_missing = Tous les replays de cette liste sont déjà indexés.
ui-index_folder-unreadable_index = Impossible de lire l'index des replays.
ui-index_folder-done = { $indexed ->
    [one] { $indexed } replay indexé
   *[other] { $indexed } replays indexés
} ; { $skipped } ignoré(s), { $failed } illisible(s)
ui-index_folder-stopped = Indexation arrêtée : { $indexed } indexé(s), { $skipped } ignoré(s), { $failed } illisible(s)
ui-index_folder-stopped_before_reading = L'indexation a été arrêtée avant la lecture du moindre replay.

## ui.replay_folders
ui-replay_folders-install = Jeu
ui-replay_folders-watched = Surveillé : les replays nouveaux et modifiés apparaissent ici
ui-replay_folders-unwatched = Non surveillé : utilisez Actualiser pour le relire
ui-replay_folders-unseen = { $count ->
    [one] { $count } replay nouveau ou modifié depuis votre dernière visite
   *[other] { $count } replays nouveaux ou modifiés depuis votre dernière visite
}
ui-replay_folders-missing = Ce répertoire n'existe plus : { $dir }
ui-replay_folders-missing_hint = Il s'ouvrira ici à chaque démarrage jusqu'à ce que vous le désépingliez.
ui-replay_folders-pin = Épingler
ui-replay_folders-unpin = Désépingler
ui-replay_folders-refresh = Actualiser
ui-replay_folders-close = Fermer
ui-replay_folders-pins_unreadable = Les répertoires de replays épinglés n'ont pas pu être lus.
ui-replay_folders-game_data_available = Les données du jeu sont disponibles pour les replays listés.
ui-replay_folders-browse = Parcourir...
ui-replay_folders-recent_title = Ouvrir un répertoire de replays
ui-replay_folders-recent_placeholder = Filtrer les répertoires récents...
ui-replay_folders-recent_missing = Ce répertoire n'existe plus et a été retiré de la liste des récents : { $dir }
ui-replay_folders-checking = Recherche de { $dir }...
ui-replay_folders-indexing = Indexation de { $dir }
ui-replay_folders-indexing_in_background = Ce répertoire est encore en cours d'indexation
ui-replay_folders-search_while_indexing = Ce répertoire est encore en cours d'indexation, les résultats ne sont donc pas encore complets. Relancez la recherche une fois l'indexation terminée.
ui-replay_folders-source_failed = Les replays de ce répertoire ne peuvent pas être indexés : { $error }
ui-replay_folders-restart_watch = Relancer la surveillance
ui-replay_folders-watch_stopped = La surveillance de { $folder } s'est arrêtée : { $cause }.
ui-replay_folders-watch_restarting = Relance dans { $seconds } s (tentative { $attempt } sur { $attempts }).
ui-replay_folders-watch_gave_up = La relance a échoué { $attempts ->
    [one] { $attempts } fois
   *[other] { $attempts } fois
} et ne sera plus tentée.
ui-replay_folders-watch_restart_hint = Cliquez sur le point rouge ou choisissez Relancer la surveillance dans le menu de l'onglet pour la relancer maintenant.
ui-replay_folders-watch_resumed = La surveillance de { $folder } a repris.
ui-replay_folders-watch_cause_backend = le système de fichiers a signalé une erreur : { $error }
ui-replay_folders-watch_cause_gone = le dossier a été déplacé ou supprimé
ui-replay_folders-watch_cause_panicked = erreur interne : { $error }
ui-replay_folders-watch_cause_exited = la surveillance s'est arrêtée de manière inattendue
ui-replay_folders-dropped = Déposés
ui-replay_folders-dropped_numbered = Déposés { $number }
ui-replay_folders-dropped_count = { $count ->
    [one] { $count } replay déposé
   *[other] { $count } replays déposés
}
ui-replay_folders-dropped_no_source = Les replays déposés sont indexés avec leurs propres dossiers. Ouvrez un dossier pour le rechercher ou l'indexer.
ui-replay_folders-unreadable_replay = Lecture impossible : { $error }

## ui.models
ui-models-heading = Modèles
ui-models-loading = Chargement des modèles...
ui-models-unavailable = Modèles indisponibles
ui-models-search = Rechercher des navires et des modèles...
ui-models-open = Ouvrir
ui-models-export = Exporter le modèle
ui-models-copy_path = Copier le chemin du modèle
ui-models-path_copied = { $path } copié
ui-models-skin_replaces = Remplace { $model } lorsque { $exterior } est équipé
ui-models-geometry_absent = Absent de vos fichiers de jeu
ui-models-geometry_none = Ce modèle n'a pas de géométrie
ui-models-hardpoints_tooltip = Afficher les points d'attache
ui-models-common = Modèles communs

## ui.models.group
ui-models-group-gameplay = Jeu
ui-models-group-building = Bâtiments
ui-models-group-character = Personnages
ui-models-group-location = Lieux
ui-models-group-port = Ports
ui-models-group-style = Styles
ui-models-group-other = Autres modèles

## ui.models.building
ui-models-building-weapon = Armes de bâtiment
ui-models-building-unassigned = Bâtiments non attribués
ui-models-building-unspecified = Bâtiments sans type

## ui.models.misc
ui-models-misc-crane = Grues
ui-models-misc-anchor = Ancres
ui-models-misc-deck_fitting = Accastillage
ui-models-misc-instrument = Instruments
ui-models-misc-propeller = Hélices
ui-models-misc-searchlight = Projecteurs
ui-models-misc-boat = Embarcations
ui-models-misc-flag = Pavillons
ui-models-misc-insignia = Insignes
ui-models-misc-railing = Garde-corps
ui-models-misc-hatch = Écoutilles
ui-models-misc-ventilation = Ventilation
ui-models-misc-antenna = Antennes
ui-models-misc-weapon = Armes
ui-models-misc-stowage = Rangement
ui-models-misc-other = Autres

## ui.models.variant
ui-models-variant-dead = Épave
ui-models-variant-skin = Apparence

## ui.models.kind
ui-models-kind-ship = Navires
ui-models-kind-gun = Canons
ui-models-kind-director = Directeurs de tir
ui-models-kind-finder = Télémètres
ui-models-kind-radar = Radars
ui-models-kind-catapult = Catapultes
ui-models-kind-aircraft = Avions
ui-models-kind-projectile = Projectiles
ui-models-kind-misc = Divers
ui-models-kind-cap = Points de capture
ui-models-kind-other = Autres

## ui.models.sub_kind
ui-models-sub_kind-battleship = Cuirassés
ui-models-sub_kind-cruiser = Croiseurs
ui-models-sub_kind-destroyer = Destroyers
ui-models-sub_kind-aircraft_carrier = Porte-avions
ui-models-sub_kind-submarine = Sous-marins
ui-models-sub_kind-auxiliary = Navires auxiliaires
ui-models-sub_kind-main_battery = Batterie principale
ui-models-sub_kind-secondary_battery = Batterie secondaire
ui-models-sub_kind-anti_aircraft = Antiaérien
ui-models-sub_kind-torpedo_tubes = Tubes lance-torpilles
ui-models-sub_kind-depth_charge_launcher = Lance-grenades ASM
ui-models-sub_kind-missile_launcher = Lance-missiles
ui-models-sub_kind-search_radar = Radars de veille
ui-models-sub_kind-fire_control_radar = Radars de conduite de tir
ui-models-sub_kind-fighter = Chasseurs
ui-models-sub_kind-bomber = Bombardiers
ui-models-sub_kind-dive_bomber = Bombardiers en piqué
ui-models-sub_kind-scout = Éclaireurs
ui-models-sub_kind-torpedo = Torpilles
ui-models-sub_kind-bomb = Bombes
ui-models-sub_kind-rocket = Roquettes
ui-models-sub_kind-depth_charge = Grenades ASM
ui-models-sub_kind-mine = Mines
ui-models-sub_kind-missile = Missiles
ui-models-sub_kind-jato = Fusées d'appoint
ui-models-sub_kind-shell = Obus

## ui.models.part
ui-models-part-hull = Coque
ui-models-part-main_battery = Batterie principale
ui-models-part-secondaries = Secondaires
ui-models-part-anti_aircraft = DCA
ui-models-part-torpedoes = Torpilles
ui-models-part-directors = Directeurs de tir
ui-models-part-finders = Télémètres
ui-models-part-radars = Radars
ui-models-part-depth_charges = Grenades ASM
ui-models-part-aircraft = Avions
ui-models-part-other = Autres
ui-models-part-misc = Divers

## ui.model_view
ui-model_view-thickness_from_building = Épaisseur d'après { $building }
ui-model_view-thickness_from = Épaisseur selon { $ship } ({ $hp })
ui-model_view-thickness_unknown = Épaisseur inconnue : aucun navire ne monte ce modèle
ui-model_view-thickness_other_ships = Autres navires
ui-model_view-export_title = Exporter le modèle
ui-model_view-export_tooltip = Exporter le modèle dans un fichier GLB
ui-model_view-part_model = Modèle : { $model }
ui-model_view-part_hardpoint = Point d'attache : { $hardpoint }

## ui.model_view.part_group
ui-model_view-part_group-hull = Coque
ui-model_view-part_group-main_battery = Batterie principale
ui-model_view-part_group-secondary_battery = Secondaires
ui-model_view-part_group-aa_guns = DCA
ui-model_view-part_group-torpedoes = Torpilles
ui-model_view-part_group-depth_charges = Grenades ASM
ui-model_view-part_group-fire_control = Conduite de tir
ui-model_view-part_group-radar = Radars
ui-model_view-part_group-missiles = Lance-missiles
ui-model_view-part_group-decorations = Décorations
ui-model_view-part_group-misc = Divers
ui-model_view-part_group-armor = Blindage
ui-model_view-part_group-other = Autres

## ui.scene_problem
ui-scene_problem-cannot_show = Impossible d'afficher { $name }
ui-scene_problem-geometry_absent = La géométrie de ce modèle ne se trouve pas dans vos fichiers de jeu.
ui-scene_problem-model_unreadable = Les données de ce modèle n'ont pas pu être lues. Le journal contient les détails.
ui-scene_problem-ship_unreadable = Les données de ce navire n'ont pas pu être lues. Le journal contient les détails.
ui-scene_problem-no_meshes = Ce modèle n'a aucun maillage.
ui-scene_problem-no_meshes_at_lod = Ce modèle n'a aucun maillage au niveau de détail choisi.
ui-scene_problem-no_visible_surface = Ce modèle n'a aucune surface visible à dessiner.
ui-scene_problem-section_stand_in = Cette section de coque n'a pas de géométrie propre. Le navire dessine cette partie de sa coque dans une autre section.

## ui.armor.option_tabs
ui-armor-option_tabs-label = Onglets d'options
ui-armor-option_tabs-ship = Navire
ui-armor-option_tabs-layers = Calques
ui-armor-option_tabs-analysis = Analyse
ui-armor-option_tabs-configuration = Configuration
ui-armor-option_tabs-hull_parts = Parties de la coque
ui-armor-option_tabs-tab_matches = { $tab }, { $count } correspondances
ui-armor-option_tabs-empty = Rien dans cet onglet ne s'applique au modèle affiché.
ui-armor-option_tabs-no_choices = Ce modèle n'a ni coque, ni module, ni camouflage, ni niveau de détail à choisir.

## ui.armor.hidden_ship
ui-armor-hidden_ship-clan_rental = Location de clan
ui-armor-hidden_ship-other_class = Pas une classe de navire

## ui.armor.textures
ui-armor-textures-title = Textures
ui-armor-textures-hint = Le jeu lit un fichier du dossier res_mods du build à la place du fichier empaqueté au même chemin. Faites un clic droit sur une texture pour copier ce chemin.
ui-armor-textures-under_pointer = Sous le pointeur : { $material }
ui-armor-textures-reading = Lecture des textures...
ui-armor-textures-summary = { $materials ->
    [one] { $materials } matériau
   *[other] { $materials } matériaux
}, { $textures ->
    [one] { $textures } texture
   *[other] { $textures } textures
}
ui-armor-textures-no_material = Aucun matériau
ui-armor-textures-unresolved = Le chemin n'est pas dans assets.bin
ui-armor-textures-absent = Absent des paquets installés
ui-armor-textures-unreadable = Impossible de lire l'en-tête DDS
ui-armor-textures-unknown_format = Format non reconnu
ui-armor-textures-details = { $format }, { $size }, { $count ->
    [one] { $count } fichier
   *[other] { $count } fichiers
}, { $bytes }
ui-armor-textures-role_albedo = Albédo
ui-armor-textures-role_normal = Normale
ui-armor-textures-role_metal_gloss = Métal-brillance
ui-armor-textures-role_ambient_occlusion = Occlusion ambiante
ui-armor-textures-role_detail = Détail
ui-armor-textures-role_name_matched = Albédo, trouvé par nom de fichier
ui-armor-textures-copy_path = Copier le chemin
ui-armor-textures-copy_overrides = { $count ->
    [one] Copier le chemin res_mods
   *[other] Copier { $count } chemins res_mods
}
ui-armor-textures-paths_copied = { $count ->
    [one] { $count } chemin copié
   *[other] { $count } chemins copiés
}
ui-armor-textures-no_build = Les données du jeu ne sont pas chargées, le dossier de mods du build est donc inconnu.
ui-armor-textures-show_in_unpacker = Afficher dans l'extracteur de ressources
ui-armor-textures-show_textures = Afficher les textures
ui-armor-textures-extract = Extraire les fichiers...
ui-armor-textures-extract_title = Extraire les fichiers de texture vers
ui-armor-textures-extracting = Extraction des fichiers de texture...
ui-armor-textures-extracted = { $count ->
    [one] { $count } fichier extrait
   *[other] { $count } fichiers extraits
} vers { $folder }
ui-armor-textures-extract_failed = Impossible d'extraire les fichiers de texture : { $reason }
ui-armor-textures-replace_title = Remplacer les fichiers existants ?
ui-armor-textures-replace_description = { $count ->
    [one] { $count } de ces fichiers est déjà dans { $folder }. L'extraction le remplacera.
   *[other] { $count } de ces fichiers sont déjà dans { $folder }. L'extraction les remplacera.
}
ui-armor-textures-replace = Remplacer

## ui.settings.mcp
ui-settings-mcp-enabled = Activer le serveur MCP
ui-settings-mcp-enabled_description = Fournit les replays et les données du jeu aux assistants IA via MCP. Affiche un contrôle dans la barre d'état.
ui-settings-mcp-start_on_launch = Démarrer avec le programme
ui-settings-mcp-start_on_launch_description = Démarre le serveur à l'ouverture de WoWs Toolkit.
ui-settings-mcp-bind_address = Adresse d'écoute
ui-settings-mcp-bind_address_description = Adresse IP de bouclage sur laquelle le serveur écoute, comme 127.0.0.1 ou ::1. Seul cet ordinateur peut joindre le serveur.
ui-settings-mcp-port = Port
ui-settings-mcp-port_description = Port TCP d'écoute. Choisi au hasard à la première activation et conservé.
ui-settings-mcp-port_unset = Choisi à la première activation
ui-settings-mcp-address_not_loopback = { $input } n'est pas une adresse de bouclage. Le serveur n'écoute que sur cet ordinateur, par exemple sur 127.0.0.1.
ui-settings-mcp-address_invalid = { $input } n'est pas une adresse IP
ui-settings-mcp-port_invalid = { $input } n'est pas un port de 1 à 65535
ui-settings-mcp-port_in_use = Un autre programme utilise ce port. Choisissez un autre port.

## ui.mcp
ui-mcp-started = Serveur MCP actif sur { $url }
ui-mcp-copy_url = Copier l'URL
ui-mcp-start_failed = Impossible de démarrer le serveur MCP : { $reason }
ui-mcp-port_in_use = le port { $port } sur { $address } est utilisé par un autre programme
ui-mcp-no_watchdog = le superviseur des tâches de fond ne fonctionne pas
ui-mcp-status_running = Serveur MCP actif sur { $url }. Cliquez pour l'arrêter.
ui-mcp-status_starting = Démarrage du serveur MCP sur { $address }
ui-mcp-status_failed = Le serveur MCP n'a pas démarré. Cliquez pour réessayer.
ui-mcp-status_stopped = Serveur MCP arrêté. Cliquez pour le démarrer.

## ui.launcher.opt_in
ui-launcher-opt_in-setting = Activer le lanceur de jeu
ui-launcher-opt_in-description = Jouez à World of Warships depuis le toolkit sous Linux, connecté avec votre compte Wargaming.
ui-launcher-opt_in-title = Activer le lanceur de jeu ?
ui-launcher-opt_in-notice_account = En utilisant la fonctionnalité de lanceur, vous reconnaissez que cette fonctionnalité n'est pas un scénario pris en charge par le développeur du jeu. Vous saisissez votre mot de passe directement dans cette application qui, si vous ne lui faites pas confiance, pourrait être utilisée pour voler votre compte (bien que le développeur de l'application ne souhaite pas le faire).
ui-launcher-opt_in-notice_reports = En outre, vous reconnaissez que, si vous utilisez cette fonctionnalité, vous ne signalerez pas de bugs graphiques ou de fiabilité au développeur du jeu, sauf si vous êtes réellement convaincu qu'il s'agit d'un problème multiplateforme qui n'affecte PAS UNIQUEMENT Linux.
ui-launcher-opt_in-continue = Continuer
ui-launcher-opt_in-cancel = Annuler

## ui.launcher.keyring
ui-launcher-keyring-unencrypted = Votre connexion est enregistrée sans chiffrement dans le dossier de données de l'application, car aucun fournisseur Secret Service (comme GNOME Keyring, KeePassXC ou KDE Wallet) n'est disponible.
ui-launcher-keyring-how = Comment la garder chiffrée
ui-launcher-keyring-how_detail = Installez et lancez un fournisseur Secret Service comme GNOME Keyring, KeePassXC ou KDE Wallet, puis redémarrez le toolkit. Les connexions enregistrées y sont transférées.
ui-launcher-keyring-unlock_hint = Votre trousseau système peut demander à être déverrouillé pour que le toolkit lise votre connexion enregistrée.

## ui.launcher.play
ui-launcher-play-play = Jouer
ui-launcher-play-play_through_steam = Jouer via Steam
ui-launcher-play-set_up_game = Configurer le jeu
ui-launcher-play-set_up_runner = Configurer Proton
ui-launcher-play-sign_in_again = Se reconnecter
ui-launcher-play-update = Mettre à jour { $size }
ui-launcher-play-updating = Mise à jour { $percent }%
ui-launcher-play-updating_unknown = Mise à jour
ui-launcher-play-paused = En pause
ui-launcher-play-starting = Démarrage...
ui-launcher-play-running = En cours
ui-launcher-play-in_battle = En bataille
ui-launcher-play-menu = Comptes et options du jeu

## ui.launcher.popover
ui-launcher-popover-game = World of Warships
ui-launcher-popover-no_install = Aucune installation du jeu n'est définie.
ui-launcher-popover-update_available = Mise à jour disponible
ui-launcher-popover-check_failed = Impossible de rechercher des mises à jour
ui-launcher-popover-accounts = Comptes
ui-launcher-popover-no_accounts = Aucun compte pour l'instant. Ajoutez-en un pour jouer connecté.
ui-launcher-popover-close_to_switch = Fermez le jeu pour changer de compte.
ui-launcher-popover-default = Par défaut
ui-launcher-popover-ready = Prêt
ui-launcher-popover-restoring = Vérification...
ui-launcher-popover-not_saved = Non enregistré
ui-launcher-popover-unencrypted = Enregistré sans chiffrement
ui-launcher-popover-keyring_locked = Trousseau verrouillé : déverrouillez-le pour restaurer les connexions enregistrées
ui-launcher-popover-forget = Retirer
ui-launcher-popover-sign_out = Se déconnecter
ui-launcher-popover-expires_in = Expire dans { $duration }
ui-launcher-popover-expired = Session expirée
ui-launcher-popover-expiry_unknown = Expiration inconnue
ui-launcher-popover-make_default = Définir par défaut
ui-launcher-popover-account_actions = Actions du compte
ui-launcher-popover-play_anyway = Jouer quand même
ui-launcher-popover-play_safe_mode = Jouer en mode sans échec
ui-launcher-popover-add_account = Ajouter un compte...
ui-launcher-popover-game_settings = Paramètres du jeu...
ui-launcher-popover-installs = Installations et configuration...
ui-launcher-popover-open_replay = Ouvrir le replay dans le jeu...

## ui.launcher.palette
ui-launcher-palette-play = Jouer
ui-launcher-palette-play_as = Jouer en tant que { $nickname } ({ $realm })
ui-launcher-palette-game_settings = Paramètres du jeu
ui-launcher-palette-add_account = Ajouter un compte
ui-launcher-palette-update = Mettre à jour World of Warships
ui-launcher-palette-open_replay = Ouvrir le replay dans le jeu

## ui.launcher.installs
ui-launcher-installs-toolkit = Répertoire du jeu du toolkit
ui-launcher-installs-played = Le compte par défaut joue celle-ci
ui-launcher-installs-play_from = Jouer depuis ici
ui-launcher-installs-update = Mettre à jour

## ui.launcher.wgc_login
ui-launcher-wgc_login-label = Utiliser la connexion Wargaming
ui-launcher-wgc_login-about = Lance cette installation Steam via la connexion du toolkit au lieu de Steam. Les paramètres et les replays sont conservés séparément des lancements via Steam. Le portefeuille Steam, les succès et le temps de jeu ne fonctionnent pas dans ce mode.
ui-launcher-wgc_login-steam_accounts = Les comptes créés via Steam ont besoin d'une adresse e-mail et d'un mot de passe. Ajoutez-les d'abord dans la Gestion du compte Wargaming.
ui-launcher-wgc_login-steam_accounts_link = Comment ajouter une adresse e-mail et un mot de passe
ui-launcher-wgc_login-ready = Fichiers du Game Center prêts
ui-launcher-wgc_login-fetchable = L'activation télécharge les fichiers du Game Center de WG pour cette version ({ $size }).
ui-launcher-wgc_login-downloading = Téléchargement des fichiers du Game Center : { $percent }%
ui-launcher-wgc_login-downloading_unknown = Téléchargement des fichiers du Game Center
ui-launcher-wgc_login-checking = Recherche des fichiers du Game Center de cette version...
ui-launcher-wgc_login-needs_sign_in = Ajoutez d'abord un compte Wargaming au toolkit.
ui-launcher-wgc_login-unavailable = Indisponible pour cette version : { $error }

## ui.launcher.replay
ui-launcher-replay-no_install = Aucune installation ne possède la version du jeu { $build }. Conservez ou installez une copie de cette version pour regarder ce replay.

## ui.launcher.setup
ui-launcher-setup-title = Configurer World of Warships
ui-launcher-setup-game = Jeu
ui-launcher-setup-proton = Proton
ui-launcher-setup-accounts = Comptes
ui-launcher-setup-replays = Replays
ui-launcher-setup-game_question = Quelle copie du jeu le toolkit doit-il utiliser ?
ui-launcher-setup-none_found = Aucune installation n'a été trouvée sur cet ordinateur.
ui-launcher-setup-choose_folder = Choisir un dossier...
ui-launcher-setup-install_new = Installer une nouvelle copie...
ui-launcher-setup-proton_explain = Sous Linux, le jeu s'exécute via Proton : celui de Steam si Steam est installé, sinon umu-run.
ui-launcher-setup-proton_ready = Proton a été trouvé.
ui-launcher-setup-accounts_explain = Connectez-vous aux comptes Wargaming avec lesquels vous jouez. Jouer connecte le jeu avec le compte par défaut.
ui-launcher-setup-replays_explain = Le jeu supprime ses replays les plus anciens dès qu'il en contient ce nombre.
ui-launcher-setup-skip = Passer la configuration
ui-launcher-setup-back = Retour
ui-launcher-setup-next = Suivant
ui-launcher-setup-done = Terminé

## ui.launcher.game_settings
ui-launcher-game_settings-title = Paramètres du jeu
ui-launcher-game_settings-build = version { $build }
ui-launcher-game_settings-display = Affichage
ui-launcher-game_settings-graphics = Graphismes
ui-launcher-game_settings-sound = Son
ui-launcher-game_settings-replays = Replays
ui-launcher-game_settings-locked = Le jeu est en cours d'exécution. Il réécrit ces fichiers à sa fermeture ; ils sont donc en lecture seule jusque-là.
ui-launcher-game_settings-footer = Écrit dans les fichiers de préférences du jeu. Une sauvegarde est conservée.
ui-launcher-game_settings-cancel = Annuler
ui-launcher-game_settings-save = Enregistrer
ui-launcher-game_settings-saved = Paramètres du jeu enregistrés.
ui-launcher-game_settings-no_install = Aucune installation du jeu n'est définie.
ui-launcher-game_settings-window_mode = Mode d'affichage
ui-launcher-game_settings-windowed = Fenêtré
ui-launcher-game_settings-borderless = Sans bordure
ui-launcher-game_settings-fullscreen = Plein écran
ui-launcher-game_settings-monitor = Écran
ui-launcher-game_settings-monitor_name = Nom de l'écran
ui-launcher-game_settings-resolution = Résolution plein écran
ui-launcher-game_settings-width = Largeur
ui-launcher-game_settings-height = Hauteur
ui-launcher-game_settings-refresh = Fréquence de rafraîchissement
ui-launcher-game_settings-display_rate = Fréquence d'affichage
ui-launcher-game_settings-vsync = Synchronisation verticale
ui-launcher-game_settings-ui_scale = Échelle de l'interface
ui-launcher-game_settings-battle_fps = Limite d'images par seconde en bataille
ui-launcher-game_settings-port_fps = Limite d'images par seconde au port
ui-launcher-game_settings-unlimited = Illimité
ui-launcher-game_settings-skip_intro = Passer la vidéo d'introduction
ui-launcher-game_settings-master = Volume général
ui-launcher-game_settings-music_port = Musique au port
ui-launcher-game_settings-music_battle = Musique en bataille
ui-launcher-game_settings-preset = Préréglage
ui-launcher-game_settings-antialiasing = Anticrénelage
ui-launcher-game_settings-textures = Qualité des textures
ui-launcher-game_settings-filtering = Filtrage des textures
ui-launcher-game_settings-shadows = Qualité des ombres
ui-launcher-game_settings-graphics_note = Modifiez les graphismes dans le jeu. Il les vérifie par rapport à votre matériel.
ui-launcher-game_settings-replays_kept = Replays à conserver
ui-launcher-game_settings-replays_kept_zero = Conservez au moins un replay.
ui-launcher-game_settings-versioned = Trier les replays par version du jeu
ui-launcher-game_settings-replays_note = Écrit dans le res_mods de cette version. Le mode sans échec ne l'utilise pas.
ui-launcher-game_settings-absent = Non défini dans les fichiers du jeu
ui-launcher-game_settings-unrecognized = Valeur inconnue : { $value }
ui-launcher-game_settings-not_a_number = { $field } : saisissez un nombre.

## ui.settings.launcher
ui-settings-launcher-installs = Installations du jeu
ui-settings-launcher-installs_description = Les installations du jeu que le lanceur lance et met à jour.
ui-settings-launcher-runner = Proton (Linux)
ui-settings-launcher-runner_description = Comment le jeu s'exécute sous Linux. Proton Experimental ou le Proton le plus récent de Steam est trouvé automatiquement, sinon umu-run.
ui-settings-launcher-launch_args = Arguments de lancement
ui-settings-launcher-launch_args_description = Arguments de ligne de commande supplémentaires pour le jeu, un par ligne.
ui-settings-launcher-environment = Environnement
ui-settings-launcher-environment_description = Variables d'environnement pour le jeu, KEY=VALUE, une par ligne.
ui-settings-launcher-safe_mode_args = Arguments du mode sans échec
ui-settings-launcher-safe_mode_args_description = Arguments ajoutés lorsque vous jouez en mode sans échec, un par ligne.
