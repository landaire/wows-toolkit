# You are encouraged to suggest edits to this file on GitHub by either filing
# an issue, editing the file directly and creating a pull request, or just
# mentioning it in the Discord.
## meta
meta-language_name = Deutsch

# -- Sprachauswahl-Dialog --

## dialog
dialog-continue_in_english = Continue in English
dialog-continue_in_language = Weiter auf Deutsch
dialog-select_language = Sprache auswählen
dialog-machine_translation_warning = Die meisten Übersetzungen wurden maschinell erstellt und können ungenau sein.

# -- Tab-Titel --

## ui.tabs
ui-tabs-launcher = Launcher
ui-tabs-unpacker = Ressourcen-Entpacker
ui-tabs-settings = Einstellungen
ui-tabs-replay_parser = Replay-Inspektor
ui-tabs-replay_directory = Replay-Verzeichnis
ui-tabs-player_tracker = Spieler-Tracker
ui-tabs-mod_manager = Mod-Verwaltung
ui-tabs-armor_viewer = Panzerungsansicht
ui-tabs-model_viewer = Modellansicht
ui-tabs-stats = Statistiken
ui-tabs-directory_stats = Statistiken: { $dir }
ui-tabs-close_directory_stats = Verzeichnisstatistiken schließen
ui-tabs-search = Suche
ui-tabs-search_these_replays = Diese Replays durchsuchen
ui-tabs-search_these_replays_unavailable = Diese Replays wurden noch nicht indexiert
ui-tabs-zoom_in_panel = Vergrößern
ui-tabs-zoom_out_panel = Verkleinern
ui-tabs-close_panel = Schließen
ui-tabs-close_tab = Schließen
ui-tabs-close_other_tabs = Andere schließen
ui-tabs-close_tabs_to_the_right = Rechts davon schließen

# -- Allgemeine Schaltflächen --

## ui.buttons
ui-buttons-yes = Ja
ui-buttons-no = Nein
ui-buttons-send_replays = Replays senden
ui-buttons-send_build_data = Nur Build-Daten senden
ui-buttons-send_nothing = Nichts senden
ui-buttons-switch_to_replays = Zu Replays wechseln
ui-buttons-keep_current = Aktuelle Einstellung behalten
ui-buttons-stop = Stopp
ui-buttons-copy = Kopieren
ui-buttons-cancel = Abbrechen
ui-buttons-continue_ = Weiter
ui-buttons-install_update = Update installieren
ui-buttons-view_release = Release ansehen
ui-buttons-clear_settings = Einstellungen zurücksetzen
ui-buttons-more_options = Weitere Optionen
ui-buttons-create_issue = Issue erstellen
ui-buttons-discord = Discord
ui-buttons-github = GitHub
ui-buttons-view_github = Projekt auf GitHub ansehen
ui-buttons-choose = Auswählen...
ui-buttons-reset = Zurücksetzen
ui-buttons-select_all = Alle auswählen
ui-buttons-download = Herunterladen
ui-buttons-dismiss = Schließen
ui-buttons-retry = Erneut versuchen
ui-buttons-check_all = Alle auswählen
ui-buttons-uncheck_all = Auswahl aufheben
ui-buttons-refresh_persisted_data_confirm = Aktualisieren

# -- Menüpunkte --

## ui.menu
ui-menu-file = Datei
ui-menu-check_updates = Nach Updates suchen
ui-menu-about = Über
ui-menu-quit = Beenden
ui-menu-licenses = Lizenzen

# -- Fenstertitel --

## ui.windows
ui-windows-build_consent = Einwilligung zur Build-Erfassung
ui-windows-replay_migration = Auf Replay-Freigabe umstellen
ui-windows-crash_detected = Anwendungsabsturz erkannt
ui-windows-update_available = Update verfügbar
ui-windows-confirm = Bestätigen
ui-windows-network_warning = Netzwerkwarnung
ui-windows-download_game_data = Spieldaten herunterladen
ui-windows-error = Fehler
ui-windows-software_encode = Kein GPU-Videoencoder
ui-windows-about = Über
ui-windows-refresh_persisted_data = Gespeicherte Replay-Daten aktualisieren
ui-windows-tactics_board = Taktiktafel
ui-windows-minimize = Minimieren
ui-windows-maximize = Maximieren
ui-windows-restore = Wiederherstellen
ui-windows-close = Schließen
ui-windows-licenses = Lizenzen

# -- Dialog- / Nachrichtentexte --

## ui.dialogs
ui-dialogs-build_consent_message = Möchten Sie Spieler-Build-Informationen aus Rang- und Zufallsgefechten an den Entwickler senden? Diese Datenerhebung hilft der Community, Spieler-Builds in großem Umfang zu analysieren. Sie können sich jederzeit in den Einstellungen abmelden.
ui-dialogs-replay_migration_message = Toolkit kann vollständige Replays teilen, was eine deutlich umfassendere Matchmaking-Analyse ermöglicht als reine Build-Daten. Gefechte, in denen Sie ein Testschiff spielen, werden nicht geteilt. Möchten Sie auf das Teilen vollständiger Replays umstellen?
ui-dialogs-crash_message = Es sieht so aus, als wäre WoWs Toolkit beim letzten Mal abgestürzt. Wenn Sie dieses Problem melden möchten, erstellen Sie bitte ein Issue auf GitHub oder treten Sie dem Discord-Server bei und geben Sie die folgenden Informationen an.
ui-dialogs-crash_clear_settings = Falls gespeicherte Anwendungsdaten für das Problem verantwortlich sein könnten, können Sie versuchen, die Einstellungen über die Schaltfläche unten zurückzusetzen. Dadurch werden alle Einstellungen gelöscht. Verfolgte Spieler bleiben erhalten. Ihre Replays und WoWs-Daten bleiben erhalten.
ui-dialogs-update_message = Version { $tag } von WoWs Toolkit ist verfügbar
ui-dialogs-update_notes = Versionshinweise
ui-dialogs-update_asset_pending = Der Windows-Download ist noch nicht verfügbar. Den Build-Status finden Sie im Release.
ui-dialogs-update_windows_only = Update verfügbar, aber derzeit wird nur Windows unterstützt.
ui-dialogs-p2p_warning = Kollaborative Sitzungen verwenden Peer-to-Peer-Netzwerke. Andere Benutzer in der Sitzung können möglicherweise Ihre IP-Adresse sehen.
ui-dialogs-software_encode_message = Es wurde kein unterstützter GPU-Videoencoder gefunden, daher wird dieser Export per Software kodiert. Das ist deutlich langsamer, und ein langes Gefecht kann mehrere Minuten dauern.
ui-dialogs-download_game_data_intro = Einige Replays benötigen Spieldaten, die nicht installiert sind. Wählen Sie die Builds aus, die heruntergeladen werden sollen.
ui-dialogs-download_build_row = Version { $version } (Build { $build })
ui-dialogs-download_replays_needing = benötigt von { $count } Replay(s)
ui-dialogs-download_availability_resolving = Repository wird geprüft
ui-dialogs-download_availability_exact = veröffentlicht
ui-dialogs-download_availability_nearest = keine genaue Übereinstimmung, am nächsten ist { $version } (Build { $build })
ui-dialogs-download_availability_unpublished = nie veröffentlicht
ui-dialogs-download_availability_unreachable = veröffentlicht, aber die Dateiliste konnte nicht gelesen werden
ui-dialogs-download_plan_pending = Ermittle, was heruntergeladen werden muss...
ui-dialogs-download_objects_to_fetch = Herunterzuladende Objekte für die markierten Builds: { $count }
ui-dialogs-download_plan_failed = Konnte nicht ermitteln, was heruntergeladen werden muss
ui-dialogs-download_plan_no_cache_dir = Es ist kein Cache-Verzeichnis für Spieldaten festgelegt. Wählen Sie eines in den Einstellungen und versuchen Sie es erneut.
ui-dialogs-refresh_persisted_data_message = Jedes Replay in Ihrem Replay-Verzeichnis erneut auswerten und seine gespeicherten Daten neu schreiben. Einträge, die mit einer Replay-Datenzuordnung erfasst wurden, die nicht zu ihrem Spiel-Build passt, bleiben unverändert, mit der Ausnahme, dass persönliche Bewertung und Spielmodus für jedes Replay anhand der heutigen Erwartungswerte neu berechnet werden. Dies kann eine Weile dauern.
ui-dialogs-file_dialog_failed = Der Dateidialog konnte nicht geöffnet werden: { $reason }

# -- Beschriftungen --

## ui.labels
ui-labels-error_occurred = Ein Fehler ist aufgetreten:
ui-labels-made_by = Erstellt von landaire.
ui-labels-credits = Danke an Trackpad, TTaro, lkolbly für ihre Beiträge.
ui-labels-pr_credits = Daten und Formel zur Berechnung der persönlichen Bewertung (PR) bereitgestellt von WoWs Numbers.
ui-labels-powered_by = Betrieben von{" "}
ui-labels-and = {" "}und{" "}
ui-labels-suppress_warning = Nicht mehr anzeigen
ui-labels-more_info = Mehr Informationen
ui-labels-loading_replays = Lade { $count } Replays...
ui-labels-app_version = Version { $version }
ui-labels-project_license = WoWs Toolkit wird unter der Lizenz { $license } verbreitet.
ui-labels-third_party_notices = Hinweise zu Drittanbieterkomponenten
ui-labels-notices_search_hint = Hinweise durchsuchen
ui-labels-no_matching_notices = Keine Hinweise entsprechen der Suche.

# -- Toast-Nachrichten --

## ui.messages
ui-messages-bug_report_info_copied = Informationen für den Fehlerbericht wurden kopiert.
ui-messages-update_downloading = Update wird heruntergeladen: { $read } von { $total }
ui-messages-update_downloading_unknown = Update wird heruntergeladen: bisher { $read }
ui-messages-app_up_to_date = Anwendung ist aktuell
ui-messages-checking_app_updates = Suche nach App-Updates...
ui-messages-update_starting = Update wird heruntergeladen...
ui-messages-update_check_failed = Konnte nicht nach App-Updates suchen
ui-messages-constants_fetch_failed = Replay-Datenzuordnung konnte nicht abgerufen werden. Erneuter Versuch später.
ui-messages-game_data_loaded = Spieldaten erfolgreich geladen
ui-messages-no_replays_detected = Keine Replays erkannt — ist Ihr WoWs-Verzeichnis richtig konfiguriert?
ui-messages-build_loaded = Build { $build } geladen
ui-messages-downloading_game_data = Spieldaten werden heruntergeladen...
ui-messages-game_data_downloaded = Spieldaten für Build { $build } heruntergeladen
ui-messages-game_data_builds_downloaded = Spieldaten für { $count } Build(s) heruntergeladen
ui-messages-game_data_download_failed = Spieldaten konnten nicht heruntergeladen werden
ui-messages-game_data_build_download_failed = Spieldaten für Build { $build } konnten nicht heruntergeladen werden
ui-messages-checking_game_data_updates = Suche nach Spieldaten-Updates...
ui-messages-game_data_up_to_date = Spieldaten sind aktuell
ui-messages-game_data_updates_available = Für { $count } zwischengespeicherte Build(s) sind neuere Daten verfügbar
ui-messages-validating_game_data = Spieldaten-Cache wird überprüft...
ui-messages-game_data_cache_valid = Spieldaten-Cache ist gültig und stimmt mit dem Remote-Repository überein
ui-messages-game_data_cache_invalid = { $count } zwischengespeicherte Build(s) sind veraltet oder beschädigt
ui-messages-replay_loaded = Replay erfolgreich geladen
ui-messages-replay_load_failed = Replay konnte nicht geladen werden
ui-messages-twitch_token_invalid = Twitch-Token ist ungültig oder abgelaufen. Bitte aktualisieren Sie ihn in den Einstellungen.
ui-messages-wows_dir_invalid = World of Warships Verzeichnis ist ungültig. Prüfen Sie den Pfad in den Einstellungen.
ui-messages-wows_dir_not_set = World of Warships Verzeichnis ist nicht festgelegt
ui-messages-twitch_token_expired = Twitch-Token ist ungültig oder abgelaufen
ui-messages-versioned_constants_rebuild_failed = Versionierte Konstanten konnten beim Neuaufbau nicht abgerufen werden. Erneuter Versuch später.
ui-messages-constants_version_mismatch =
    Die Version der Replay-Datenzuordnung stimmt nicht mit der Spielversion überein.
    Nachkampfergebnisse sind möglicherweise nicht genau. Bitte haben Sie Geduld, während die Projektbetreuer die Zuordnung auf dem Server aktualisieren.
ui-messages-constants_updated = Replay-Datenzuordnung erfolgreich aktualisiert
ui-messages-constants_imported = Replay-Datenzuordnung für Build { $build } installiert
ui-messages-constants_imported_other_build = Replay-Datenzuordnung für Build { $build } installiert, dies ist nicht der aktuell geladene Build ({ $loaded })
ui-messages-constants_import_failed = Replay-Datenzuordnung konnte nicht importiert werden: { $error }
ui-messages-session_started = Sitzung gestartet
ui-messages-user_joined = { $name } ist beigetreten
ui-messages-user_timeout = { $name } hat die Verbindung verloren
ui-messages-user_left = { $name } hat die Sitzung verlassen
ui-messages-session_ended = Sitzung beendet
ui-messages-session_error = Sitzungsfehler: { $msg }
ui-messages-connected_to_session = Mit Sitzung verbunden
ui-messages-joined_session = Sitzung beigetreten: { $name }
ui-messages-replay_spam_protection = Getrennt: Host öffnet Replays zu schnell
ui-messages-host_opened_replay = Host hat Replay geöffnet: { $name }
ui-messages-host_closed_replay = Host hat ein Replay geschlossen
ui-messages-session_error_generic = Sitzung: { $msg }
ui-messages-session_rejected = Sitzung abgelehnt: { $reason }
ui-messages-indexing_replays = Replays werden indexiert...
ui-messages-indexing_replays_progress = Replays werden indexiert ({ $done }/{ $total })
ui-messages-replays_indexed = { $indexed } von { $total } Replay(s) indexiert
ui-messages-replays_already_indexed = Alle { $total } Replay(s) sind bereits indexiert
ui-messages-loading_row_summaries = Replay-Details werden geladen...
ui-messages-reading_replay_directory = Replay-Verzeichnis wird gelesen...
ui-messages-directory_replays_skipped = { $skipped } Replay(s) übersprungen, die nicht gelesen werden konnten
ui-messages-drop_not_a_replay = Das ist keine Replay-Datei
ui-messages-drop_not_replays = Das sind keine Replay-Dateien
ui-messages-drop_skipped = { $count } abgelegte Datei(en) übersprungen, die weder Replays noch Ordner sind
ui-messages-directory_replays_not_indexed = { $count } Replay(s) ohne Gefechtsdetails aufgelistet
ui-messages-log_copied = { $bytes } KB aus { $file } in die Zwischenablage kopiert
ui-messages-log_copy_failed = Eine Protokolldatei konnte nicht gelesen werden: { $error }
ui-messages-log_not_found = In { $dir } wurde keine Protokolldatei gefunden

# -- Statistiknamen --

## stat
stat-damage = Schaden
stat-spotting_damage = Aufklärungsschaden
stat-frags = Abschüsse
stat-raw_xp = Roh-XP
stat-base_xp = Basis-XP
stat-win_rate = Siegrate
stat-win_rate_pct = Siegrate (%)
stat-personal_rating = Persönliche Bewertung
stat-match_contribution = Gefechtsbeitrag
stat-capture_points = Eroberungspunkte
stat-planes_shot_down = Abgeschossene Flugzeuge
stat-anti_air_damage = Flugabwehrschaden
stat-survival_rate = Überlebensrate
stat-torpedoes_spotted = Entdeckte Torpedos
stat-avg_damage = Durchschn. Schaden
stat-avg_spotting_damage = Durchschn. Aufklärungsschaden
stat-avg_frags = Durchschn. Abschüsse
stat-avg_raw_xp = Durchschn. Roh-XP
stat-avg_base_xp = Durchschn. Basis-XP
stat-avg_pr = Durchschn. PR
stat-avg_mc = Durchschn. MC
stat-avg_prefix = Durchschn. { $name }

## chart
chart-game_number = Spiel Nr.
chart-combined = Kombiniert
chart-combined_suffix = (Kombiniert)
chart-rolling_average_suffix = (Gleitender Durchschnitt)
chart-no_data = Keine Daten für ausgewählte Schiffe verfügbar
chart-win_rate_unavailable = Siegrate ist für Einzelspiel-Liniendiagramme nicht verfügbar (aktivieren Sie den gleitenden Durchschnitt oder verwenden Sie ein Balkendiagramm)

# -- Bestätigungsdialoge --

## confirm
confirm-open_in_game = Dies wird World of Warships starten. Fortfahren?
confirm-clear_all_session_stats = Dies wird alle Sitzungsstatistiken löschen. Fortfahren?
confirm-clear_ship_session_stats = Dies wird alle Spiele für dieses Schiff aus den Sitzungsstatistiken entfernen. Fortfahren?
confirm-set_as_session_stats = Dies wird Ihre aktuellen Sitzungsstatistiken ersetzen. Fortfahren?

# -- Fehlermeldungen --

## error
error-file_watcher_creation = Dateiüberwachung konnte nicht erstellt werden: { $error }
error-replay_dir_watch = Replay-Verzeichnis konnte nicht überwacht werden: { $error }

# -- Einstellungen --

## ui.settings.app
ui-settings-app-heading = Anwendungseinstellungen
ui-settings-app-description = Allgemeines Anwendungsverhalten und Erscheinungsbild
ui-settings-app-check_for_updates = Beim Start nach Updates suchen
ui-settings-app-enable_logging = Protokollierung aktivieren (wird beim Neustart wirksam)
ui-settings-app-enable_logging_tooltip = Debug-Protokolle in eine Datei im Anwendungsdatenverzeichnis schreiben. Nützlich zur Fehlerbehebung.
ui-settings-app-data_sharing_mode = Datenweitergabe (Rang- und Zufallsgefechte)
ui-settings-app-data_sharing_off = Nichts senden
ui-settings-app-data_sharing_build_data = Build-Daten an ShipBuilds.com senden
ui-settings-app-data_sharing_replays = Replays an ShipBuilds.com senden
ui-settings-app-zoom_factor = Zoomfaktor (Strg + und Strg - ändert dies ebenfalls)
ui-settings-app-theme = Design
ui-settings-app-theme_system = System
ui-settings-app-theme_dark = Dunkel
ui-settings-app-theme_light = Hell
ui-settings-app-code_integrity = Unsignierten Code blockieren (wird beim Neustart wirksam)
ui-settings-app-code_integrity_tooltip = Verhindert das Laden von Code, den Microsoft nicht signiert hat, und hält damit Overlays, Aufnahmewerkzeuge und Audio-Hilfsprogramme aus dieser Anwendung heraus. Diese sind eine häufige Ursache für Abstürze und Hänger beim Rendern. Grafiktreiber sind über Windows signiert und funktionieren weiterhin. Nach dem Start einer Sitzung lässt sich dies nicht mehr abschalten, eine Änderung wirkt daher erst beim nächsten Start. Automatisch überspringt den Schutz, wenn Sie eine Eingabemethode eines Drittanbieters installiert haben, die sonst blockiert würde. Beachten Sie, dass andere Schutzmechanismen unabhängig von dieser Einstellung aktiv bleiben; starten Sie mit --no-hardening, um alle abzuschalten.
ui-settings-app-code_integrity_automatic = Automatisch
ui-settings-app-code_integrity_always = Immer
ui-settings-app-code_integrity_never = Nie
ui-settings-app-language = Sprache
ui-settings-app-open_data_dir = Anwendungsdatenordner öffnen
ui-settings-app-open_data_dir_tooltip = Den Anwendungsdatenordner (Einstellungen, Protokolle, zwischengespeicherte Daten) im Dateimanager öffnen.
ui-settings-app-proxy_url = HTTP-Proxy
ui-settings-app-proxy_url_hint = host:port, oder leer lassen, um die Proxy-Einstellungen des Systems zu verwenden (wird beim Neustart wirksam)
ui-settings-app-proxy_url_hint_live = host:port, oder leer lassen, um die Proxy-Einstellungen des Systems zu verwenden

## ui.settings.wows
ui-settings-wows-heading = World of Warships Einstellungen
ui-settings-wows-description = Pfad zu Ihrer World of Warships Installation
ui-settings-wows-directory_hint = World of Warships Verzeichnis
ui-settings-wows-browse = Durchsuchen...

## ui.settings.wows.cache
ui-settings-wows-cache-heading = Spieldaten-Cache
ui-settings-wows-cache-description = Extrahierte Spieldaten, damit sich Replays älterer Builds weiterhin öffnen lassen
ui-settings-wows-cache-auto_dump = Spieldaten zwischenspeichern für Kompatibilität älterer Replays
ui-settings-wows-cache-directory_label = Cache-Verzeichnis:
ui-settings-wows-cache-browse = Durchsuchen
ui-settings-wows-cache-open_folder = Ordner öffnen
ui-settings-wows-cache-delete_old = Alte Versionen löschen
ui-settings-wows-cache-stats = Spieldaten-Cache: { $size } in { $count } Version(en)
ui-settings-wows-cache-check_updates = Nach Updates suchen
ui-settings-wows-cache-actions = Cache-Aktionen
ui-settings-wows-cache-actions_tooltip = Nach Updates suchen oder zwischengespeicherte Builds überprüfen
ui-settings-wows-cache-update_all = Alle aktualisieren
ui-settings-wows-cache-updates_available = Für { $count } zwischengespeicherte Build(s) sind neuere Daten verfügbar
ui-settings-wows-cache-validate = Cache überprüfen
ui-settings-wows-cache-validate_tooltip = Jeden zwischengespeicherten Build mit dem Remote-Repository abgleichen und die Dateiintegrität prüfen
ui-settings-wows-cache-repair = Reparieren
ui-settings-wows-cache-repair_needed = { $count } zwischengespeicherte Build(s) sind veraltet oder beschädigt

## ui.task
ui-task-batch_render_progress = Rendern { $current }/{ $total }: { $name }

## ui.settings.replay
ui-settings-replay-heading = Replay-Einstellungen
ui-settings-replay-description = Konfigurieren Sie, welche Spalten in der Replay-Ergebnistabelle angezeigt werden
ui-settings-replay-show_raw_xp = Roh-XP anzeigen
ui-settings-replay-show_entity_id = Entity-ID anzeigen
ui-settings-replay-show_observed_damage = Beobachteten Schaden anzeigen
ui-settings-replay-enable_previews = Minimap-Vorschau beim Überfahren anzeigen
ui-settings-replay-enable_previews_tooltip = Beim Überfahren eines Replays wird dieses im Hintergrund gelesen und abgespielt, um eine sich wiederholende Minimap-Vorschau zu erzeugen. Auf langsamen Datenträgern besser deaktivieren.
ui-settings-replay-show_fires = Brände anzeigen
ui-settings-replay-show_floods = Flutungen anzeigen
ui-settings-replay-show_citadels = Zitadellentreffer anzeigen
ui-settings-replay-show_crits = Kritische Modultreffer anzeigen
ui-settings-replay-auto_export_data = Daten automatisch exportieren
ui-settings-replay-format_json = JSON
ui-settings-replay-format_csv = CSV
ui-settings-replay-format_cbor = CBOR
ui-settings-replay-export_path_hint = Verzeichnis für automatischen Datenexport
ui-settings-replay-export_path_missing = Das Verzeichnis existiert nicht. Es können keine Daten dorthin exportiert werden.
ui-settings-replay-index_all_replays = Alle Replays indexieren
ui-settings-replay-reindex_all_replays = Alle Replays neu indexieren
ui-settings-replay-reindex_all_replays_hover = Jedes Replay erneut auswerten und indexieren, auch bereits indexierte. Verwenden Sie dies, nachdem ein Update neue Indexdaten hinzugefügt hat (z. B. persönliche Bewertung, Verbindungsabbruch-Status), damit diese in vorhandene Einträge nachgetragen werden.
ui-settings-replay-show_heals = Heals anzeigen
ui-settings-replay-current_path = Aktueller Replay-Pfad

## ui.settings.session
ui-settings-session-heading = Sitzungseinstellungen
ui-settings-session-description = Einstellungen für kollaborative Replay-Sitzungen
ui-settings-session-display_name = Anzeigename
ui-settings-session-suppress_ip_warning = P2P-IP-Adressenwarnung nicht anzeigen
ui-settings-session-ip_warning_tooltip = Kollaborative Sitzungen verwenden Peer-to-Peer-Netzwerke. Andere Benutzer in der Sitzung können möglicherweise Ihre IP-Adresse sehen.
ui-settings-session-disable_auto_open = Automatisches Öffnen von Sitzungsfenstern deaktivieren
ui-settings-session-auto_open_tooltip = Wenn aktiviert, werden Sitzungsfenster (Replays, Taktiktafeln) nicht automatisch geöffnet. Sie können sie weiterhin über das Sitzungs-Popover öffnen.

## ui.settings.twitch
ui-settings-twitch-heading = Twitch-Einstellungen
ui-settings-twitch-description = Mit Twitch verbinden für Chat-Integration und Spielerverfolgung
ui-settings-twitch-get_token = Anmelde-Token abrufen
ui-settings-twitch-get_token_tooltip = Wir verwenden die Anmeldeseite von Chatterino, da sie ein Token mit den erforderlichen Berechtigungen bereitstellt (im Grunde ein Moderator-Token mit Chat-Berechtigungen), und es entfällt die Notwendigkeit für den WoWs Toolkit-Entwickler, eine eigene Anmeldeseite zu hosten, die das gleiche Ergebnis hätte.
ui-settings-twitch-paste_token_no_token = Token einfügen (Kein Token gesetzt)
ui-settings-twitch-paste_token_tooltip = Liest die Anmeldedaten aus der Zwischenablage
ui-settings-twitch-paste_token_valid = Token einfügen (Token gültig)
ui-settings-twitch-paste_token_unvalidated = Token wird geprüft
ui-settings-twitch-paste_token_invalid = Token einfügen (Token ungültig / abgelaufen)
ui-settings-twitch-monitored_channel = Überwachter Kanal (Standard: eigener Kanal)
ui-settings-twitch-signed_in_as = Angemeldet als { $who }

# -- Replay-Inspektor --

## ui.replay
ui-replay-copy_bug_report_info = Informationen für Fehlerbericht kopieren
ui-replay-collapse_listing = Replay-Liste einklappen
ui-replay-expand_listing = Replay-Liste ausklappen
ui-replay-match_age = vor { $age }
ui-replay-listing_caption = Replays
ui-replay-listing_reading = Lese { $done } von { $total }...
ui-replay-listing_scanning = Prüfe { $done } von { $total }...
ui-replay-listing_downloading = Spieldaten werden heruntergeladen, { $done } von { $total } Dateien...
ui-replay-listing_loading_data = Spieldaten für { $version } werden geladen ({ $index } von { $count })...
ui-replay-loading_game_data_version = Spieldaten für { $version } werden geladen...
ui-replay-spectator = Zuschauer
ui-replay-loading = Laden...
ui-replay-preview_loading = Vorschau wird erstellt...
ui-replay-preview_empty = Keine Bewegung vorhanden
ui-replay-preview_no_game_data = Keine Spieldaten für Client-Version { $value }
ui-replay-preview_unreadable = Dieses Replay konnte nicht gelesen werden
ui-replay-preview_unreadable_because = Dieses Replay konnte nicht gelesen werden: { $value }
ui-replay-preview_unwalkable = Das Gefecht dieses Replays konnte nicht verarbeitet werden: { $value }
ui-replay-preview_unknown_version = Dieses Replay meldet eine unlesbare Client-Version: { $value }
ui-replay-preview_no_map = Keine Kartendaten für { $value }
ui-replay-preview_cancelled = Vorschau wurde ersetzt
ui-replay-preview_render_failed = Die Vorschau konnte nicht gezeichnet werden: { $value }
ui-replay-no_selection = Klicken Sie auf ein Replay zum Anzeigen, oder doppelklicken Sie, um es in einem neuen Tab zu öffnen
ui-replay-workspace_closed = Dieser Replay-Arbeitsbereich wurde geschlossen
ui-replay-incomplete_results = Unvollständige Kampfergebnisse
ui-replay-incomplete_results_tooltip = Das Replay hat noch keine Endkampfergebnisse. Die Daten werden automatisch neu geladen, wenn das Gefecht endet und die Endergebnisse dem Replay hinzugefügt werden.
ui-replay-no_chat = In diesem Replay wurden keine Chatnachrichten gesendet
ui-replay-chat_copy_all = Alles kopieren
ui-replay-chat_save_to_file = In Datei speichern
ui-replay-chat_save_title = Spielchat speichern
ui-replay-export_results_title = Ergebnisse exportieren
ui-replay-results_exported = Ergebnisse exportiert
ui-replay-chat_saved = Chat gespeichert unter { $path }
ui-replay-chat_save_failed = Der Chat wurde nicht gespeichert: { $error }
ui-replay-team_damage = Teamschaden:{" "}
ui-replay-team_share = { $pct } der Teamsumme ({ $team })
ui-replay-hide_my_stats = Meine Testschiff-Statistiken ausblenden
ui-replay-open_manually = Replay-Datei manuell öffnen...
ui-replay-open_directory = Replay-Verzeichnis öffnen...
ui-replay-search_these_replays = Diese Replays durchsuchen
ui-replay-reading_directory = Replays in { $dir } werden aufgelistet
ui-replay-import_constants = constants.json-Datei importieren...
ui-replay-refresh_persisted_data = Gespeicherte Replay-Daten aktualisieren...
ui-replay-contributing = { $done } von { $total } Replays werden beigetragen
ui-replay-contributed = { $sent } von { $total } Replays beigetragen
ui-replay-contribute_nothing_shared = Wählen Sie zuerst in den Einstellungen, was geteilt werden soll
ui-replay-contribute_none_listed = Keine Replays zum Beitragen aufgelistet
ui-replay-contribute_not_ready = Replays können beigetragen werden, sobald die Spieldaten und die Einstellungen geladen sind
ui-replay-contribute_already_running = Ein Beitragsdurchlauf läuft bereits
ui-replay-auto_export_failed = Ein Gefecht konnte nicht geschrieben werden: { $error }
ui-replay-auto_exported = Gefecht wurde nach { $file } exportiert
ui-replay-opened_new_battle = Neues Gefecht geöffnet: { $battle }
ui-replay-new_battle_listed = Neues Gefecht: { $battle }
ui-replay-results_arrived = Die Gefechtsergebnisse für { $battle } sind eingetroffen
ui-replay-constants_written = Ergebniszuordnungen für { $count } Build(s) wurden heruntergeladen
ui-replay-constants_import_title = constants.json importieren
ui-replay-constants_imported = Die Ergebniszuordnung für Build { $build } wurde importiert
ui-replay-constants_import_failed = Diese Zuordnung wurde nicht importiert: { $reason }
ui-replay-constants_no_build = Laden Sie zuerst einen Spiel-Build
ui-replay-constants_latest_written = Die neueste Ergebniszuordnung wurde heruntergeladen
ui-replay-constants_check_failed = Die Ergebniszuordnungen konnten nicht geprüft werden: { $reason }
ui-replay-constants_build_label = Build { $build }
ui-replay-constants_generating = Spielkonstanten für { $version } werden aktualisiert...
ui-replay-constants_generated = Spielkonstanten für { $version } wurden in { $seconds } s erzeugt
ui-replay-constants_generation_failed = Spielkonstanten für { $version } konnten nicht aktualisiert werden: { $reason }. { $fallback }
ui-replay-constants_fallback_kept = Die bereits gespeicherten Konstanten werden beibehalten.
ui-replay-constants_fallback_published = Die für Build { $build } veröffentlichten Konstanten werden verwendet.
ui-replay-constants_fallback_embedded = Die integrierten Konstanten werden verwendet.
ui-replay-copy_latest_log = Neuestes Protokoll in die Zwischenablage kopieren
ui-replay-load_alt_perspective = Perspektive des anderen Teams laden...
ui-replay-load_alt_perspective_tooltip = Die Aufzeichnung eines anderen Spielers desselben Gefechts in das aktuelle Replay einfügen. Zeigt Feindpositionen, Builds, Torpedos, Artillerie und Verbrauchsgüter aus der anderen Perspektive.
ui-replay-load_alt_perspective_disabled_tooltip = Öffnen oder fokussieren Sie zuerst ein Replay.
ui-replay-load_alt_perspective_failed = Replay konnte nicht zusammengeführt werden: { $error }
ui-replay-load_alt_perspective_limit = Dieses Gefecht wird bereits über { $limit } weitere Aufzeichnungen gelesen
ui-replay-consumable_charges_remaining = { $remaining } von { $total } Ladungen übrig
ui-replay-consumable_charges_unlimited = { $used } verwendet (unbegrenzte Ladungen)
ui-replay-consumable_header_consumable = Verbrauchsgut
ui-replay-consumable_header_remaining = Übrig
ui-replay-consumable_header_total = Gesamt
ui-replay-consumable_charges_infinite = ∞
ui-replay-autoload_latest = Neuestes Replay automatisch laden
ui-replay-autoload_latest_tooltip = Ein Gefecht öffnen, sobald das Spiel es fertig geschrieben hat
ui-replay-anonymize_names = Spielernamen anonymisieren
ui-replay-anonymize_names_tooltip = Erzeugte Namen statt Spielernamen und Clan-Tags anzeigen. Exporte behalten die echten Namen.
ui-replay-group_win_rate_all_modes = Alle Spielmodi in die Siegrate der Gruppen einbeziehen
ui-replay-group_win_rate_all_modes_tooltip = Standardmäßig zählen Gruppenüberschriften nur Zufalls- und Ranglistengefechte. Ist dies aktiviert, zählen alle Spielmodi und die Siegrate wird nicht eingefärbt, da die Farben auf Zufallsgefechte abgestimmt sind.
ui-replay-select_replay = Replay auswählen
ui-replay-drop_hint = Ziehen Sie ein Replay hierher, um es zu öffnen, oder einen Ordner, um seine Replays aufzulisten
ui-replay-scanning = Replays werden durchsucht...
ui-replay-scanning_count = Lese Replay { $done } von { $total }
ui-replay-no_replays_in = Keine Replays in { $folder }
ui-replay-drop_folder_hint = Ziehen Sie einen Ordner in das Fenster, um ihn in einem eigenen Tab aufzulisten
ui-replay-new_tag = Neu
ui-replay-loading_game_data = Spieldaten werden geladen...
ui-replay-game_data_failed = Spieldaten konnten nicht geladen werden: { $error }
ui-replay-group_label = Replay-Gruppierung
ui-replay-column_filters = Spaltenfilter
ui-replay-row_elements = Zeilenelemente
ui-replay-row_element_personal_rating = Persönliche Bewertung
ui-replay-row_element_match_contribution = Gefechtsbeitrag
ui-replay-row_element_division = Division
ui-replay-row_element_twitch = Twitch-Chat
ui-replay-row_element_damage = Schaden
ui-replay-row_element_kills = Abschüsse
ui-replay-row_element_time = Gefechtszeit
ui-replay-listing_options = Listenoptionen
ui-replay-open = Öffnen
ui-replay-open_file = Replay-Datei öffnen...
ui-replay-check_game_data = Spieldaten prüfen
ui-replay-check_game_data_tooltip = Nach Spieldaten suchen, die von Replays in diesem Verzeichnis benötigt werden
ui-replay-close_side_panel = Bereich schließen
ui-replay-nda = NDA
ui-replay-render = Rendern
ui-replay-chat = Chat
ui-replay-game_chat = Spielchat
ui-replay-copy_all = Alles kopieren
ui-replay-save_to_file = In Datei speichern
ui-replay-chat_copied = Chat kopiert
ui-replay-copy_message = Nachricht kopieren
ui-replay-actions = Aktionen
ui-replay-row_actions_hint = Spieleraktionen (oder Rechtsklick auf die Zeile)
ui-replay-section_match = Gefecht
ui-replay-section_export = Export
ui-replay-section_debug = Debug
ui-replay-timeline = Gefechtsverlauf
ui-replay-row_not_indexed = nicht indexiert
ui-replay-hover_survived = Überlebt
ui-replay-hover_sunk = Versenkt
ui-replay-hover_not_indexed = Nicht indexiert
ui-replay-row_kills = { $count } Abschüsse
ui-replay-row_survived = überlebt
ui-replay-row_sunk = versenkt
ui-replay-row_division = Division: { $members }
ui-replay-export_chat_save = Chat: In Datei speichern
ui-replay-export_chat_copy = Chat: Kopieren
ui-replay-export_results_json = Ergebnisse: JSON
ui-replay-export_results_cbor = Ergebnisse: CBOR
ui-replay-export_results_csv = Ergebnisse: CSV
ui-replay-timeline_filter = Filter
ui-replay-timeline_filter_all = Alle
ui-replay-timeline_filter_none = Keine
ui-replay-timeline_search_hint = Schiff oder Spieler suchen
ui-replay-timeline_no_events = Keine bedeutenden Ereignisse erkannt.
ui-replay-timeline_no_matches = Keine Ereignisse entsprechen dem aktuellen Filter.
ui-replay-timeline_parsing = Ereignisse werden ausgewertet...
ui-replay-timeline_failed = Gefechtsverlauf konnte nicht extrahiert werden: { $error }
ui-replay-timeline_retry = Erneut versuchen
ui-replay-timeline_export_tooltip = Gilt für die nach der Filterung aktuell sichtbaren Ereignisse.
ui-replay-timeline_copied = Gefechtsverlauf kopiert
ui-replay-timeline_event_copied = Ereignis kopiert
ui-replay-timeline_saved = Zeitleiste gespeichert unter { $path }
ui-replay-timeline_save_failed = Die Zeitleiste wurde nicht gespeichert: { $error }
ui-replay-timeline_kind_health_lost = Schwerer Schaden
ui-replay-timeline_kind_death = Zerstört
ui-replay-timeline_kind_cap_contested = Punkt umkämpft
ui-replay-timeline_kind_cap_flipped = Punkt erobert
ui-replay-timeline_kind_cap_being_captured = Punkt wird erobert
ui-replay-timeline_kind_radar = Radar eingesetzt
ui-replay-timeline_kind_advantage = Vorteilsänderung
ui-replay-timeline_kind_disconnected = Verbindung getrennt

## ui.replay.context
ui-replay-context-open_in_new_tab = In neuem Tab öffnen
ui-replay-context-copy_cell = Zelle kopieren
ui-replay-context-copy_row = Zeile kopieren
ui-replay-context-copy_table = Tabelle kopieren
ui-replay-context-pin_column = Spalte anheften
ui-replay-context-unpin_column = Spalte lösen
ui-replay-context-copied = Kopiert
ui-replay-context-copy_path = Pfad kopieren
ui-replay-context-copy_replay = Replay kopieren
ui-replay-context-copy_replays = { $count } Replays kopieren
ui-replay-context-copy_paths = { $count } Pfade kopieren
ui-replay-context-show_in_explorer = Im Datei-Explorer anzeigen
ui-replay-context-show_replay_controls = Replay-Steuerung anzeigen
ui-replay-context-open_in_game = Im Spiel öffnen
ui-replay-context-render_replay = Replay rendern
ui-replay-context-set_session_stats_one = Als Sitzungsstatistik setzen (1 Replay)
ui-replay-context-set_session_stats_many = Als Sitzungsstatistik setzen ({ $count } Replays)
ui-replay-context-add_session_stats_one = Zu Sitzungsstatistik hinzufügen (1 Replay)
ui-replay-context-add_session_stats_many = Zu Sitzungsstatistik hinzufügen ({ $count } Replays)
ui-replay-context-render_to_video = Als Video rendern
ui-replay-context-render_to_video_many = { $count } Replays als Video rendern
ui-replay-context-render_to_clipboard = In Zwischenablage rendern
ui-replay-context-render_to_clipboard_many = { $count } Replays in Zwischenablage rendern

## ui.replay.column
ui-replay-column-actions = Aktionen
ui-replay-column-player_name = Spielername
ui-replay-column-base_xp = Basis-XP
ui-replay-column-raw_xp = Roh-XP
ui-replay-column-ship_name = Schiffsname
ui-replay-column-hits = Treffer
ui-replay-column-heals = Heals
ui-replay-column-heals_tooltip = Anzahl der beobachteten Aktivierungen der Reparaturmannschaft bei diesem Spieler. Kann bei Schiffen ungenau sein, die nicht auf dem Bildschirm dargestellt werden (z. B. feindliche Schiffe, die nie aufgeklärt wurden).
ui-replay-column-heals_no_repair_tooltip = Dieses Schiff hat keine Reparaturmannschaft.
ui-replay-column-kills = Abschüsse
ui-replay-column-observed_damage = Beobachteter Schaden
ui-replay-column-actual_damage = Tatsächlicher Schaden
ui-replay-column-spotting_damage = Aufklärungsschaden
ui-replay-column-potential_damage = Potenzieller Schaden
ui-replay-column-time_lived = Überlebenszeit
ui-replay-column-fires = Brände
ui-replay-column-floods = Flutungen
ui-replay-column-citadels = Zitadellen
ui-replay-column-crits = Krit. Treffer
ui-replay-column-received_damage = Erhaltener Schaden
ui-replay-column-distance_traveled = Zurückgelegte Strecke
ui-replay-column-skills = Fähigkeiten
ui-replay-column-personal_rating = PR
ui-replay-column-match_contribution = MC

## ui.rating
ui-rating-personal_rating = Persönliche Bewertung
ui-rating-match_contribution = Gefechtsbeitrag
ui-rating-versus_average = ggü. Durchschnitt
ui-rating-average = Durchschnitt
ui-rating-damage = Schaden
ui-rating-kills = Abschüsse
ui-rating-wins = Sieg
ui-rating-team_damage_share = Anteil am Teamschaden
ui-rating-tanking = Auf sich gezogenes Feuer
ui-rating-spotting = Aufklärung
ui-rating-objectives = Ziele
ui-rating-anti_air = Flugabwehr
ui-rating-torpedo_warnings = Torpedowarnungen
ui-rating-critical_bursts = Kritische Schadensspitzen
ui-rating-unrated_early_exit = Kein MC: Die Aufzeichnung endete vor dem Gefecht
ui-rating-mc_unrated_battle_type = Kein MC: Der Gefechtsbeitrag gilt nur für Zufalls- und Ranglistengefechte

## ui.rating.detail
ui-rating-detail-versus_expected = { $actual } ggü. { $expected } erwartet
ui-rating-detail-wins = { $actual } gewonnen ggü. { $expected } erwartet
ui-rating-detail-weighted_damage = { $actual } ggü. { $expected } erwartet, Schaden an Zerstörern und U-Booten x1,2
ui-rating-detail-team_damage_share = { $share }% des Teamschadens an Gegnern, gleichmäßiger Anteil wäre { $even }%
ui-rating-detail-tanking = { $potential } potenzieller Schaden, { $class }x der Klassendurchschnitt, { $team }x ein gleichmäßiger Anteil des Teams
ui-rating-detail-spotting = { $damage } Aufklärungsschaden (Klassendurchschnitt { $baseline }), aufgeklärte Schiffs-HP { $ships } (Durchschnitt { $ships_baseline })
ui-rating-detail-objectives = Punkte: { $capture } Einnehmen, { $block } Blockieren, { $reset } Zurücksetzen, { $battle_end } am Ende gehalten, { $hold } Halten (zu einem Viertel gewertet)
ui-rating-detail-anti_air_planes = { $strike } Angriffsflugzeuge, { $support } Jäger und Aufklärer
ui-rating-detail-anti_air_damage = { $damage } Flak-Schaden
ui-rating-detail-torpedo_warnings = { $threatening } bedrohliche Torpedos entdeckt
ui-rating-detail-torpedo_warnings_one = 1 bedrohlicher Torpedo entdeckt
ui-rating-detail-critical_bursts = { $count } kritische Schadensspitzen: { $bursts }. Nur für Schiffe in der Nähe des aufzeichnenden Spielers sichtbar
ui-rating-detail-critical_bursts_one = 1 kritische Schadensspitze: { $bursts }. Nur für Schiffe in der Nähe des aufzeichnenden Spielers sichtbar
ui-rating-detail-critical_burst = { $share }% von { $ship } in { $seconds } s
ui-rating-detail-critical_burst_at_once = { $share }% von { $ship } auf einmal

## ui.personal_rating
ui-personal_rating-unrated_hover = Die persönliche Bewertung gilt nur für Zufalls- und Rangliste-Gefechte.

## ui.rating.basis
ui-rating-basis-static = Statisch
ui-rating-basis-dynamic = Dynamisch
ui-rating-basis-static_explained = Statisch: gemessen an einer festen Konstante oder einem Klassenrichtwert
ui-rating-basis-dynamic_explained = Dynamisch: gemessen an den Erwartungswerten von wows-numbers oder den anderen Spielern in diesem Gefecht

## ui.replay.filter
ui-replay-filter-raw_xp = Roh-XP
ui-replay-filter-entity_id = Entity-ID
ui-replay-filter-observed_damage = Beobachteter Schaden
ui-replay-filter-heals = Heals
ui-replay-filter-fires = Brände
ui-replay-filter-floods = Flutungen
ui-replay-filter-citadels = Zitadellen
ui-replay-filter-crits = Kritische Modultreffer

## ui.replay.group
ui-replay-group-date = Datum
ui-replay-group-ship = Schiff
ui-replay-group-none = Keine
ui-replay-group-prefix = Gruppe: { $label }

## ui.replay.player
ui-replay-player-hidden_profile = Spieler hat ein verstecktes Profil
ui-replay-player-never_connected = Spieler hat sich nie mit dem Gefecht verbunden

## ui.replay.build
ui-replay-build-open_shiptool = Build in ShipTool öffnen
ui-replay-build-open_in_ship_builder = Build in WoWs SB öffnen
ui-replay-build-copy_link = Build-Link kopieren
ui-replay-build-copy_short_link = Kurzen Build-Link kopieren
ui-replay-build-link_copied = Build-Link kopiert
ui-replay-build-open_wows_numbers = WoWs Numbers Seite öffnen
ui-replay-build-not_spotted = Dieses Schiff wurde nie aufgeklärt. Build-Informationen nicht verfügbar.

## ui.replay.sections
ui-replay-sections-achievements = Auszeichnungen
ui-replay-sections-ribbons = Abzeichen
ui-replay-sections-modules = Module:
ui-replay-sections-modules_none = Keine Module
ui-replay-sections-signals = Signale:
ui-replay-sections-loadout = Ausrüstung:
ui-replay-sections-loadout_none = Keine Ausrüstung
ui-replay-sections-abilities = Fähigkeiten:
ui-replay-sections-abilities_none = Keine Fähigkeiten
ui-replay-sections-captain_skills = Kapitänsfertigkeiten:
ui-replay-sections-captain_skills_none = Keine Kapitänsfertigkeiten
ui-replay-sections-consumables = Verbrauchsgüter:
ui-replay-sections-fire_chance = Effektive Brandwahrscheinlichkeit
ui-replay-sections-fire_chance_expected = erwartet
ui-replay-sections-fire_chance_fires = { $fires } Brände
ui-replay-sections-fire_chance_fires_one = 1 Brand
ui-replay-sections-fire_chance_eligible_hits = { $hits } Treffer, die einen hätten auslösen können
ui-replay-sections-fire_chance_eligible_hits_one = 1 Treffer, der einen hätte auslösen können
ui-replay-sections-fire_chance_ships = auf { $ships } Zielschiffen
ui-replay-sections-fire_chance_ships_one = auf 1 Zielschiff
ui-replay-sections-fire_chance_expected_fires = erwartet: { $fires } Brände
ui-replay-sections-fire_chance_per_ship = Pro Zielschiff
ui-replay-sections-fire_chance_no_eligible_hits = keine Treffer, die einen Brand hätten auslösen können
ui-replay-sections-fire_chance_formula = Brandwahrscheinlichkeitsformel des Angreifers
ui-replay-sections-fire_chance_formula_base = Basis-burnProb
ui-replay-sections-fire_chance_formula_clamped = begrenzt
ui-replay-sections-fire_chance_breakdown = Aufschlüsselung
ui-replay-sections-fire_chance_shells_fired = HE-Granaten abgefeuert
ui-replay-sections-fire_chance_shells_fired_one = HE-Granate abgefeuert
ui-replay-sections-fire_chance_he_hits = HE-Treffer auf einem Schiff
ui-replay-sections-fire_chance_he_hits_one = HE-Treffer auf einem Schiff
ui-replay-sections-fire_chance_no_target_ship = HE-Treffer, die keinem Zielschiff zuzuordnen sind
ui-replay-sections-fire_chance_no_target_ship_one = HE-Treffer, der keinem Zielschiff zuzuordnen ist
ui-replay-sections-fire_chance_eligible = hätte einen Brand auslösen können
ui-replay-sections-fire_chance_ribbons = SetFire-Abzeichen
ui-replay-sections-fire_chance_ribbons_one = SetFire-Abzeichen
ui-replay-sections-fire_chance_ribbons_credited = einer Granate zugeordnet
ui-replay-sections-fire_chance_ribbons_uncredited = nicht zugeordnet
ui-replay-sections-fire_chance_unattributed_no_burn_state = keine Änderung des Brandzustands zum Abgleich vorhanden
ui-replay-sections-fire_chance_unattributed_already_credited = ein früherer Brand hat den nahen Treffer bereits verwendet
ui-replay-sections-fire_chance_unattributed_secondary_contest = eine unserer eigenen Sekundärwaffen hätte ihn ebenso auslösen können
ui-replay-sections-fire_chance_unattributed_hit_contest = jeder nahe Treffer von uns war mit einem anderen mehrdeutig
ui-replay-sections-fire_chance_unattributed_all_excluded = jeder nahe Treffer von uns war bereits ausgeschlossen
ui-replay-sections-fire_chance_unattributed_none_could_burn = kein naher Treffer von uns hätte einen Brand auslösen können
ui-replay-sections-fire_chance_unattributed_no_hit = kein Treffer von uns lag im Zeitfenster
ui-replay-sections-fire_chance_not_applicable = Ziel bereits zerstört, nicht anwendbar
ui-replay-sections-fire_chance_copied = Aufschlüsselung der Brandwahrscheinlichkeit kopiert
ui-replay-sections-fire_chance_click_to_copy = Klicken, um diese Aufschlüsselung zu kopieren
ui-replay-sections-fire_chance_exclusion_already_burning = Abschnitt brennt bereits
ui-replay-sections-fire_chance_exclusion_victim_build_unknown = Build des Ziels unbekannt, Brandzonen könnten zusammengefasst sein
ui-replay-sections-fire_chance_exclusion_damage_control_active = Schadenskontrolle aktiv
ui-replay-sections-fire_chance_exclusion_damage_control_unknown = Zustand der Schadenskontrolle unbekannt
ui-replay-sections-fire_chance_exclusion_observation_gap = Beobachtungslücke
ui-replay-sections-fire_chance_exclusion_consumable_unreliable = Modell der Verbrauchsgut-Aktivierung unzuverlässig
ui-replay-sections-fire_chance_exclusion_victim_fate_unknown = Schicksal des Ziels unbekannt
ui-replay-sections-fire_chance_exclusion_hit_type_does_not_roll = Treffertyp würfelt nicht auf Brand
ui-replay-sections-fire_chance_exclusion_no_geometry = keine Geometrie der Brandabschnitte
ui-replay-sections-fire_chance_exclusion_impact_unplaceable = Einschlag konnte auf dem zugeordneten Schiff nicht verortet werden
ui-replay-sections-fire_chance_exclusion_victim_pose_unknown = Position des Ziels beim Einschlag unbekannt
ui-replay-sections-fire_chance_exclusion_ambiguous_hit = mehrdeutig mit einem anderen Treffer von uns
ui-replay-sections-damage_events = Schadensereignisse

## ui.replay.debug
ui-replay-debug-view_raw_metadata = Rohe Spielermetadaten anzeigen
ui-replay-debug-raw_metadata = Rohe Metadaten
ui-replay-debug-raw_results = Rohe Ergebnisse
ui-replay-debug-mapped_results = Zugeordnete Ergebnisse
ui-replay-debug-player_metadata = Spielermetadaten
ui-replay-debug-no_results_packet = Dieses Replay enthält kein Kampfergebnis-Paket
ui-replay-debug-raw_json_tooltip = Die rohen Kampfergebnisse, wie sie von WG serialisiert wurden.
ui-replay-debug-mapped_json_tooltip = Kampfergebnisse mit positionellen Arrays aufgelöst zu benannten Feldern.
ui-replay-debug-results_raw_json = Kampfergebnisse: Roh-JSON
ui-replay-debug-results_mapped_json = Kampfergebnisse: Zugeordnetes JSON
ui-replay-debug-search = Suchen
ui-replay-debug-previous_match = Vorheriger Treffer (Umschalt+Eingabe)
ui-replay-debug-next_match = Nächster Treffer (Eingabe)

## ui.replay.results
ui-replay-results-victory = Sieg
ui-replay-results-defeat = Niederlage
ui-replay-results-draw = Unentschieden

## ui.replay.controls
ui-replay-controls-window_title = Replay-Steuerung
ui-replay-controls-unavailable = Steuerung nicht verfügbar (commands.scheme.xml nicht in den Spieldateien gefunden).

# -- Kollaboration / Sitzung --

## ui.collab
ui-collab-session = Sitzung
ui-collab-starting = Sitzung wird gestartet...
ui-collab-session_active = Sitzung aktiv
ui-collab-connected_to_session = Mit Sitzung verbunden
ui-collab-session_token = Sitzungs-Token:
ui-collab-toggle_visibility = Token-Sichtbarkeit umschalten
ui-collab-copy_token = Token kopieren
ui-collab-token_copied = Token in Zwischenablage kopiert
ui-collab-copy_web_link = Web-Link kopieren
ui-collab-copy_web_link_tooltip = Link für den Web-Client kopieren
ui-collab-web_link_copied = Web-Link in Zwischenablage kopiert
ui-collab-copy_localhost_link = Localhost-Link kopieren
ui-collab-copy_localhost_tooltip = Localhost-Link kopieren (Entwicklung)
ui-collab-localhost_copied = Localhost-Link in Zwischenablage kopiert
ui-collab-connected_count = { $count } verbunden
ui-collab-promote_cohost = Zum Co-Host befördern
ui-collab-promoted_cohost = { $name } zum Co-Host befördert
ui-collab-permissions = Berechtigungen
ui-collab-lock_annotations = Anmerkungen sperren
ui-collab-lock_settings = Einstellungen sperren
ui-collab-reset_overrides = Client-Änderungen zurücksetzen
ui-collab-reset_overrides_tooltip = Alle Änderungen der Client-Anzeigeeinstellungen zurücksetzen
ui-collab-leave = Verlassen
ui-collab-role_host = Host
ui-collab-role_cohost = Co-Host
ui-collab-you = (du)
ui-collab-display_name = Anzeigename:
ui-collab-display_name_hint = Dein Name...
ui-collab-display_name_error = Bitte geben Sie einen Anzeigenamen ein
ui-collab-enter_display_name = Geben Sie zuerst einen Anzeigenamen ein
ui-collab-host_session = Sitzung hosten
ui-collab-start_session = Sitzung starten
ui-collab-join_session = Sitzung beitreten
ui-collab-paste_and_join = Token einfügen und beitreten
ui-collab-clipboard_empty = Zwischenablage ist leer
ui-collab-invalid_token = Ungültiger Token: { $error }
ui-collab-no_runtime = Die Sitzung konnte nicht gestartet werden: keine Laufzeitumgebung
ui-collab-shared_windows = Geteilte Fenster
ui-collab-watching = Zuschauen
ui-collab-waiting_for_frames = Warte auf Gefechtsdaten
ui-collab-open = Öffnen
ui-collab-open_for_everyone = Für alle öffnen
ui-collab-tactics_board = Taktiktafel
ui-collab-max_boards = Maximal { $count } Taktiktafeln geöffnet
ui-collab-waiting_for_data = Warte auf das Laden der Spieldaten...

# -- Kampfergebnis-Text --

## battle
battle-victory = SIEG
battle-defeat = NIEDERLAGE
battle-draw = UNENTSCHIEDEN
battle-battle_starts_in = GEFECHT BEGINNT IN

# -- Beschreibungen des Kampfendes --

## finish_type
finish_type-unknown = Gefecht beendet
finish_type-extermination = Alle feindlichen Schiffe zerstört
finish_type-base_captured = Basis erobert
finish_type-timeout = Zeit abgelaufen
finish_type-failure = Mission fehlgeschlagen
finish_type-technical = Technisches Ende
finish_type-score = Punktelimit erreicht
finish_type-score_on_timeout = Punkteführung bei Zeitablauf
finish_type-pve_success = Mission erfüllt
finish_type-pve_failed = Mission fehlgeschlagen
finish_type-score_zero = Punkte aufgebraucht
finish_type-score_excess = Punktelimit überschritten

# -- Statistik-Tab --

## ui.stats
ui-stats-overview = Übersicht
ui-stats-sample_notice = Beispieldaten, keine aufgezeichneten Spiele
ui-stats-limit = Limit
ui-stats-division = Div.:
ui-stats-div_all = Alle
ui-stats-div_solo = Solo
ui-stats-div_div = Div
ui-stats-filters = Filter
ui-stats-own_filters = Dieses Diagramm separat filtern
ui-stats-own_filters_tooltip = Dieses Diagramm eingrenzen, ohne die Anzeige der anderen Bereiche zu ändern
ui-stats-mode_label = Modus:
ui-stats-clear = Löschen
ui-stats-pr = PR:
ui-stats-mc = MC:
ui-stats-frags_count = { $count } Abschüsse
ui-stats-best_frags = Beste: { $ship } ({ $count } Abschüsse)
ui-stats-max_damage = Max. Schaden: { $ship } ({ $damage })
ui-stats-copy_markdown = Als Markdown kopieren
ui-stats-copy_image = Als Bild kopieren
ui-stats-copy_image_done = Diagramm in die Zwischenablage kopiert
ui-stats-copy_image_failed = Das Diagramm konnte nicht kopiert werden
ui-stats-copy_csv = Als CSV kopieren
ui-stats-remove_games_hint = Alle { $ship }-Spiele entfernen (Strg+Klick zum Überspringen der Bestätigung)
ui-stats-no_stats = Keine Sitzungsstatistiken verfügbar. Spielen Sie zuerst einige Gefechte!
ui-stats-settings = Einstellungen
ui-stats-chart_type = Diagrammtyp
ui-stats-stat_label = Statistik:
ui-stats-chart_line = Linie
ui-stats-chart_bar = Balken
ui-stats-options = Optionen
ui-stats-combined = Kombiniert
ui-stats-rolling_avg = Gleit. Durchschn.
ui-stats-labels = Beschriftungen
ui-stats-ships = Schiffe
ui-stats-filter_ships = Diagrammschiffe filtern
ui-stats-all_ships = Alle
ui-stats-no_ships = Keine
ui-stats-search_ships = Schiffe suchen...
ui-stats-no_ship_matches = Keine Schiffe entsprechen dieser Suche
ui-stats-search_statistics = Statistiken suchen
ui-stats-no_statistic_matches = Keine Statistik entspricht dieser Suche
ui-stats-ship_selection_count = { $selected } von { $total } ausgewählt
ui-stats-all_ships_tooltip = Alle passenden Schiffe auswählen
ui-stats-no_ships_tooltip = Auswahl der passenden Schiffe aufheben
ui-stats-copy_as_image = Als Bild kopieren
ui-stats-limit_to_recent = Auf letzte Gefechte begrenzen
ui-stats-limit_to_recent_tooltip = Das Limit gilt in Schiffe und Diagramme pro Schiff und in der Übersicht für alle Gefechte zusammen.
ui-stats-division_label = Division:
ui-stats-add_chart = Diagramm hinzufügen
ui-stats-clear_confirm = Alle Gefechte löschen?
ui-stats-clear_tooltip = Alle erfassten Gefechte verwerfen
ui-stats-clear_failed = Nicht gelöscht: { $reason }
ui-stats-clear_ship_confirm = Erneut drücken, um die Gefechte dieses Schiffs zu verwerfen
ui-stats-copy_table = Diese Tabelle kopieren
ui-stats-copied = In die Zwischenablage kopiert
ui-stats-no_games = Keine Gefechte für die aktuellen Filter erfasst
ui-stats-nothing_to_plot = Für die aktuellen Filter gibt es nichts darzustellen
ui-stats-column_ship = Schiff
ui-stats-column_battles = Gefechte
ui-stats-column_win_rate = Siegrate
ui-stats-column_avg_damage = Durchschn. Schaden
ui-stats-column_avg_frags = Durchschn. Abschüsse
ui-stats-frags = { $count } Abschüsse
ui-stats-statistic = Statistik
ui-stats-open_directory_stats = Verzeichnisstatistiken
ui-stats-open_directory_stats_tooltip = Statistiken für alle Replays in diesem Verzeichnis und seinen Unterverzeichnissen öffnen
ui-stats-directory_stats_unavailable = Spieldaten werden noch geladen
ui-stats-directory_reading_task = Replays für Verzeichnisstatistiken werden gelesen...
ui-stats-directory_walking = Replays werden gesucht...
ui-stats-directory_scanning = Prüfe { $done } von { $total } Replays...
ui-stats-directory_reading = Lese { $done } von { $total } Replays...
ui-stats-directory_reload = Neu laden
ui-stats-directory_reload_tooltip = Die Replays dieses Verzeichnisses erneut lesen
ui-stats-directory_skipped = { $count } Replays übersprungen
ui-stats-directory_skipped_detail =
    Nicht lesbar: { $unreadable }
    Keine Spieldaten für ihren Build: { $missing_build }
    Keine Kampfergebnisse: { $no_results }
ui-stats-directory_no_games = Kein Replay in diesem Verzeichnis enthält Kampfergebnisse
ui-stats-chart = Diagramm
ui-stats-combine_ships = Schiffe kombinieren
ui-stats-running_average = Gleitender Durchschnitt
ui-stats-value_labels = Wertbeschriftungen
ui-stats-reset_view = Ansicht zurücksetzen
ui-stats-keep_current_layout = Aktuelles Layout behalten
ui-stats-session_title = Sitzungsstatistik
ui-stats-session_manual = Manuell
ui-stats-session_since_opened = Seit Programmstart
ui-stats-session_manual_tooltip = Die Sitzung enthält die Gefechte, die du spielst, und die Replays, die du in der Replay-Liste setzt oder hinzufügst. Sie bleibt zwischen Programmstarts erhalten.
ui-stats-session_since_opened_tooltip = Die Sitzung enthält jedes Gefecht, das um oder nach { $time } begonnen hat, als das Programm gestartet wurde. Deine manuelle Sitzung bleibt erhalten und kehrt zurück, wenn du zu Manuell wechselst.
ui-stats-session_battles_one = 1 Gefecht
ui-stats-session_battles_many = { $count } Gefechte
ui-stats-session_chip_manual = Sitzung: { $battles }
ui-stats-session_chip_since = Sitzung seit { $time }: { $battles }
ui-stats-session_empty_title = Noch keine Gefechte in dieser Sitzung
ui-stats-session_empty_manual = Gespielte Gefechte werden nach ihrem Ende zur Sitzung hinzugefügt. Du kannst Replays auch über das Menü der Replay-Liste setzen oder hinzufügen.
ui-stats-session_empty_since = Gefechte, die nach { $time } beginnen, werden nach ihrem Ende zur Sitzung hinzugefügt.
ui-stats-session_set_add_unavailable = Die Sitzung enthält Gefechte seit { $time }. Wechsle zu Manuell, um Replays zu setzen oder hinzuzufügen.
ui-stats-clear_unavailable_since = Eine Sitzung seit Programmstart wird nicht gespeichert. Wechsle zu Manuell, um die gespeicherte Sitzung zu löschen.
ui-stats-charts_save_failed = Diagrammeinstellungen nicht gespeichert: { $reason }
ui-stats-layout_save_failed = Diagrammlayout nicht gespeichert: { $reason }
ui-stats-axis_game = Gefecht
ui-stats-chart_hover_game = Gefecht { $number }, { $time }
ui-stats-series_average = { $stat } (Durchschnitt)
ui-stats-series_running = { $stat } (gleitend)
ui-stats-series_more = +{ $count } weitere
ui-stats-mode_random = Zufallsgefechte
ui-stats-mode_ranked = Ranglistengefechte
ui-stats-mode_other = Andere
ui-stats-win_rate_rated_only = Die Siegrate zählt nur Zufalls- und Ranglistengefechte
ui-stats-win_rate_breakdown_line = { $mode }: { $rate } (Gefechte: { $battles })
ui-stats-matches = Spiele
ui-stats-column_time = Zeit
ui-stats-column_mode = Modus
ui-stats-column_result = Ergebnis
ui-stats-keep_game = Dieses Spiel in der Statistik zählen
ui-stats-leave_out_game = Dieses Spiel aus der Statistik ausschließen
ui-stats-leave_out_ship = Alle Spiele mit { $ship } ausschließen
ui-stats-leave_out_ship_hint = { $ship } aus der Statistik ausschließen
ui-stats-keep_all_games = Alle Spiele zählen
ui-stats-keep_all_games_tooltip = Alle manuell ausgeschlossenen Spiele wieder zählen
ui-stats-left_out_only = Nur ausgeschlossene
ui-stats-match_count = { $shown } von { $total } Spielen, { $left_out } ausgeschlossen
ui-stats-no_matches = Keine Spiele entsprechen den aktuellen Filtern
ui-stats-search_matches = Nach Schiff suchen...
ui-stats-ship_filter_tooltip = Wählen, welche Schiffe in der Statistik zählen
ui-stats-keep_ships_tooltip = Alle passenden Schiffe zählen
ui-stats-leave_out_ships_tooltip = Alle passenden Schiffe aus der Statistik ausschließen
ui-stats-dates = Datum
ui-stats-all_dates = Alle Daten
ui-stats-clear_dates = Alle Daten anzeigen
ui-stats-games_left_out = { $count } Spiele ausgeschlossen
ui-stats-reset_filters = Filter zurücksetzen
ui-stats-reset_filters_tooltip = Alle Spiele anzeigen: kein Limit, Zeitraum, Division, Modus oder ausgeschlossene Schiffe und Spiele

## ui.stats.table
ui-stats-table-min = Min
ui-stats-table-max = Max
ui-stats-table-total = Gesamt
ui-stats-table-average = Durchschnitt

# -- Spieler-Tracker --

## ui.player_tracker
ui-player_tracker-clear_stats = Statistiken löschen
ui-player_tracker-player_filter = Spielerfilter
ui-player_tracker-populate_from_replays = Daten aus Replays befüllen
ui-player_tracker-find_matches = Gefechte finden
ui-player_tracker-open_wows_numbers = Auf wows-numbers öffnen
ui-player_tracker-open_shipbuilds = Builds auf shipbuilds öffnen
ui-player_tracker-subtab_historical = Historisch
ui-player_tracker-subtab_players = Spieler
ui-player_tracker-filter_hint = Nach Spieler oder Clan filtern...
ui-player_tracker-time_period = Zeitraum
ui-player_tracker-allies = Verbündete
ui-player_tracker-enemies = Gegner
ui-player_tracker-live_no_directory = Legen Sie das World of Warships Verzeichnis fest, um Gefechte zu überwachen
ui-player_tracker-live_checking = Suche nach laufendem Gefecht...
ui-player_tracker-live_none = Kein Gefecht im Gange
ui-player_tracker-live_loading_build = Schiffsdaten dieses Builds werden geladen. Namen und Klassen erscheinen, sobald sie verfügbar sind.
ui-player_tracker-waiting_for_index = Warte auf den Replay-Index
ui-player_tracker-loading_clans = Clans werden geladen...
ui-player_tracker-loading_players = Spieler werden geladen...
ui-player_tracker-index_failed = Index konnte nicht gelesen werden: { $reason }
ui-player_tracker-no_players = Keine Spieler für diesen Zeitraum indexiert
ui-player_tracker-notes_title = Notizen: { $name }
ui-player_tracker-note_not_saved = Nicht gespeichert: { $reason }
ui-player_tracker-subtab_current_match = Aktuelles Gefecht
ui-player_tracker-your_team = Ihr Team ({ $count })
ui-player_tracker-enemy_team = Gegnerisches Team ({ $count })
ui-player_tracker-no_live_match = Kein Gefecht im Gange. Die Aufstellung erscheint, sobald Sie ein Gefecht betreten.
ui-player_tracker-encounters_hover = Insgesamt { $total } Mal gesehen, { $range } im gewählten Zeitraum. Zuletzt gesehen { $last }.
ui-player_tracker-never_encountered = Noch nie gesehen
ui-player_tracker-copy_wg_id = Klicken, um die WG-ID zu kopieren
ui-player_tracker-copied_wg_id = WG-ID { $id } kopiert
ui-player_tracker-aliases_hover = Auch bekannt als: { $names }
ui-player_tracker-notes_hint = Notizen zu diesem Spieler
ui-player_tracker-arena_count = { $count } Gefechte aufgezeichnet
ui-player_tracker-last_encountered_exact = Zuletzt begegnet { $timestamp }
ui-player_tracker-subtab_clans = Clans
ui-player_tracker-clan_no_data = Noch keine Clan-Begegnungen aufgezeichnet.
ui-player_tracker-find_clan_matches = Gefechte finden, in denen dieses Kürzel vorkommt. Der Index hat kein reines Clan-Feld, daher passen auch Spielernamen, die das Kürzel enthalten.
ui-player_tracker-clan_sightings_hover = { $range } davon fallen in den gewählten Zeitraum.
ui-player_tracker-clan_member_matches = in { $count } Gefechten gesehen
ui-player_tracker-show_division_mates = Divisionspartner anzeigen
ui-player_tracker-show_division_mates_hover = Die Gefechte mitzählen, die Sie in einer Division gespielt haben. Standardmäßig aus, da Sie diesen Spielern dort nur begegnet sind, weil Sie sie eingeladen haben. Andere Gefechte gegen dieselben Spieler werden immer gezählt.
ui-player_tracker-seen_in_twitch_chat = Im Twitch-Chat gesehen
ui-player_tracker-seen_in_twitch_chat_hover = Nur die Spieler anzeigen, deren Name während eines Gefechts mit Ihnen einem Login im Twitch-Chat entsprach.
ui-player_tracker-twitch_seen_hover = Im Twitch-Chat als { $logins } in { $count } Gefechten mit Ihnen gesehen
ui-player_tracker-win_rate_overall = Gesamt
ui-player_tracker-win_rate_ship = Schiff
ui-player_tracker-view_compact = Kompakt
ui-player_tracker-view_detailed = Detailliert
ui-player_tracker-ship_stats = Schiffsstatistiken
ui-player_tracker-win_rate_mode_hover = Account-Statistiken oder Statistiken dieses Schiffs anzeigen. Zeilenfarbe, PR, Gefechtsanzahl und Durchschnittsschaden richten sich danach.
ui-player_tracker-stats_resolving = Gefechtsaufstellung wird gelesen...
ui-player_tracker-stats_fetching = Spielerstatistiken werden abgerufen...
ui-player_tracker-stats_rate_limited = Ratenbegrenzung, erneuter Versuch in { $wait }
ui-player_tracker-stats_unavailable = Statistiken nicht verfügbar: { $reason }
ui-player_tracker-roster_unavailable = Gefechtsaufstellung konnte nicht aus dem Replay gelesen werden
ui-player_tracker-stats_hidden = Das Profil dieses Spielers ist privat
ui-player_tracker-stats_no_data = Keine Statistiken für diesen Spieler
ui-player_tracker-stats_unplayed_ship = Keine Gefechte mit diesem Schiff aufgezeichnet
ui-player_tracker-pr_overall_hover = Account-PR über alle Schiffe
ui-player_tracker-pr_ship_hover = PR mit diesem Schiff
ui-player_tracker-avg_damage = Durchschn. Schaden
ui-player_tracker-win_rate_hover_line = { $label }: { $rate } über { $battles } Gefechte
ui-player_tracker-win_rate_hover_line_damage = { $label }: { $rate } über { $battles } Gefechte, { $damage } durchschn. Schaden
ui-player_tracker-team_average_win_rate = Durchschn. SR: { $rate }
ui-player_tracker-team_players = { $count } Spieler
ui-player_tracker-row_win_rate = SR { $rate }
ui-player_tracker-row_personal_rating = PR { $value }
ui-player_tracker-row_battles = { $count } Gefechte
ui-player_tracker-row_seen = Gesehen x{ $total }
ui-player_tracker-debug_replay_picker = Debug: ein Replay als aktuelles Gefecht behandeln
ui-player_tracker-debug_replay_picker_hover = Lädt die Aufstellung eines beendeten Replays in diesen Tab, damit der Statistikpfad außerhalb eines Gefechts getestet werden kann.
ui-player_tracker-debug_replay_none = Replay auswählen

## ui.player_tracker.column
ui-player_tracker-column-win_rate = SR
ui-player_tracker-column-personal_rating = PR
ui-player_tracker-column-battles = Gefechte
ui-player_tracker-column-avg_damage = Schaden
ui-player_tracker-column-encounters = Gesehen
ui-player_tracker-column-clan = Clan
ui-player_tracker-column-player_name = Spielername
ui-player_tracker-column-total_encounters = Begegnungen gesamt
ui-player_tracker-column-encounters_in_range = Begegnungen im Zeitraum
ui-player_tracker-column-last_encountered = Zuletzt begegnet
ui-player_tracker-column-members = Mitglieder
ui-player_tracker-column-matches = Gefechte
ui-player_tracker-column-sightings = Sichtungen

## ui.player_tracker.period
ui-player_tracker-period-past_hour = Letzte Stunde
ui-player_tracker-period-past_six_hours = Letzte 6 Stunden
ui-player_tracker-period-past_day = Letzte 24 Stunden
ui-player_tracker-period-past_week = Letzte Woche
ui-player_tracker-period-past_month = Letzter Monat
ui-player_tracker-period-all_time = Gesamte Zeit

# -- Mod-Verwaltung --


## ui.twitch
ui-twitch-possible_name = Möglicher Stream-Name: { $name }
ui-twitch-seen_minutes = Gesehen: { $minutes } Minuten nach Gefechtsstart
ui-twitch-click_to_copy = Klicken zum Kopieren
ui-twitch-copied = { $name } kopiert
## ui.mod_manager
ui-mod_manager-category_filters = Kategoriefilter
ui-mod_manager-enabled = Aktiviert
ui-mod_manager-mod_home = Mod-Homepage
ui-mod_manager-discord_thread = Discord-Genehmigungsthread
ui-mod_manager-save_config = Mod-Konfiguration speichern
ui-mod_manager-load_config = Mod-Konfiguration laden
ui-mod_manager-filter = Filter

## ui.mod_manager.column
ui-mod_manager-column-enabled = Aktiviert
ui-mod_manager-column-name = Name
ui-mod_manager-column-author = Autor
ui-mod_manager-column-category = Kategorie

# -- Replay-Renderer --

## ui.renderer
ui-renderer-loading = Replay-Daten werden geladen...
ui-renderer-preparing_export = Export wird vorbereitet...
ui-renderer-encoding = Kodierung
ui-renderer-muxing = Muxing

## ui.renderer.advantage
ui-renderer-advantage-breakdown = Vorteilsaufschlüsselung
ui-renderer-advantage-team_eliminated = Ein Team wurde ausgeschaltet
ui-renderer-advantage-score_projection = Punkteprognose
ui-renderer-advantage-fleet_power = Flottenstärke
ui-renderer-advantage-strategic_threat = Strategische Bedrohung
ui-renderer-advantage-total = Gesamt
ui-renderer-advantage-hp_incomplete = HP/Schiffsdaten unvollständig — nur Punktefaktoren werden angezeigt

## ui.renderer.context
ui-renderer-context-show_trail = Spur anzeigen
ui-renderer-context-disable_other_trails = Alle anderen Spuren deaktivieren
ui-renderer-context-ranges = Reichweiten
ui-renderer-context-detection = Erkennung
ui-renderer-context-main_battery = Hauptbatterie
ui-renderer-context-secondary = Sekundärbewaffnung
ui-renderer-context-torpedo = Torpedo
ui-renderer-context-radar = Radar
ui-renderer-context-hydro = Hydro
ui-renderer-context-enable_all = Alle aktivieren
ui-renderer-context-disable_all = Alle deaktivieren
ui-renderer-context-disable_other_ranges = Alle anderen Reichweiten deaktivieren
ui-renderer-context-enable_all_ranges = Reichweiten aller Schiffe aktivieren
ui-renderer-context-show_armor = Echtzeit-Panzerung anzeigen
ui-renderer-context-realtime_armor = Echtzeit-Panzerung anzeigen

## ui.renderer.controls
ui-renderer-controls-jump_to_start = Zum Anfang springen
ui-renderer-controls-previous_event = Vorheriges Ereignis (Shift+Left)
ui-renderer-controls-back_10s = 10s zurück (Left)
ui-renderer-controls-pause = Pause (Space)
ui-renderer-controls-play = Abspielen (Space)
ui-renderer-controls-forward_10s = 10s vorwärts (Right)
ui-renderer-controls-next_event = Nächstes Ereignis (Shift+Right)
ui-renderer-controls-jump_to_end = Zum Ende springen
ui-renderer-controls-save_as_video = Als Video speichern
ui-renderer-controls-copy_to_clipboard = Video in Zwischenablage rendern
ui-renderer-controls-speed = Wiedergabegeschwindigkeit

## ui.renderer.settings
ui-renderer-settings-title = Anzeigeeinstellungen
ui-renderer-settings-hp_bars = HP-Balken
ui-renderer-settings-ship_settings = Schiffseinstellungen
ui-renderer-settings-armament = Bewaffnung
ui-renderer-settings-dead_ships = Zerstörte Schiffe
ui-renderer-settings-dead_ship_names = Namen zerstörter Schiffe
ui-renderer-settings-player_names = Spielernamen
ui-renderer-settings-ship_ranges = Schiffsreichweiten
ui-renderer-settings-self_ship_ranges = Eigene Schiffsreichweiten
ui-renderer-settings-ship_names = Schiffsnamen
ui-renderer-settings-camera_direction = Turmrichtung
ui-renderer-settings-trail_settings = Spureinstellungen
ui-renderer-settings-heat_trail = Wärmespur
ui-renderer-settings-speed_trails = Geschwindigkeitsspuren
ui-renderer-settings-dead_ship_trails = Spuren zerstörter Schiffe
ui-renderer-settings-map_settings = Karteneinstellungen
ui-renderer-settings-buildings = Gebäude
ui-renderer-settings-capture_points = Eroberungspunkte
ui-renderer-settings-consumables = Verbrauchsgüter
ui-renderer-settings-planes = Flugzeuge
ui-renderer-settings-smoke = Rauch
ui-renderer-settings-torpedoes = Torpedos
ui-renderer-settings-tracers = Leuchtspur
ui-renderer-settings-hud_settings = HUD-Einstellungen
ui-renderer-settings-battle_result = Kampfergebnis
ui-renderer-settings-buff_counters = Buff-Zähler
ui-renderer-settings-chat_label = Chat
ui-renderer-settings-kill_feed = Abschussliste
ui-renderer-settings-score_label = Punktestand
ui-renderer-settings-score_timers = Punkte-Timer
ui-renderer-settings-team_advantage = Teamvorteil
ui-renderer-settings-team_rosters = Teamaufstellungen
ui-renderer-settings-stats_panel = Statistikpanel
ui-renderer-settings-timer = Timer
ui-renderer-settings-export_settings = Export-Einstellungen
ui-renderer-settings-prefer_cpu = CPU-Encoder bevorzugen
ui-renderer-settings-prefer_cpu_tooltip = Software-Encoder (CPU) anstelle von GPU für den Videoexport verwenden
ui-renderer-settings-include_pre_battle = Vorgefechtsphase einbeziehen
ui-renderer-settings-include_pre_battle_tooltip = Spawn und Countdown vor dem Gefechtsbeginn im exportierten Video einbeziehen
ui-renderer-settings-export_timing = Videotempo
ui-renderer-settings-export_timing_length = An Länge anpassen
ui-renderer-settings-export_timing_speed = Geschwindigkeit
ui-renderer-settings-export_timing_result_speed = Läuft mit etwa { $speed }x
ui-renderer-settings-export_timing_result_length = Video dauert { $length }
ui-renderer-settings-codec = Codec
ui-renderer-settings-codec_auto = Automatisch
ui-renderer-settings-save_defaults = Standardwerte speichern
ui-renderer-settings-save_defaults_done = Als Starteinstellungen des Renderers gespeichert
ui-renderer-settings-save_defaults_failed = Die Standardwerte konnten nicht gespeichert werden
ui-renderer-settings-event_timeline = Ereigniszeitleiste

# -- Datei-Entpacker --

## ui.unpacker
ui-unpacker-assets_bin = Assets.bin
ui-unpacker-loading_assets = Lade assets.bin...
ui-unpacker-load_failed = Laden fehlgeschlagen: { $error }
ui-unpacker-no_game_data = Keine Spieldaten geladen. Setzen Sie Ihr World of Warships Verzeichnis in den Einstellungen.
ui-unpacker-filter_files = Dateien filtern...
ui-unpacker-search_in_files = In Dateien suchen
ui-unpacker-search_hint = In Dateien suchen...
ui-unpacker-path_filter_hint = Pfadfilter (z.B. *.xml)
ui-unpacker-queue_all = Alle einreihen
ui-unpacker-queue_all_files = Alle Dateien einreihen
ui-unpacker-empty_directory = Leeres Verzeichnis
ui-unpacker-stop_search_tooltip = Suche stoppen
ui-unpacker-no_matches = Keine Treffer gefunden.
ui-unpacker-view_as_json = Als JSON anzeigen
ui-unpacker-extract_as_json = Als JSON extrahieren
ui-unpacker-view_contents = Inhalt anzeigen
ui-unpacker-version = Version:
ui-unpacker-game_params = GameParams
ui-unpacker-base_as_json = Basis als JSON
ui-unpacker-as_json = Als JSON
ui-unpacker-as_cbor = Als CBOR
ui-unpacker-as_json_minimal = Als JSON (Minimal / Transformiert)
ui-unpacker-as_cbor_minimal = Als CBOR (Minimal / Transformiert)
ui-unpacker-decode_prototypes = Prototypen als JSON dekodieren
ui-unpacker-decode_prototypes_tooltip = Unterstützte Assets.bin-Prototypen (.visual, .model, .mfm) beim Extrahieren als JSON dekodieren
ui-unpacker-extraction_queue = Extraktionswarteschlange
ui-unpacker-collapse_folders = Ordnerbaum einklappen
ui-unpacker-expand_folders = Ordnerbaum ausklappen
ui-unpacker-clear_all = Alle löschen
ui-unpacker-browse = Durchsuchen
ui-unpacker-output_dir_hint = Ausgabeverzeichnis...
ui-unpacker-go_to_directory = Zum Verzeichnis gehen
ui-unpacker-remove_from_queue = Aus Warteschlange entfernen
ui-unpacker-queue_folder = Gesamten Ordner einreihen
ui-unpacker-queue_file = Diese Datei einreihen
ui-unpacker-extraction_output = Extrahieren nach
ui-unpacker-queue_empty = Markieren Sie eine Datei oder einen Ordner in der Liste, um sie einzureihen
ui-unpacker-show_queue = Extraktionswarteschlange
ui-unpacker-double_click_open = Doppelklicken zum Öffnen
ui-unpacker-packages = Pakete
ui-unpacker-item_count = { $count } Elemente
ui-unpacker-search_title = Suche: { $query }
ui-unpacker-search_in = in { $source }
ui-unpacker-image_decode_failed = Dieses Bild konnte nicht dekodiert werden: { $reason }
ui-unpacker-find = Suchen
ui-unpacker-find_previous = Vorheriger Treffer
ui-unpacker-find_next = Nächster Treffer
ui-unpacker-find_close = Suche schließen
ui-unpacker-find_count = { $current } von { $total }
ui-unpacker-find_none = Keine Treffer
ui-unpacker-find_match_case = Groß-/Kleinschreibung beachten
ui-unpacker-queue = Warteschlange
ui-unpacker-reveal_in_tree = Im Paketbaum anzeigen
ui-unpacker-hit_offset = bei { $offset }
ui-unpacker-queued_count = { $count } eingereiht
ui-unpacker-counting_files = Dateien werden gezählt...
ui-unpacker-extracting = Extrahiere { $done } von { $total }
ui-unpacker-extract_failed = Extraktion fehlgeschlagen: { $reason }
ui-unpacker-extracted = { $count } Dateien extrahiert
ui-unpacker-cancelled = Nach { $count } Dateien abgebrochen
ui-unpacker-extract = Extrahieren
ui-unpacker-extract_one = 1 Element extrahieren
ui-unpacker-extract_many = { $count } Elemente extrahieren
ui-unpacker-choose_directory = Wählen Sie zuerst ein Zielverzeichnis für die Extraktion
ui-unpacker-dump_parameters = GameParams exportieren
ui-unpacker-dump_parameters_tooltip = GameParams.data dieses Builds exportieren
ui-unpacker-load_a_build = Laden Sie zuerst einen Build
ui-unpacker-parameters_written = Parameter geschrieben
ui-unpacker-save_parameters_title = Spielparameter speichern
ui-unpacker-writing = Wird geschrieben...
ui-unpacker-prototype_written = Prototyp geschrieben
ui-unpacker-whole_tree = Gesamter Baum
ui-unpacker-base_only = Nur Basisparameter

## ui.unpacker.column
ui-unpacker-column-name = Name
ui-unpacker-column-size = Größe
ui-unpacker-column-type_col = Typ
ui-unpacker-column-path = Pfad
ui-unpacker-column-file = Datei
ui-unpacker-column-offset = Offset
ui-unpacker-column-context = Kontext

# -- Panzerungsansicht --

## ui.armor
ui-armor-load_settings = Laden Sie Spieldaten in den Einstellungen, um die Panzerungsansicht zu verwenden.
ui-armor-gpu_unavailable = GPU-Renderstatus nicht verfügbar.
ui-armor-loading_ship = Schiffsdaten werden geladen...
ui-armor-load_failed = Laden der Schiffsdaten fehlgeschlagen: { $error }
ui-armor-ship_load_failed = { $ship } konnte nicht geladen werden: { $reason }
ui-armor-empty = Leer
ui-armor-mirror_cameras = Kameras spiegeln
ui-armor-sync_options = Optionen synchronisieren
ui-armor-show_armor_by_default = Panzerung standardmäßig anzeigen
ui-armor-common_settings = Gemeinsame Einstellungen
ui-armor-tier_filter = Stufen { $lowest } bis { $highest }
ui-armor-fortifications = Befestigungen
ui-armor-show_hidden_ships = Ausgeblendete Schiffe zeigen
ui-armor-show_hidden_ships_tooltip = Auch Schiffe auflisten, die die Liste auslässt, etwa Clan-Leihschiffe
ui-armor-sync_tooltip = Panzerungs-/Rumpfsichtbarkeit und Anzeige-Overlays (Schiffsmitte, Wasserlinie, Kameraringe) über alle Fenster synchron halten
ui-armor-compare_split = In neuem Split vergleichen
ui-armor-open_in_new_tab = In neuem Tab öffnen
ui-armor-export_model = Schiffsmodell exportieren
ui-armor-armor_thickness = Panzerungsdicke
ui-armor-export_disclaimer = Diese 3D-Modelle und Texturen sind geistiges Eigentum von Wargaming, und jede Nutzung dieser Modelle sollte den geltenden Gesetzen Ihres Landes entsprechen.
ui-armor-export_button = Exportieren
ui-armor-armor_toggle = Panzerung
ui-armor-armor_tooltip = Panzerungszonen ein-/ausblenden
ui-armor-hull_toggle = Rumpf
ui-armor-hull_tooltip = Rumpfmodellteile ein-/ausblenden
ui-armor-splash_toggle = Splash
ui-armor-splash_tooltip = Splash-Box ein-/ausblenden
ui-armor-show_hidden = Versteckte anzeigen
ui-armor-show_hidden_tooltip = (Möglicherweise) versteckte Platten ein-/ausblenden
ui-armor-gaps = Lücken
ui-armor-gaps_tooltip = Grenzflächen hervorheben, an denen Panzerplatten nicht verbunden sind (mögliche Lücken)
ui-armor-display = Anzeige
ui-armor-display_tooltip = Anzeigeeinstellungen (Ctrl+S)
ui-armor-marker_opacity = Markierungsdeckkraft
ui-armor-save_defaults = Als Standard speichern
ui-armor-export_tab = Export
ui-armor-export_tooltip = Schiffsmodell als OBJ-Datei exportieren
ui-armor-pen_check = Durchschlagspr.
ui-armor-pen_check_tooltip = Granaten für Durchschlagsvergleich konfigurieren
ui-armor-trajectory = Flugbahn
ui-armor-trajectory_on = Flugbahn [AN]
ui-armor-trajectory_range = Entfernung
ui-armor-trajectory_clear = Alle löschen
ui-armor-arc_heading = Flugbahn { $number }
ui-armor-arc_summary = { $hits } Treffer, { $armor } mm auf { $range } km
ui-armor-arc_delete = Diese Flugbahn löschen
ui-armor-arc_isolate_plates = Platten isolieren
ui-armor-arc_isolate_plates_tooltip = Nur die Panzerplatten anzeigen, die diese Flugbahn durchquert hat
ui-armor-arc_isolate_zones = Zonen isolieren
ui-armor-arc_isolate_zones_tooltip = Alle Teile der Panzerungszonen anzeigen, in die diese Flugbahn eingedrungen ist
ui-armor-arc_simulation_caveat = Diese Simulation basiert auf per Reverse Engineering gewonnenen Daten und entspricht möglicherweise nicht der Ballistikberechnung des Spiels.
ui-armor-arc_no_shell = Ohne Granate berechnet, die durchquerten Platten werden ohne Granate angezeigt
ui-armor-arc_detonated_in = Detoniert in { $zone }
ui-armor-arc_detonated_past_armor = Detoniert hinter der gesamten Panzerung
ui-armor-arc_ricochet = Abpraller an { $plate }
ui-armor-arc_shatter = Zerschellt an { $plate }
ui-armor-arc_stopped = Gestoppt an { $plate }
ui-armor-arc_overpen = Durchschuss
ui-armor-arc_overpen_unarmed = Durchschuss (Zünder nie scharf)
ui-armor-arc_plate_unknown = einer nicht erfassten Platte
ui-armor-angle_good = unter 30 Grad
ui-armor-angle_mid = 30 bis 45 Grad
ui-armor-angle_bad = über 45 Grad
ui-armor-trajectory_tooltip = Auf Panzerung klicken, um Granatflugbahnen zu simulieren (Ctrl+T)
ui-armor-trajectory_ship = Schiff:
ui-armor-trajectory_select_ship = Schiff auswählen
ui-armor-trajectory_ship_tooltip = Das Schiff wählen, das die Granate für die Flugbahn liefert
ui-armor-trajectory_add_ship = Feuerndes Schiff hinzufügen
ui-armor-trajectory_add_ship_tooltip = Durchschlagsprüfung öffnen, um ein Schiff zu suchen und hinzuzufügen
ui-armor-trajectory_shell = Granate:
ui-armor-trajectory_select_shell = Granate auswählen
ui-armor-trajectory_shell_tooltip = Die Granate für Flugbahnberechnungen wählen
ui-armor-continue_ricochet = Nach Abpraller fortsetzen
ui-armor-splash_mode = Splash
ui-armor-splash_on = Splash [AN]
ui-armor-splash_no_data = Keine Splash-Daten für dieses Schiff
ui-armor-splash_no_shells = Fügen Sie ein Schiff mit HE/SAP-Granaten zur Vergleichsliste hinzu
ui-armor-splash_tooltip_mode = Auf Panzerung klicken, um das HE-Splash-Schadensvolumen zu visualisieren
ui-armor-loading_armor = Schiff wird geladen...
ui-armor-select_ship = Wählen Sie ein Schiff aus der Liste
ui-armor-pen_check_label = Durchschlagsprüfung
ui-armor-all_btn = Alle
ui-armor-none_btn = Keine
ui-armor-opaque = Undurchsichtig
ui-armor-upgrade = Aufrüstung:
ui-armor-camo = Tarnung:
ui-armor-camo_none = Standard (keine)
ui-armor-camo_group_universal = Universaltarnungen
ui-armor-camo_group_expendable = Verbrauchstarnungen
ui-armor-camo_group_other = Andere Tarnungen
ui-armor-show_armor_thickness = Legende zur Panzerungsdicke anzeigen
ui-armor-plate_edges = Plattenkanten
ui-armor-waterline = Wasserlinie
ui-armor-camera_rings = Kameraringe
ui-armor-realtime_readout = { $health } HP  -  { $hits } Treffer
ui-armor-show_camera_rings = Kameraringe anzeigen
ui-armor-camera_fov = Kamera-Sichtfeld
ui-armor-camera_height = Kamerahöhe
ui-armor-show_camera_zoom_path = Zoompfad anzeigen
ui-armor-zoom_path_regular_fov = Normales Sichtfeld
ui-armor-zoom_path_max_fov = Max. Sichtfeld
ui-armor-camera_perspective = Kameraperspektive
ui-armor-perspective_fov = Ansichts-Sichtfeld
ui-armor-perspective_projection = Projektion
ui-armor-perspective_proj_game = Spiel
ui-armor-perspective_proj_center = Durch die Mitte
ui-armor-ship_center = Schiffsmitte
ui-armor-grid = Raster
ui-armor-length_marks = Längenmarken
ui-armor-length_readout = Länge { $length }, Breite { $beam }
ui-armor-length_metres = { $value } m
ui-armor-length_centimetres = { $value } cm
ui-armor-filter_options = Optionen filtern...
ui-armor-filter_zones = Zonen filtern...
ui-armor-filter_parts = Teile filtern...
ui-armor-tree_matches = Treffer: { $count }
ui-armor-tree_no_matches = Keine Treffer
ui-armor-no_matching_options = Keine Optionen passen zum Filter
ui-armor-opacity = Deckkraft
ui-armor-ship_hull = Schiffsrumpf
ui-armor-opaque_hull = Undurchsichtiger Rumpf
ui-armor-lighting = Rumpfbeleuchtung
ui-armor-lighting_enabled = Beleuchtung aktivieren
ui-armor-lighting_preset_ingame = Im Spiel
ui-armor-lighting_preset_flat = Flach
ui-armor-lighting_preset_studio = Studio
ui-armor-lighting_flat = Flach (Umgebung)
ui-armor-lighting_intensity = Lichtintensität
ui-armor-lighting_azimuth = Lichtazimut
ui-armor-lighting_elevation = Lichthöhe
ui-armor-lighting_rim = Randlicht
ui-armor-lighting_specular = Glanzlicht
ui-armor-lighting_shininess = Glanzschärfe
ui-armor-lighting_key_color = Hauptlichtfarbe
ui-armor-lighting_ambient_color = Umgebungslichtfarbe
ui-armor-pop_out = Ausklappen
ui-armor-zero_mm_plates = 0 mm Platten
ui-armor-armor_opacity = Panzerungsdeckkraft
ui-armor-ballistic_disclaimer = Ballistische Ergebnisse basieren auf Reverse Engineering/Schätzungen der Simulationsfunktionsweise
ui-armor-roll = Krängung
ui-armor-ctrl_click_solo = Strg+Klick zum Isolieren
ui-armor-reset_plates = Platten zurücksetzen
ui-armor-visibility = Sichtbarkeit
ui-armor-armor_options = Panzerung
ui-armor-no_ship_loaded = Kein Schiff geladen
ui-armor-lod = LOD:
ui-armor-lod_highest = 0 (höchste)
ui-armor-camouflage = Tarnung
ui-armor-search_ships = Schiffe suchen...
ui-armor-ships = Schiffe
ui-armor-compare = Vergleichen
ui-armor-compare_tooltip = Neuen Vergleichsbereich öffnen, oder per Rechtsklick ein bestimmtes Schiff vergleichen
ui-armor-mirror_cameras_tooltip = Die Kameras aller Bereiche gemeinsam bewegen
ui-armor-show_armor_by_default_tooltip = Modelle mit sichtbarer Panzerung statt des Rumpfs öffnen
ui-armor-xray_highlight = Röntgen-Hervorhebung beim Überfahren der Ansicht
ui-armor-xray_always = Immer
ui-armor-xray_while_held = Beim Halten von { $key }
ui-armor-xray_off = Aus
ui-armor-stack_panes = Bereiche stapeln
ui-armor-stack_panes_tooltip = Vergleichsbereiche übereinander statt nebeneinander anordnen
ui-armor-viewports = Ansichten
ui-armor-options_panel = Optionsbereich
ui-armor-options_pane = Bereich { $number } von { $count }
ui-armor-splash_boxes = Splash-Boxen
ui-armor-cast_here_hint = { $key } halten und klicken, um hier eine Granate abzufeuern
ui-armor-trajectory_next_cast = Nächster Schuss
ui-armor-arc_pick_hint = Auf eine Markierung klicken, um ihre Flugbahn zu bearbeiten.
ui-armor-arc_marker_tooltip = Flugbahn { $number }. Klicken zum Bearbeiten. Rechtsklick zum Entfernen.
ui-armor-zone_toggle_tooltip = Blendet alle Platten dieser Zone ein oder aus. { $key } halten, um nur diese Zone anzuzeigen.
ui-armor-mode_solo_zone = nur diese Zone
ui-armor-mode_cast_trajectory = Flugbahn abfeuern
ui-armor-loading_catalog = Schiffskatalog wird geladen...
ui-armor-catalog_failed = Schiffskatalog konnte nicht geladen werden: { $reason }
ui-armor-legend_collapse = Einklappen
ui-armor-legend_hide = Legende ausblenden
ui-armor-armor_thickness_title = Panzerungsdicke
ui-armor-initializing = 3D-Ansicht wird initialisiert...
ui-armor-init_failed = Panzerungsansicht konnte nicht initialisiert werden: { $reason }
ui-armor-loading_named_ship = { $ship } wird geladen...
ui-armor-rendering = Rendern...
ui-armor-options = Optionen

## ui.armor.compass
ui-armor-compass-north = N
ui-armor-compass-east = O
ui-armor-compass-south = S
ui-armor-compass-west = W

## ui.armor.realtime
ui-armor-realtime-loading = Schiffspanzerung wird geladen...
ui-armor-realtime-waiting = Warte auf Schiffsdaten...
ui-armor-realtime-incoming_fire = Eingehender Beschuss
ui-armor-realtime-attacker_filter = Angreiferfilter
ui-armor-realtime-all_enemies = Alle Feinde
ui-armor-realtime-show_secondaries = Sekundärbewaffnung anzeigen
ui-armor-realtime-auto_scroll = Automatisches Scrollen
ui-armor-realtime-seek_to_salvo = Zur Salve springen
ui-armor-realtime-deselect = Abwählen
ui-armor-realtime-select_all = Alle auswählen
ui-armor-realtime-no_data = Granatendaten nicht mehr verfügbar
ui-armor-realtime-sim_agrees = Simulation stimmt überein
ui-armor-realtime-sim_unchecked = { $count } Granaten konnten nicht simuliert werden
ui-armor-realtime-sim_rng_zone = Zufallsbereich ({ $angle } deg in { $from }-{ $to })
ui-armor-realtime-sim_disagrees = Simulation: { $sim } / Server: { $server }
ui-armor-realtime-no_armor_hit = Keine Panzerplatten getroffen
ui-armor-realtime-tracked = { $salvos } Salven, { $shells } Granaten erfasst
ui-armor-realtime-nothing_landed = Dieses Schiff wurde noch nicht getroffen
ui-armor-realtime-shells = { $count }x
ui-armor-realtime-salvo_shell = { $caliber } mm { $shell }
ui-armor-realtime-salvo_range = { $km } km
ui-armor-realtime-unknown_attacker = Schiff { $id }
ui-armor-realtime-server_outcome = Server: { $outcome }
ui-armor-realtime-not_cast = Diese Granate kann nicht durch den Rumpf verfolgt werden: Ihr Flug oder die Position des Schiffs wurde nicht aufgezeichnet
ui-armor-realtime-pick_hint = Salve oder Granate anklicken, um sie durch den Rumpf zu verfolgen

## ui.armor.plates
ui-armor-plates-heading = Getroffene Platten ({ $count })
ui-armor-plates-arrival = { $velocity } m/s beim Aufprall, { $time } s Flugzeit, Fallwinkel { $fall } deg
ui-armor-plates-overmatch = Overmatch: { $penetration } mm Durchschlag bei { $velocity } m/s
ui-armor-plates-penetrated = Durchschlägt: { $penetration } mm gegen { $effective } mm effektiv bei { $velocity } m/s
ui-armor-plates-ricochet = Abpraller bei { $angle } deg
ui-armor-plates-shatter = Zerschellt: { $penetration } mm gegen { $effective } mm effektiv
ui-armor-plates-fuse_armed = Zünder scharf
ui-armor-plates-detonation = Explodiert in { $zone }, { $distance } m nach Platte #{ $number }
ui-armor-plates-detonation_past_armor = Explodiert hinter aller Panzerung, { $distance } m nach Platte #{ $number }
ui-armor-plates-flat_penetrates = { $ammo } durchschlägt: { $penetration } mm gegen { $thickness } mm
ui-armor-plates-flat_shatters = { $ammo } zerschellt: { $penetration } mm gegen { $thickness } mm
ui-armor-plates-server_angle = Server { $angle } deg

# -- Vorteilsstufen-Bezeichnungen --

## advantage
advantage-absolute = Absolut
advantage-strong = Stark
advantage-moderate = Mäßig
advantage-weak = Schwach

## ui.search
ui-search-undo = Letzte Änderung an der Abfrage rückgängig machen
ui-search-edit_value = Wert bearbeiten
ui-search-add_filter = Filter hinzufügen
ui-search-redo = Rückgängig gemachte Änderung wiederherstellen
ui-search-pill_select = Auswählen
ui-search-pill_deselect = Abwählen
ui-search-pill_group_all = Mit UND gruppieren
ui-search-pill_group_any = Mit ODER gruppieren
ui-search-pill_ungroup = Gruppierung aufheben
ui-search-pill_negate = Negieren
ui-search-pill_flip_connector = UND/ODER umschalten
ui-search-pill_delete = Löschen
ui-search-match_count_truncated = { $count }+ Gefechte
ui-search-outcome_win = Sieg
ui-search-outcome_loss = Niederlage
ui-search-outcome_draw = Unentschieden
ui-search-outcome_unknown = Unbekannt
ui-search-bool_true = wahr
ui-search-bool_false = falsch
ui-search-match_count = { $count } Gefecht(e)
ui-search-row_actions_hint = Replay-Aktionen (oder Rechtsklick auf die Zeile)
ui-search-looking_up_values = Werte werden nachgeschlagen...
ui-search-type_a_query = Abfrage eingeben und Enter drücken
ui-search-parse_failed = Diese Abfrage konnte nicht ausgewertet werden: { $reason }
ui-search-searching = Suche läuft...
ui-search-failed = Die Suche ist fehlgeschlagen: { $reason }
ui-search-no_matches = Keine Treffer
ui-search-game_mode_gap_hint_one = Bei 1 indexierten Gefecht ist kein Spielmodus erfasst. Führen Sie in den Einstellungen "Alle Replays neu indexieren" aus, um ihn nachzutragen.
ui-search-game_mode_gap_hint = Bei { $count } indexierten Gefechten ist kein Spielmodus erfasst. Führen Sie in den Einstellungen "Alle Replays neu indexieren" aus, um ihn nachzutragen.
ui-search-open = Öffnen
ui-search-open_missing = Replay-Datei existiert nicht mehr
ui-search-opened_directory = { $dir } geöffnet, das dieses Replay enthält
ui-search-open_no_directory = Es lässt sich nicht feststellen, zu welchem Verzeichnis dieses Replay gehört
ui-search-path_copied = Replay-Pfad in die Zwischenablage kopiert
ui-search-preview_map = Karte in der Vorschau anzeigen
ui-search-preview_and_render = Überfahren für Vorschau, Klicken zum Rendern
ui-search-free_text = enthält "{ $text }"
ui-search-quant_any = beliebige
ui-search-quant_none = keine
ui-search-quant_ge = { $n } oder mehr
ui-search-quant_eq = genau { $n }
ui-search-quant_le = höchstens { $n }
ui-search-quant_gt = mehr als { $n }
ui-search-quant_lt = weniger als { $n }
ui-search-quant_ne = nicht { $n }
ui-search-scope_self = Eigenes
ui-search-scope_ally = Verbündetes
ui-search-scope_enemy = Feindliches
ui-search-scope_div = Div

## ui.search.suggest
ui-search-suggest-preset_divmate_test_ship = Jemand in meiner Division hat ein Testschiff gespielt
ui-search-suggest-preset_no_enemy_cv = Kein feindlicher Flugzeugträger
ui-search-suggest-preset_all_enemies_died = Alle Gegner wurden versenkt
ui-search-suggest-preset_high_damage_enemies = 3 oder mehr Gegner über 100k
ui-search-suggest-preset_stream_sniper = Enthält einen Stream-Sniper
ui-search-suggest-preset_i_survived = Ich habe überlebt
ui-search-suggest-preset_i_disconnected = Ich hatte einen Verbindungsabbruch
ui-search-suggest-my_damage = Mein Schaden
ui-search-suggest-enemy_ship = Feindliches Schiff
ui-search-suggest-allied_ship = Verbündetes Schiff
ui-search-suggest-player_in_match = Spieler war im Gefecht
ui-search-suggest-someone_in_division = Jemand in meiner Division
ui-search-suggest-scope_self = Mein
ui-search-suggest-scope_ally = Verbündet
ui-search-suggest-scope_enemy = Feindlich
ui-search-suggest-scope_division = Division
ui-search-suggest-scope_anyone = Beliebiger Spieler

## ui.search.bar
ui-search-bar-hint = Replays filtern
ui-search-bar-and_word = und
ui-search-bar-or_word = oder
ui-search-bar-not_word = nicht
ui-search-bar-group_and = Mit UND gruppieren
ui-search-bar-group_or = Mit ODER gruppieren
ui-search-bar-negate = Negieren
ui-search-bar-delete = Löschen
ui-search-bar-clear_selection = Auswahl aufheben
ui-search-bar-ungroup = Klicken, um diese Gruppe aufzulösen
ui-search-bar-flip_connector = Klicken, um zwischen UND und ODER zu wechseln
ui-search-bar-context_preset = Vorlage
ui-search-bar-context_match = Gefecht
ui-search-bar-context_roster = Aufstellung
ui-search-bar-no_suggestions = Keine Treffer

## ui.search.field
ui-search-field-outcome = Ergebnis
ui-search-field-map = Karte
ui-search-field-class = Klasse
ui-search-field-tier = Stufe
ui-search-field-date = Datum
ui-search-field-player_present = Spieler
ui-search-field-group = Replay-Gruppe
ui-search-field-stat_damage = Schaden
ui-search-field-stat_kills = Abschüsse
ui-search-field-stat_spotting = Aufklärung
ui-search-field-stat_potential = Potenzieller Schaden
ui-search-field-stat_received = Erhaltener Schaden
ui-search-field-stat_pr = PR
ui-search-field-stat_mc = MC
ui-search-field-rating = Bewertung
ui-search-field-stat_survived = Überlebt
ui-search-field-stat_disconnected = Verbindung getrennt
ui-search-field-game_type = Gefechtsart
ui-search-field-game_mode = Spielmodus
ui-search-field-match_group = Gefechtsgruppe
ui-search-field-build = Build
ui-search-field-results_available = Ergebnisse verfügbar
ui-search-field-relation = Beziehung
ui-search-field-division = Division
ui-search-field-name = Name
ui-search-field-clan = Clan
ui-search-field-realm = Server
ui-search-field-ship = Schiff
ui-search-field-ship_index = Schiffsindex
ui-search-field-nation = Nation
ui-search-field-test_ship = Testschiff
ui-search-field-stream_sniper = Stream-Sniper
ui-search-field-sniper_login = Twitch-Login des Snipers

## ui.search.op
ui-search-op-contains = enthält
ui-search-op-equals = ist gleich
ui-search-op-not_equals = ist ungleich
ui-search-op-eq = =
ui-search-op-ne = !=
ui-search-op-gt = >
ui-search-op-ge = >=
ui-search-op-lt = <
ui-search-op-le = <=
ui-search-op-is = ist
ui-search-op-is_not = ist nicht
ui-search-op-is_set = ist gesetzt
ui-search-op-is_not_set = ist nicht gesetzt

## ui.search.value
ui-search-value-relation_self = Selbst
ui-search-value-relation_ally = Verbündeter
ui-search-value-relation_enemy = Gegner
ui-search-value-division_mine = Meine
ui-search-value-division_any = Beliebig
ui-search-value-division_none = Keine
ui-search-value-class_air_carrier = Flugzeugträger
ui-search-value-class_battleship = Schlachtschiff
ui-search-value-class_cruiser = Kreuzer
ui-search-value-class_destroyer = Zerstörer
ui-search-value-class_submarine = U-Boot
ui-search-value-class_auxiliary = Hilfsschiff
ui-search-value-rating_bad = Schlecht
ui-search-value-rating_below_average = Unterdurchschnittlich
ui-search-value-rating_average = Durchschnittlich
ui-search-value-rating_good = Gut
ui-search-value-rating_very_good = Sehr gut
ui-search-value-rating_great = Hervorragend
ui-search-value-rating_unicum = Unicum
ui-search-value-rating_super_unicum = Super Unicum
ui-search-value-game_mode_test = Test
ui-search-value-game_mode_standard = Standard
ui-search-value-game_mode_singlebase = Einzelbasis
ui-search-value-game_mode_domination = Vorherrschaft
ui-search-value-game_mode_tutorial = Tutorial
ui-search-value-game_mode_megabase = Megabasis
ui-search-value-game_mode_forts = Festungen
ui-search-value-game_mode_standard_domination = Standard-Vorherrschaft
ui-search-value-game_mode_epicenter = Epizentrum
ui-search-value-game_mode_assault_defense = Sturmangriff-Verteidigung
ui-search-value-game_mode_pve = PVE
ui-search-value-game_mode_arms_race = Wettrüsten
ui-search-value-game_mode_epicenter_ring = Epizentrum-Ring
ui-search-value-game_mode_anti_standard = Anti-Standard
ui-search-value-game_mode_attack_defense = Angriff-Verteidigung
ui-search-value-game_mode_torpedo_beat = Torpedo-Beat
ui-search-value-game_mode_team_battle_royale = Team Battle Royale
ui-search-value-game_mode_escape_to_portal = Flucht zum Portal
ui-search-value-game_mode_domination_asymm = Vorherrschaft asymm.
ui-search-value-game_mode_key_battle = Schlüsselgefecht
ui-search-value-game_mode_portal_2021 = Portal 2021
ui-search-value-game_mode_team_battle_royale_2021 = Team Battle Royale 2021
ui-search-value-game_mode_convoy_event = Konvoi-Event
ui-search-value-game_mode_convoy_airship = Konvoi-Luftschiff
ui-search-value-game_mode_two_teams_battle_royale = Battle Royale mit zwei Teams
ui-search-value-game_mode_pinata_event = Pinata-Event
ui-search-value-game_mode_respawns = Respawns
ui-search-value-game_mode_respawns_sectors = Respawn-Sektoren


## ui.search.column
ui-search-column-date = Datum
ui-search-column-map = Karte
ui-search-column-mode = Modus
ui-search-column-ship = Ihr Schiff
ui-search-column-result = Ergebnis
ui-search-column-damage = Schaden
ui-search-column-kills = Abschüsse
ui-search-column-pr = PR
ui-search-column-mc = MC

## ui.settings.index
ui-settings-index-heading = Replay-Index
ui-settings-index-description = Das durchsuchbare Verzeichnis aller Replays in Ihrem Ordner
ui-settings-index-build = Index aufbauen
ui-settings-index-rebuild = Alles neu indexieren
ui-settings-index-rebuild_hint = Jedes Replay erneut lesen, für Einträge, die eine ältere Auswertung falsch erfasst hat. Index aufbauen liest nur, was fehlt.
ui-settings-index-progress = { $done } von { $total } gelesen, { $failed } nicht lesbar
ui-settings-index-progress_skipped = { $done } von { $total } gelesen, { $skipped } bereits vorhanden, { $failed } nicht lesbar
ui-settings-index-built = { $indexed } Replays indexiert, { $failed } konnten nicht gelesen werden
ui-settings-index-built_with_skipped = { $indexed } Replays indexiert, { $skipped } waren bereits vorhanden, { $failed } konnten nicht gelesen werden

## ui.settings
ui-settings-loading = Einstellungen werden geladen...
ui-settings-load_failed = Einstellungen konnten nicht geladen werden
ui-settings-not_set = (nicht festgelegt)

## ui.settings.panel
ui-settings-panel-mcp = MCP-Server
ui-settings-panel-open = Einstellungen öffnen
ui-settings-panel-search_placeholder = Einstellungen durchsuchen
ui-settings-panel-count_one = 1 Einstellung
ui-settings-panel-count = { $count } Einstellungen
ui-settings-panel-no_matches = Keine Einstellungen entsprechen der Suche
ui-settings-panel-close = Einstellungen schließen
ui-settings-panel-in_error = { $name }: Eine Einstellung erfordert Aufmerksamkeit
ui-settings-panel-general = Allgemein
ui-settings-panel-appearance = Darstellung
ui-settings-panel-game = World of Warships
ui-settings-panel-replays = Replays
ui-settings-panel-collaboration = Zusammenarbeit
ui-settings-panel-twitch = Twitch
ui-settings-panel-language_description = Sprache der Oberfläche
ui-settings-panel-data_sharing_description = Was nach Ranglisten- und Zufallsgefechten an ShipBuilds.com gesendet wird
ui-settings-panel-code_integrity_description = Ob nicht von Microsoft signierter Code in diese Anwendung geladen werden darf
ui-settings-panel-check_for_updates_description = Bei jedem Start der Anwendung nach einer neueren Version suchen
ui-settings-panel-data_folder = App-Datenordner
ui-settings-panel-theme_description = Farben der Oberfläche. Zeigen Sie auf eine Auswahl, um eine Vorschau zu sehen.
ui-settings-panel-zoom_description = Größe aller Texte und Bedienelemente
ui-settings-panel-zoom = Zoom
ui-settings-panel-zoom_shortcuts = { $zoom_in } vergrößert, { $zoom_out } verkleinert, { $reset } setzt auf 1x zurück
ui-settings-panel-zoom_changed = Zoom: { $factor }x
ui-settings-panel-cache_directory = Cache-Verzeichnis
ui-settings-panel-cache_directory_description = Speicherort der zwischengespeicherten Builds
ui-settings-panel-cached_builds = Zwischengespeicherte Builds
ui-settings-panel-current_path_description = Die zuletzt geöffnete Replay-Datei
ui-settings-panel-show_raw_xp_description = Der Ergebnistabelle eine Spalte für Roh-XP hinzufügen
ui-settings-panel-show_observed_damage_description = Der Ergebnistabelle eine Spalte für beobachteten Schaden hinzufügen
ui-settings-panel-show_entity_id_description = Der Ergebnistabelle eine Spalte für die Entity-ID hinzufügen
ui-settings-panel-show_heals_description = Der Ergebnistabelle eine Spalte für Heilungen hinzufügen
ui-settings-panel-auto_export_description = Die Daten jedes Gefechts im gewählten Format in eine Datei im gewählten Verzeichnis schreiben
ui-settings-panel-display_name_description = Der Name, den andere in einer Sitzung sehen
ui-settings-panel-get_token_description = Eine Anmeldeseite öffnen, die ein Chat-Token ausstellt
ui-settings-panel-token = Anmeldetoken
ui-settings-panel-monitored_channel_description = Zu beobachtender Kanal. Leer lassen, um den eigenen zu beobachten.
ui-settings-panel-armor_defaults = Gespeicherte Standardwerte
ui-settings-panel-armor_saved_description = Aus den Optionen der Modellansicht gespeichert

## ui.settings.licenses
ui-settings-licenses-title = Info und Lizenzen
ui-settings-licenses-back = Zurück zu den Einstellungen
ui-settings-licenses-third_party_intro = Diese Anwendung enthält die unten aufgeführten Drittanbieterkomponenten. Wählen Sie einen Eintrag, um die Pakete, die ihn verwenden, und den vollständigen Lizenztext anzuzeigen.

## ui.settings.armor
ui-settings-armor-no_defaults = (keine gespeicherten Standardwerte)

## ui.app
ui-app-status_cache_checking = Spieldaten-Cache wird geprüft
ui-app-status_cache_validating = Spieldaten-Cache wird überprüft
ui-app-status_cache_downloading = Spieldaten werden heruntergeladen
ui-app-status_cache_planning = Veröffentlichte Spieldaten werden abgefragt
ui-app-drop_to_load = Ablegen, um { $file } zu laden
ui-app-drop_to_list = Ablegen, um die Replays in { $folder } aufzulisten
ui-app-drop_to_list_replays = Ablegen, um { $count } Replays aufzulisten
ui-app-drop_to_list_folders = Ablegen, um die Replays in { $count } Ordnern aufzulisten
ui-app-drop_to_open_and_list = Ablegen, um { $file } zu öffnen und die Replays in { $folders } aufzulisten
ui-app-drop_to_list_replays_and_folders = Ablegen, um { $count } Replays und die Replays in { $folders } aufzulisten
ui-app-drop_folder_count = { $count } Ordnern
ui-app-reading_replay = { $replay } wird gelesen
ui-app-reading_replays = { $count } Replays werden gelesen
ui-app-status_constants_generating = Spielkonstanten für { $version } werden aktualisiert
ui-app-worker_constants = Der Spielkonstanten-Prozess
ui-app-worker_live_match = Die Gefechtsüberwachung
ui-app-worker_replay_loader = Der Replay-Lader
ui-app-worker_restarting = { $worker } wurde beendet und startet neu
ui-app-worker_stalled = { $worker } reagiert nicht
ui-app-worker_stopped = { $worker } wurde beendet
ui-app-status_indexing = Replays werden indexiert
ui-app-status_of = { $done } von { $total }
ui-app-status_tasks = Laufende Aufgaben: { $count }
ui-app-status_show_panel = Bereich einblenden: { $panel }
ui-app-status_hide_panel = Bereich ausblenden: { $panel }
ui-app-panel_left = Links
ui-app-panel_right = Rechts
ui-app-panel_bottom = Unten
ui-app-debug_build = Debug-Build

## ui.palette
ui-palette-title = Befehle
ui-palette-placeholder = Befehl eingeben oder suchen...
ui-palette-go_to = Gehe zu: { $tab }
ui-palette-advanced_search = Erweiterte Suche...
ui-palette-set_theme = Design: { $theme }
ui-palette-games_i_died_in = Gefechte, in denen ich versenkt wurde
ui-palette-games_i_won = Gewonnene Gefechte
ui-palette-send_all_replays = Alle Replays an ShipBuilds senden
ui-palette-send_all_replays_again = Alle Replays an ShipBuilds senden (Sendeprotokoll ignorieren)
ui-palette-search_players = Spieler suchen...
ui-palette-my_matches_in_ship = Meine Gefechte mit Schiff...
ui-palette-open_ship_in_armor_viewer = Schiff in Modellansicht öffnen...
ui-palette-open_ship_in_shiptool = Schiff in ShipTool öffnen...
ui-palette-players_title = Spieler suchen
ui-palette-my_ships_title = Meine Gefechte mit Schiff
ui-palette-armor_ships_title = Schiff in Modellansicht öffnen
ui-palette-shiptool_ships_title = Schiff in ShipTool öffnen
ui-palette-players_placeholder = Spieler nach Namen suchen...
ui-palette-my_ships_placeholder = Gespielte Schiffe suchen...
ui-palette-armor_ships_placeholder = Alle Schiffe nach Namen suchen...
ui-palette-armor_ship_row = { $name } -- Stufe { $tier } { $class }, { $nation }
ui-palette-back_hint = Esc: zurück
ui-palette-ships_loading = Schiffsliste wird geladen...
ui-palette-ships_unavailable = Keine Schiffsliste: { $reason }
ui-palette-matches_found = { $count } Treffer
ui-palette-nothing_indexed = Noch nichts indexiert
ui-palette-sample_stats = Debug: Beispieldaten in Statistiken anzeigen
ui-palette-restore_stats = Debug: aufgezeichnete Daten in Statistiken anzeigen

## ui.replay.hover
ui-replay-hover-ship = Schiff
ui-replay-hover-map = Karte
ui-replay-hover-mode = Modus
ui-replay-hover-blank_mode = Leer
ui-replay-hover-played = Gespielt
ui-replay-hover-damage = Schaden
ui-replay-hover-kills = Abschüsse
ui-replay-hover-result = Ergebnis
ui-replay-hover-division = Division
ui-replay-hover-twitch = Twitch-Chat

## ui.replay.renderer
ui-replay-renderer-export_video = Als Video rendern
ui-replay-renderer-export_clipboard = In Zwischenablage rendern
ui-replay-renderer-export_saved = Video gespeichert unter { $path }
ui-replay-renderer-export_copied = Video in die Zwischenablage kopiert
ui-replay-renderer-export_not_written = Das Video konnte nicht gerendert werden: { $reason }
ui-replay-renderer-pop_out = In eigenem Fenster öffnen
ui-replay-renderer-export_path_unusable = In diesen Ort kann nicht geschrieben werden
ui-replay-renderer-batch_started = { $count } Replay(s) werden nach { $dir } gerendert
ui-replay-renderer-batch_finished = { $written } Replay(s) gerendert, { $failed } konnten nicht gerendert werden
ui-replay-renderer-batch_all_written = { $written } Replay(s) gerendert
ui-replay-renderer-batch_copied = { $written } Replay(s) gerendert und in die Zwischenablage gelegt
ui-replay-renderer-batch_not_copied = In { $dir } gerendert, aber die Zwischenablage hat sie abgelehnt: { $reason }
ui-replay-renderer-batch_progress = Rendern { $done } von { $total }: { $replay }
ui-replay-renderer-baking = Replay wird gelesen und Gefecht ausgewertet...
ui-replay-renderer-failed = Dieses Replay konnte nicht abgespielt werden
ui-replay-renderer-play = Abspielen
ui-replay-renderer-pause = Pause

## ui.renderer.annotations
ui-renderer-annotations-title = Zeichenwerkzeuge
ui-renderer-annotations-undo = Rückgängig
ui-renderer-annotations-redo = Wiederholen
ui-renderer-annotations-ship_hint = Schiff suchen
ui-renderer-annotations-no_ship = Kein Schiff ausgewählt
ui-renderer-annotations-nib = Strichstärke
ui-renderer-annotations-thinner = Dünnerer Strich
ui-renderer-annotations-thicker = Dickerer Strich
ui-renderer-annotations-shortcuts_title = Tastenkürzel
ui-renderer-annotations-shortcuts_tools = Werkzeuge
ui-renderer-annotations-shortcuts_actions = Aktionen
ui-renderer-annotations-tool_select = Auswählen
ui-renderer-annotations-tool_arrow = Pfeil
ui-renderer-annotations-tool_freehand = Freihand
ui-renderer-annotations-tool_line = Linie
ui-renderer-annotations-tool_circle = Kreis
ui-renderer-annotations-tool_rectangle = Rechteck
ui-renderer-annotations-tool_triangle = Dreieck
ui-renderer-annotations-tool_measurement = Messen
ui-renderer-annotations-tool_eraser = Radiergummi
ui-renderer-annotations-action_undo = Rückgängig
ui-renderer-annotations-action_multi_select = Mehrfachauswahl
ui-renderer-annotations-action_stroke_width = Strichstärke
ui-renderer-annotations-action_delete = Löschen
ui-renderer-annotations-action_cancel = Abbrechen / Abwählen
ui-renderer-annotations-toggle_fill = Erneut klicken, um die Füllung umzuschalten
ui-renderer-annotations-clear_all = Alles löschen
ui-renderer-annotations-edit = Zeichnung bearbeiten
ui-renderer-annotations-size = Größe
ui-renderer-annotations-color = Farbe
ui-renderer-annotations-custom_ink = Eigene Farbe
ui-renderer-annotations-filled = Gefüllt
ui-renderer-annotations-locked = Anmerkungen vom Host gesperrt

## ui.armor.cast
ui-armor-cast-prompt = { $key }+Klick auf den Rumpf fügt einen Einschlagpunkt hinzu
ui-armor-cast-empty = Noch keine Einschlagpunkte
ui-armor-cast-miss_nothing_under_pointer = Keine Panzerplatte unter dem Mauszeiger
ui-armor-cast-miss_thickness_unknown = Die Platten dieses Modells haben keine bekannte Dicke, auf die geschossen werden kann
ui-armor-cast-miss_no_plate_on_path = Die Flugbahn der Granate aus diesem Winkel kreuzt keine Panzerplatte

## ui.armor.pen
ui-armor-pen-search_hint = Schiffe zum Vergleichen suchen...
ui-armor-pen-add_a_ship = Suchen Sie oben Schiffe und fügen Sie sie hinzu, um den Durchschlag zu vergleichen
ui-armor-pen-hover_a_plate = Fahren Sie über eine Platte des Schiffs, um sie zu prüfen
ui-armor-pen-awaiting_plate = über eine Platte fahren
ui-armor-pen-clear_all = Alle löschen
ui-armor-pen-remove = Aus Vergleich entfernen
ui-armor-pen-title = Durchschlag
ui-armor-pen-title_tooltip = Prüfen, was die Granaten eines Schiffs mit einer Platte machen
ui-armor-pen-ship = Feuerndes Schiff
ui-armor-pen-firing_ship_tooltip = Die Granaten dieses Schiffs für Flugbahnberechnungen verwenden
ui-armor-pen-use_for_trajectory = Für Flugbahn verwenden
ui-armor-pen-pick_ship = Das Schiff wählen, das auf dieses feuert
ui-armor-pen-enter_thickness = Plattendicke eingeben
ui-armor-pen-no_shells = { $ship } hat keine Granaten für die Hauptbatterie
ui-armor-pen-plate = Platte
ui-armor-pen-mm = mm
ui-armor-pen-ifhe = IFHE
ui-armor-pen-ifhe_tooltip = Trägheitszünder für Sprenggranaten erhöht den HE-Durchschlag um ein Viertel
ui-armor-pen-last_hovered = Zuletzt überfahren: { $plate }
ui-armor-pen-penetrates = durchschlägt
ui-armor-pen-shatters = zerschellt
ui-armor-pen-angle_dependent = abhängig vom Winkel
ui-armor-pen-no_data = keine Durchschlagsdaten

## ui.armor.splash
ui-armor-splash-hint = Klicken Sie im Splash-Modus auf Panzerung, um zu sehen, welche Zonen eine Explosion erreicht
ui-armor-splash-half_extent = Halbe Ausdehnung der Explosion: { $value } Modelleinheiten
ui-armor-splash-triangles = Panzerungsdreiecke in der Explosion: { $total }, durchschlagen: { $penetrated }
ui-armor-splash-no_zones = An dieser Stelle überlappen keine Splash-Boxen
ui-armor-splash-zones = Zonen in der Explosion
ui-armor-splash-direct_hit = Direkter Treffer
ui-armor-splash-hp = { $value } HP
ui-armor-splash-section = Splash-Zonen

## ui.armor.export
ui-armor-export-hull = Rumpf
ui-armor-export-contents = Inhalt
ui-armor-export-contents_mesh = Mesh
ui-armor-export-contents_armor = Panzerung
ui-armor-export-contents_both = Mesh und Panzerung
ui-armor-export-lod = Mesh-Detailstufe
ui-armor-export-lod_tooltip = 0 ist die höchste Detailstufe. Die Panzerung hat keine Detailstufen und ist nicht betroffen.
ui-armor-export-res = Texturauflösung
ui-armor-export-res_full = Voll
ui-armor-export-res_2048 = 2048 px
ui-armor-export-res_1024 = 1024 px
ui-armor-export-res_512 = 512 px
ui-armor-export-camos = Tarnungen
ui-armor-export-camo_select_none = Keine auswählen
ui-armor-export-camo_reset = Auf Standard zurücksetzen
ui-armor-export-loading_details = Laden...
ui-armor-export-size_estimate = Geschätzte Dateigröße: { $size }
ui-armor-export-size_breakdown = Geometrie { $geometry } (exakt), Texturen { $textures } (geschätzt)
ui-armor-export-size_incomplete = Größe von { $count } Tarnung(en) nicht ermittelbar, Gesamtgröße möglicherweise unvollständig
ui-armor-export-exporting = { $ship } wird exportiert...
ui-armor-export-close_button = Schließen
ui-armor-export-already_running = Ein Export läuft bereits
ui-armor-export-exported = { $ship } exportiert
ui-armor-export-exported_with_size = { $ship } exportiert ({ $size })
ui-armor-export-vehicle_not_found = Schiff nicht gefunden
ui-armor-export-create_file_failed = Datei konnte nicht erstellt werden: { $error }
ui-armor-export-export_failed = Export fehlgeschlagen: { $error }

## ui.armor.outcome
ui-armor-outcome-penetration = Durchschlag
ui-armor-outcome-citadel = Zitadelle
ui-armor-outcome-ricochet = Abpraller
ui-armor-outcome-shatter = Zerschellen
ui-armor-outcome-overpenetration = Durchschuss
ui-armor-outcome-underwater = Unterwasser
ui-armor-outcome-unknown = Unbekannt ({ $raw })
ui-armor-outcome-stopped = In der Panzerung gestoppt

## ui.armor.sim
ui-armor-sim-overmatch_rules_out_ricochet = Overmatch, daher kein Abpraller
ui-armor-sim-always_ricochet_band = Abpraller, im Bereich des garantierten Abprallers
ui-armor-sim-ricochet_angle_too_low = Kein Abpraller möglich, Winkel zu gering

## ui.tactics
ui-tactics-a_peers_board = Eine Tafel, die von jemand anderem in dieser Sitzung geteilt wird
ui-tactics-map = Karte
ui-tactics-map_hint = Karte suchen
ui-tactics-mode = Modus
ui-tactics-pick_a_map = Karte zum Zeichnen wählen
ui-tactics-drawing = Karte wird gezeichnet...
ui-tactics-reset_view = Ansicht zurücksetzen
ui-tactics-add_cap = Punkt hinzufügen
ui-tactics-add_cap_tooltip = Mit dem nächsten Klick auf die Karte einen Eroberungspunkt platzieren
ui-tactics-selected_cap = Punkt { $letter }
ui-tactics-cap_radius = { $km } km
ui-tactics-cap_team = Team
ui-tactics-cap_team_tooltip = Team des ausgewählten Eroberungspunkts wechseln
ui-tactics-team_neutral = Neutral
ui-tactics-team_one = Team 1 (Grün)
ui-tactics-team_two = Team 2 (Rot)
ui-tactics-delete_cap = Löschen
ui-tactics-delete_cap_tooltip = Ausgewählten Eroberungspunkt entfernen
ui-tactics-cap_replay_delete_tooltip = Ein aus einem Replay geladener Eroberungspunkt kann nicht gelöscht werden
ui-tactics-cap_narrow_tooltip = Radius des ausgewählten Eroberungspunkts verkleinern
ui-tactics-cap_widen_tooltip = Radius des ausgewählten Eroberungspunkts vergrößern
ui-tactics-undo_tooltip = Letzte Änderung an der Tafel rückgängig machen ({ $shortcut })
ui-tactics-redo_tooltip = Zuletzt rückgängig gemachte Änderung wiederherstellen ({ $shortcut } oder { $alternate })
ui-tactics-clear_caps = Hinzugefügte Punkte löschen
ui-tactics-clear_added_caps_tooltip = Vom Benutzer hinzugefügte Eroberungspunkte entfernen
ui-tactics-preset_name = Name der Tafel
ui-tactics-preset_save = Speichern
ui-tactics-preset_delete = Löschen
ui-tactics-reset_board = Tafel zurücksetzen
ui-tactics-reset_board_tooltip = Punkte des ausgewählten Modus wiederherstellen und Zeichnungen löschen
ui-tactics-preset_needs_a_name = Geben Sie der Tafel zuerst einen Namen
ui-tactics-preset_needs_a_map = Wählen Sie zuerst eine Karte für die Tafel
ui-tactics-preset_saved = { $name } gespeichert
ui-tactics-preset_save_failed = Die Tafel konnte nicht gespeichert werden: { $error }
ui-tactics-preset_load_failed = Die Tafel konnte nicht geöffnet werden: { $error }
ui-tactics-preset_delete_failed = Die Tafel konnte nicht gelöscht werden: { $error }
ui-tactics-presets = Gespeicherte Tafeln
ui-tactics-preset_open = Öffnen
ui-tactics-clear_drawing = Zeichnung löschen
ui-tactics-scan_replays = Punkte aus Replays übernehmen
ui-tactics-scan_replays_tooltip = Replays nach den Punkteanordnungen der Modi auslesen, in denen sie gespielt wurden
ui-tactics-scan_needs_game_data = Laden Sie zuerst einen Spiel-Build
ui-tactics-scan_running = Replays werden gelesen...
ui-tactics-scan_progress = Replays werden gelesen... { $read } / { $total }
ui-tactics-scan_done = { $total } Replays gelesen und { $added } neue Anordnungen gefunden
ui-tactics-session_boards_full = Die Sitzung hat bereits { $limit } Tafeln offen, daher bleibt diese Tafel auf ihrer Karte
ui-tactics-placing = { $ship } wird platziert
ui-tactics-side_friendly = Verbündet
ui-tactics-side_enemy = Feindlich
ui-tactics-ink_white = Weiße Farbe
ui-tactics-ink_gray = Graue Farbe
ui-tactics-ink_red = Rote Farbe
ui-tactics-ink_orange = Orange Farbe
ui-tactics-ink_yellow = Gelbe Farbe
ui-tactics-ink_green = Grüne Farbe
ui-tactics-ink_blue = Blaue Farbe
ui-tactics-ink_purple = Violette Farbe
ui-tactics-ink_pink = Rosa Farbe
ui-tactics-no_map_art = Dieser Build enthält keine Grafik für diese Karte
ui-tactics-ship_config = Schiffskonfiguration
ui-tactics-hull = Rumpf
ui-tactics-hull_default = Standard
ui-tactics-modifiers = Modifikatoren
ui-tactics-concealment_expert = Tarnungsexperte
ui-tactics-gun_range_mod = Mod. Hauptbatterie-Reichweite
ui-tactics-secondary_range_mod = Mod. Sekundärbatterie-Reichweite
ui-tactics-ship_ranges = Reichweitenkreise
ui-tactics-shape = Form
ui-tactics-shape_size = Größe
ui-tactics-shape_size_km = Größe (km)
ui-tactics-shape_filled = Gefüllt
ui-tactics-shape_smaller = Form verkleinern
ui-tactics-shape_larger = Form vergrößern

## ui.launcher
ui-launcher-rescan = Erneut suchen
ui-launcher-add_install = Installation hinzufügen...
ui-launcher-add_install_title = Installationsverzeichnis von World of Warships
ui-launcher-no_installs = Keine Installationen gefunden. Fügen Sie eine hinzu oder installieren Sie unten ein Spiel.
ui-launcher-managed_by_steam = von Steam verwaltet
ui-launcher-checking_updates = suche nach Updates
ui-launcher-up_to_date = Aktuell
ui-launcher-update_available = Update verfügbar: { $parts }
ui-launcher-not_installed = nicht installiert: { $parts }
ui-launcher-check_failed = Update-Prüfung fehlgeschlagen: { $error }
ui-launcher-recheck = Erneut prüfen
ui-launcher-download = Herunterladen / Aktualisieren...
ui-launcher-download_title = Installationsort für den Build
ui-launcher-download_hint = Eine Neuinstallation umfasst mehrere zehn GB. Delta-Updates werden noch nicht unterstützt, daher wird ein geänderter Teil vollständig neu heruntergeladen.
ui-launcher-pause = Pause
ui-launcher-cancel = Abbrechen
ui-launcher-resume = Fortsetzen
ui-launcher-paused = Download pausiert. Bereits geladene Dateien bleiben erhalten.
ui-launcher-installed = Installiert
ui-launcher-installed_skipped = Installiert ({ $count } Delta-Teil(e) übersprungen: noch nicht unterstützt, heruntergeladen, aber nicht angewendet)
ui-launcher-download_failed = Download fehlgeschlagen: { $error }
ui-launcher-update_in_place = Ein Update wird in den Ordner der Installation selbst geschrieben. Wählen Sie { $path }, um sie zu aktualisieren.
ui-launcher-cancelled = Download abgebrochen.
ui-launcher-launch_failed = Start fehlgeschlagen: { $error }
ui-launcher-use_for_toolkit = Für Toolkit verwenden
ui-launcher-not_set = nicht festgelegt
ui-launcher-browse = Durchsuchen...
ui-launcher-install_heading = Spiel installieren
ui-launcher-install_hint = Eine Neuinstallation umfasst mehrere zehn GB.
ui-launcher-install_title = Installationsort für das Spiel
ui-launcher-install = Installieren...
ui-launcher-checking_version = verfügbare Version wird geprüft
ui-launcher-version = Version { $version }
ui-launcher-version_unreported = Client-Version nicht gemeldet
ui-launcher-unavailable = nicht verfügbar: { $error }
ui-launcher-retry = Erneut versuchen

## ui.launcher.account
ui-launcher-account-sign_in = Anmelden
ui-launcher-account-add_title = Konto hinzufügen
ui-launcher-account-again_title = Erneut anmelden
ui-launcher-account-memory_only = Nur für diese Sitzung angemeldet. Die Anmeldung wird nicht gespeichert, daher müssen Sie sich nach einem Neustart erneut anmelden.
ui-launcher-account-password_note = Ihr Passwort wird nur an Wargaming gesendet und niemals gespeichert.
ui-launcher-account-realm_fixed = Region: { $realm }
ui-launcher-account-email = E-Mail
ui-launcher-account-password = Passwort
ui-launcher-account-code = Zwei-Faktor-Code
ui-launcher-account-code_required = Zwei-Faktor-Code erforderlich.
ui-launcher-account-use_backup_code = Backup-Code verwenden
ui-launcher-account-submit_code = Code senden
ui-launcher-account-captcha = Diese Anmeldung erfordert ein CAPTCHA, das dieser Launcher noch nicht anzeigen kann. Melden Sie sich einmal über das offizielle Wargaming Game Center oder wargaming.net im Browser an und versuchen Sie es dann erneut.

## ui.launcher.sign_in_error
ui-launcher-sign_in_error-wrong_credentials = Falsche E-Mail-Adresse oder falsches Passwort.
ui-launcher-sign_in_error-two_factor_required = Dieses Konto erfordert eine Zwei-Faktor-Bestätigung. Bestätigen Sie die Anmeldung in Wargaming Auth oder mit dem per E-Mail gesendeten Code und melden Sie sich dann erneut an.
ui-launcher-sign_in_error-two_factor_invalid = Der Zwei-Faktor-Code ist falsch oder abgelaufen. Geben Sie einen neuen Code ein.
ui-launcher-sign_in_error-captcha_required = Wargaming verlangt ein CAPTCHA. Melden Sie sich einmal im offiziellen Game Center oder auf wargaming.net an und versuchen Sie es dann erneut.
ui-launcher-sign_in_error-rate_limited = Zu viele Anmeldeversuche. Warten Sie einige Minuten und versuchen Sie es dann erneut.
ui-launcher-sign_in_error-account_banned = Dieses Konto ist gesperrt.
ui-launcher-sign_in_error-network_unreachable = Wargaming ist nicht erreichbar. Prüfen Sie Ihre Internetverbindung.
ui-launcher-sign_in_error-service_unavailable = Der Anmeldedienst von Wargaming ist nicht verfügbar. Versuchen Sie es später erneut.
ui-launcher-sign_in_error-session_expired = Die gespeicherte Anmeldung ist abgelaufen. Melden Sie sich erneut an.
ui-launcher-sign_in_error-keyring_locked = Ihr System-Schlüsselbund ist gesperrt. Entsperren Sie ihn und melden Sie sich dann erneut an.
ui-launcher-sign_in_error-not_saved = Angemeldet, aber die Anmeldung konnte nicht gespeichert werden. Nach einem Neustart müssen Sie sich erneut anmelden.
ui-launcher-sign_in_error-unexpected = Anmeldung fehlgeschlagen. Details stehen im Protokoll.

## ui.launcher.realm
ui-launcher-realm-eu = Europa
ui-launcher-realm-na = Nordamerika
ui-launcher-realm-asia = Asien

## ui.launcher.runner
ui-launcher-runner-looking = Proton wird gesucht.
ui-launcher-runner-steam_proton = { $name } von Steam
ui-launcher-runner-umu = umu-run mit { $name }
ui-launcher-runner-umu_default = umu-run mit UMU-Proton
ui-launcher-runner-native = Das Spiel läuft nativ.
ui-launcher-runner-first_run_prefix = Der erste Start erstellt ein Wine-Präfix, das dauert einige Sekunden.
ui-launcher-runner-first_run_download = Der erste Start lädt UMU-Proton und eine Steam-Laufzeitumgebung herunter, etwa 700 MB.
ui-launcher-runner-choose = Proton-Ordner auswählen
ui-launcher-runner-automatic = Automatisch suchen
ui-launcher-runner-detect = Erneut suchen
ui-launcher-runner-needed = Zum Starten unter Linux wird Proton benötigt. Installieren Sie Steam mit Proton oder umu-launcher.

## ui.replay.damage_share
ui-replay-damage_share-mode = Schaden als Anteil an der Summe des anderen Schiffs
ui-replay-damage_share-dealt = { $pct } des von { $ship } verursachten Schadens gingen an { $other }.
ui-replay-damage_share-dealt_inverse = { $pct } des von { $other } erlittenen Schadens kamen von { $ship }.
ui-replay-damage_share-received = { $pct } des von { $ship } erlittenen Schadens kamen von { $other }.
ui-replay-damage_share-received_inverse = { $pct } des von { $other } verursachten Schadens gingen an { $ship }.
ui-replay-damage_share-hold = { $key } gedrückt halten, um umzukehren.
ui-replay-damage_share-release = { $key } loslassen, um zurückzukehren.

## ui.reindex
ui-reindex-context_one = Neu indexieren erzwingen
ui-reindex-context_many = { $count } Replays neu indexieren erzwingen
ui-reindex-palette_by_version = Replays neu indexieren erzwingen: nach Version
ui-reindex-palette_all = Replays neu indexieren erzwingen: alle
ui-reindex-versions_title = Replays neu indexieren erzwingen: nach Version
ui-reindex-versions_placeholder = Spielversionen filtern...
ui-reindex-version_row = { $version } -- { $count } Replay(s)
ui-reindex-no_versions = Kein aufgelistetes Replay nennt eine Spielversion
ui-reindex-confirm_title = Replays neu indexieren erzwingen
ui-reindex-confirm_ok = Neu indexieren
ui-reindex-confirm_count = { $count } Replay(s) werden erneut gelesen und ihre Indexeinträge neu geschrieben.
ui-reindex-confirm_range = Gespielt von { $oldest } bis { $newest }.
ui-reindex-confirm_undated = { $count } zu lesende(s) Replay(s) haben kein lesbares Gefechtsdatum und liegen nicht in diesem Zeitraum.
ui-reindex-confirm_downloads = Spieldaten für diese Versionen werden zuerst heruntergeladen:
ui-reindex-confirm_unobtainable = Für diese Versionen sind keine Spieldaten erhältlich, daher werden ihre { $count } Replay(s) übersprungen:
ui-reindex-confirm_no_cache_dir = Es ist kein Cache-Verzeichnis für Spieldaten festgelegt, daher können die Spieldaten für diese Versionen nicht heruntergeladen werden und ihre { $count } Replay(s) werden übersprungen:
ui-reindex-confirm_no_build = { $count } Replay(s) nennen keinen Spiel-Build und werden übersprungen.
ui-reindex-confirm_no_version = { $count } Replay(s) nennen keine lesbare Spielversion und werden übersprungen.
ui-reindex-build_row = { $version } (Build { $build }): { $count } Replay(s)
ui-reindex-nothing_listed = Die Liste enthält keine Replays zum Neuindexieren.
ui-reindex-nothing_readable = Keines dieser Replays kann neu indexiert werden: Für sie sind keine Spieldaten verfügbar ({ $count } übersprungen).
ui-reindex-no_game_data = Replays können erst gelesen werden, wenn das Spielverzeichnis festgelegt ist.
ui-reindex-busy_index = Ein Indexdurchlauf läuft bereits.
ui-reindex-busy_cache = Ein Spieldaten-Auftrag läuft bereits. Versuchen Sie es erneut, wenn er beendet ist.
ui-reindex-planning = Prüfe, welche Spieldaten verfügbar sind...
ui-reindex-no_index = Der Replay-Index ist nicht geöffnet, daher wurde nichts neu indexiert.
ui-reindex-done = { $indexed } Replay(s) neu indexiert; { $skipped } übersprungen, { $failed } konnten nicht gelesen werden
ui-reindex-stopped = Neuindexierung angehalten: { $indexed } neu indexiert, { $skipped } übersprungen, { $failed } konnten nicht gelesen werden
ui-reindex-stopped_before_reading = Neuindexierung angehalten, bevor ein Replay gelesen wurde.

## ui.index_folder
ui-index_folder-tooltip = Alle Replays in dieser Liste indexieren, die noch nicht im Replay-Index enthalten sind
ui-index_folder-confirm_title = Replays indexieren
ui-index_folder-confirm_ok = Indexieren
ui-index_folder-confirm_count = { $count } Replay(s) in dieser Liste sind noch nicht im Replay-Index und werden eingelesen und hinzugefügt.
ui-index_folder-note_live = Indexierte Replays werden dem Replay-Index hinzugefügt, den Suche und Spieler-Tracker lesen.
ui-index_folder-note_folder = Indexierte Replays werden dem Replay-Index unter der eigenen Quelle dieses Verzeichnisses ({ $dir }) hinzugefügt. Die Suche und die Spielerliste des Spieler-Trackers berücksichtigen sie. Die Clanliste des Spieler-Trackers liest nur die eigenen Replays des Spiels.
ui-index_folder-nothing_listed = Diese Liste enthält keine Replays zum Indexieren.
ui-index_folder-nothing_missing = Alle Replays in dieser Liste sind bereits indexiert.
ui-index_folder-unreadable_index = Der Replay-Index konnte nicht gelesen werden.
ui-index_folder-done = { $indexed } Replay(s) indexiert; { $skipped } übersprungen, { $failed } konnten nicht gelesen werden
ui-index_folder-stopped = Indexierung angehalten: { $indexed } indexiert, { $skipped } übersprungen, { $failed } konnten nicht gelesen werden
ui-index_folder-stopped_before_reading = Die Indexierung wurde angehalten, bevor ein Replay gelesen wurde.

## ui.replay_folders
ui-replay_folders-install = Spiel
ui-replay_folders-watched = Überwacht: neue und geänderte Replays erscheinen hier
ui-replay_folders-unwatched = Nicht überwacht: mit Aktualisieren erneut einlesen
ui-replay_folders-unseen = { $count } neue oder geänderte Replay(s) seit Ihrem letzten Besuch
ui-replay_folders-missing = Dieses Verzeichnis existiert nicht mehr: { $dir }
ui-replay_folders-missing_hint = Es wird bei jedem Start hier geöffnet, bis Sie es lösen.
ui-replay_folders-pin = Anheften
ui-replay_folders-unpin = Lösen
ui-replay_folders-refresh = Aktualisieren
ui-replay_folders-close = Schließen
ui-replay_folders-pins_unreadable = Angeheftete Replay-Verzeichnisse konnten nicht gelesen werden.
ui-replay_folders-game_data_available = Spieldaten sind für die aufgeführten Replays verfügbar.
ui-replay_folders-browse = Durchsuchen...
ui-replay_folders-recent_title = Replay-Verzeichnis öffnen
ui-replay_folders-recent_placeholder = Zuletzt verwendete Verzeichnisse filtern...
ui-replay_folders-recent_missing = Dieses Verzeichnis existiert nicht mehr und wurde aus der Liste der zuletzt verwendeten entfernt: { $dir }
ui-replay_folders-checking = Suche nach { $dir }...
ui-replay_folders-indexing = { $dir } wird indexiert
ui-replay_folders-indexing_in_background = Dieses Verzeichnis wird noch indexiert
ui-replay_folders-search_while_indexing = Dieses Verzeichnis wird noch indexiert, daher sind die Ergebnisse noch nicht vollständig. Suchen Sie erneut, sobald der Vorgang abgeschlossen ist.
ui-replay_folders-source_failed = Die Replays dieses Verzeichnisses können nicht indexiert werden: { $error }
ui-replay_folders-restart_watch = Überwachung neu starten
ui-replay_folders-watch_stopped = Die Überwachung von { $folder } wurde beendet: { $cause }.
ui-replay_folders-watch_restarting = Neustart in { $seconds } s (Versuch { $attempt } von { $attempts }).
ui-replay_folders-watch_gave_up = Der Neustart ist { $attempts }-mal fehlgeschlagen und wird nicht erneut versucht.
ui-replay_folders-watch_restart_hint = Klicke auf den roten Punkt oder wähle im Tab-Menü „Überwachung neu starten“, um sie jetzt neu zu starten.
ui-replay_folders-watch_resumed = Die Überwachung von { $folder } läuft wieder.
ui-replay_folders-watch_cause_backend = das Dateisystem hat einen Fehler gemeldet: { $error }
ui-replay_folders-watch_cause_gone = das Verzeichnis wurde verschoben oder entfernt
ui-replay_folders-watch_cause_panicked = interner Fehler: { $error }
ui-replay_folders-watch_cause_exited = die Überwachung wurde unerwartet beendet
ui-replay_folders-dropped = Abgelegt
ui-replay_folders-dropped_numbered = Abgelegt { $number }
ui-replay_folders-dropped_count = { $count } abgelegte(s) Replay(s)
ui-replay_folders-dropped_no_source = Abgelegte Replays werden mit ihren eigenen Verzeichnissen indiziert. Öffnen Sie ein Verzeichnis, um es zu durchsuchen oder zu indizieren.
ui-replay_folders-unreadable_replay = Konnte nicht gelesen werden: { $error }

## ui.models
ui-models-heading = Modelle
ui-models-loading = Modelle werden geladen...
ui-models-unavailable = Modelle nicht verfügbar
ui-models-search = Schiffe und Modelle suchen...
ui-models-open = Öffnen
ui-models-export = Modell exportieren
ui-models-copy_path = Modellpfad kopieren
ui-models-path_copied = { $path } kopiert
ui-models-skin_replaces = Ersetzt { $model }, wenn { $exterior } ausgerüstet ist
ui-models-geometry_absent = Nicht in Ihren Spieldateien
ui-models-geometry_none = Dieses Modell hat keine Geometrie
ui-models-hardpoints_tooltip = Befestigungspunkte anzeigen
ui-models-common = Gemeinsame Modelle

## ui.models.group
ui-models-group-gameplay = Spielmodelle
ui-models-group-building = Gebäude
ui-models-group-character = Figuren
ui-models-group-location = Schauplätze
ui-models-group-port = Häfen
ui-models-group-style = Stile
ui-models-group-other = Weitere Modelle

## ui.models.building
ui-models-building-weapon = Gebäudewaffen
ui-models-building-unassigned = Nicht zugeordnete Gebäude
ui-models-building-unspecified = Gebäude ohne Typ

## ui.models.misc
ui-models-misc-crane = Kräne
ui-models-misc-anchor = Anker
ui-models-misc-deck_fitting = Decksbeschläge
ui-models-misc-instrument = Instrumente
ui-models-misc-propeller = Propeller
ui-models-misc-searchlight = Suchscheinwerfer
ui-models-misc-boat = Boote
ui-models-misc-flag = Flaggen
ui-models-misc-insignia = Abzeichen
ui-models-misc-railing = Reling
ui-models-misc-hatch = Luken
ui-models-misc-ventilation = Lüftung
ui-models-misc-antenna = Antennen
ui-models-misc-weapon = Waffen
ui-models-misc-stowage = Stauraum
ui-models-misc-other = Sonstiges

## ui.models.variant
ui-models-variant-dead = Wrack
ui-models-variant-skin = Skin

## ui.models.kind
ui-models-kind-ship = Schiffe
ui-models-kind-gun = Geschütze
ui-models-kind-director = Feuerleitstände
ui-models-kind-finder = Entfernungsmesser
ui-models-kind-radar = Radare
ui-models-kind-catapult = Katapulte
ui-models-kind-aircraft = Flugzeuge
ui-models-kind-projectile = Geschosse
ui-models-kind-misc = Sonstiges
ui-models-kind-cap = Eroberungspunkte
ui-models-kind-other = Andere

## ui.models.sub_kind
ui-models-sub_kind-battleship = Schlachtschiffe
ui-models-sub_kind-cruiser = Kreuzer
ui-models-sub_kind-destroyer = Zerstörer
ui-models-sub_kind-aircraft_carrier = Flugzeugträger
ui-models-sub_kind-submarine = U-Boote
ui-models-sub_kind-auxiliary = Hilfsschiffe
ui-models-sub_kind-main_battery = Hauptbatterie
ui-models-sub_kind-secondary_battery = Sekundärbatterie
ui-models-sub_kind-anti_aircraft = Flugabwehr
ui-models-sub_kind-torpedo_tubes = Torpedorohre
ui-models-sub_kind-depth_charge_launcher = Wasserbombenwerfer
ui-models-sub_kind-missile_launcher = Raketenwerfer
ui-models-sub_kind-search_radar = Suchradare
ui-models-sub_kind-fire_control_radar = Feuerleitradare
ui-models-sub_kind-fighter = Jäger
ui-models-sub_kind-bomber = Bomber
ui-models-sub_kind-dive_bomber = Sturzkampfbomber
ui-models-sub_kind-scout = Aufklärer
ui-models-sub_kind-torpedo = Torpedos
ui-models-sub_kind-bomb = Bomben
ui-models-sub_kind-rocket = Raketen
ui-models-sub_kind-depth_charge = Wasserbomben
ui-models-sub_kind-mine = Minen
ui-models-sub_kind-missile = Flugkörper
ui-models-sub_kind-jato = Starthilfsraketen
ui-models-sub_kind-shell = Granaten

## ui.models.part
ui-models-part-hull = Rumpf
ui-models-part-main_battery = Hauptbatterie
ui-models-part-secondaries = Sekundärbewaffnung
ui-models-part-anti_aircraft = Flak
ui-models-part-torpedoes = Torpedos
ui-models-part-directors = Feuerleitstände
ui-models-part-finders = Entfernungsmesser
ui-models-part-radars = Radare
ui-models-part-depth_charges = Wasserbomben
ui-models-part-aircraft = Flugzeuge
ui-models-part-other = Andere
ui-models-part-misc = Sonstiges

## ui.model_view
ui-model_view-thickness_from_building = Dicke aus { $building }
ui-model_view-thickness_from = Dicke von { $ship } ({ $hp })
ui-model_view-thickness_unknown = Dicke unbekannt: kein Schiff trägt dieses Modell
ui-model_view-thickness_other_ships = Andere Schiffe
ui-model_view-export_title = Modell exportieren
ui-model_view-export_tooltip = Modell als GLB-Datei exportieren
ui-model_view-part_model = Modell: { $model }
ui-model_view-part_hardpoint = Befestigungspunkt: { $hardpoint }

## ui.model_view.part_group
ui-model_view-part_group-hull = Rumpf
ui-model_view-part_group-main_battery = Hauptbatterie
ui-model_view-part_group-secondary_battery = Sekundärbewaffnung
ui-model_view-part_group-aa_guns = Flak
ui-model_view-part_group-torpedoes = Torpedos
ui-model_view-part_group-depth_charges = Wasserbomben
ui-model_view-part_group-fire_control = Feuerleitung
ui-model_view-part_group-radar = Radare
ui-model_view-part_group-missiles = Raketenwerfer
ui-model_view-part_group-decorations = Dekorationen
ui-model_view-part_group-misc = Sonstiges
ui-model_view-part_group-armor = Panzerung
ui-model_view-part_group-other = Andere

## ui.scene_problem
ui-scene_problem-cannot_show = { $name } kann nicht angezeigt werden
ui-scene_problem-geometry_absent = Die Geometrie dieses Modells ist nicht in Ihren Spieldateien enthalten.
ui-scene_problem-model_unreadable = Die Daten dieses Modells konnten nicht gelesen werden. Details stehen im Protokoll.
ui-scene_problem-ship_unreadable = Die Daten dieses Schiffs konnten nicht gelesen werden. Details stehen im Protokoll.
ui-scene_problem-no_meshes = Dieses Modell hat keine Meshes.
ui-scene_problem-no_meshes_at_lod = Dieses Modell hat in der gewählten Detailstufe keine Meshes.
ui-scene_problem-no_visible_surface = Dieses Modell hat keine sichtbare Oberfläche zum Zeichnen.
ui-scene_problem-section_stand_in = Dieser Rumpfabschnitt hat keine eigene Geometrie. Das Schiff zeichnet diesen Teil des Rumpfs in einem anderen Abschnitt.

## ui.armor.option_tabs
ui-armor-option_tabs-label = Optionsregisterkarten
ui-armor-option_tabs-ship = Schiff
ui-armor-option_tabs-layers = Ebenen
ui-armor-option_tabs-analysis = Analyse
ui-armor-option_tabs-configuration = Konfiguration
ui-armor-option_tabs-hull_parts = Rumpfteile
ui-armor-option_tabs-tab_matches = { $tab }, { $count } Treffer
ui-armor-option_tabs-empty = Nichts auf dieser Registerkarte gilt für das gezeigte Modell.
ui-armor-option_tabs-no_choices = Dieses Modell hat keinen Rumpf, kein Modul, keine Tarnung und keine Detailstufe zur Auswahl.

## ui.armor.hidden_ship
ui-armor-hidden_ship-clan_rental = Clan-Leihschiff
ui-armor-hidden_ship-other_class = Keine Schiffsklasse

## ui.armor.textures
ui-armor-textures-title = Texturen
ui-armor-textures-hint = Das Spiel liest eine Datei im res_mods-Ordner des Builds anstelle der gepackten Datei mit demselben Pfad. Rechtsklicken Sie auf eine Textur, um diesen Pfad zu kopieren.
ui-armor-textures-under_pointer = Unter dem Zeiger: { $material }
ui-armor-textures-reading = Texturen werden gelesen...
ui-armor-textures-summary = { $materials } Materialien, { $textures } Texturen
ui-armor-textures-no_material = Kein Material
ui-armor-textures-unresolved = Der Pfad ist nicht in assets.bin enthalten
ui-armor-textures-absent = Nicht in den installierten Paketen
ui-armor-textures-unreadable = Der DDS-Header konnte nicht gelesen werden
ui-armor-textures-unknown_format = Unbekanntes Format
ui-armor-textures-details_one = { $format }, { $size }, 1 Datei, { $bytes }
ui-armor-textures-details = { $format }, { $size }, { $count } Dateien, { $bytes }
ui-armor-textures-role_albedo = Albedo
ui-armor-textures-role_normal = Normal
ui-armor-textures-role_metal_gloss = Metall-Glanz
ui-armor-textures-role_ambient_occlusion = Umgebungsverdeckung
ui-armor-textures-role_detail = Detail
ui-armor-textures-role_name_matched = Albedo, anhand des Dateinamens gefunden
ui-armor-textures-copy_path = Pfad kopieren
ui-armor-textures-copy_override = res_mods-Pfad kopieren
ui-armor-textures-copy_overrides = { $count } res_mods-Pfade kopieren
ui-armor-textures-paths_copied = { $count } Pfade kopiert
ui-armor-textures-no_build = Die Spieldaten wurden nicht geladen, daher ist der Mod-Ordner des Builds unbekannt.
ui-armor-textures-show_in_unpacker = Im Ressourcen-Entpacker anzeigen
ui-armor-textures-show_textures = Texturen anzeigen
ui-armor-textures-extract = Dateien extrahieren...
ui-armor-textures-extract_title = Texturdateien extrahieren nach
ui-armor-textures-extracting = Texturdateien werden extrahiert...
ui-armor-textures-extracted = { $count } Dateien nach { $folder } extrahiert
ui-armor-textures-extract_failed = Die Texturdateien konnten nicht extrahiert werden: { $reason }
ui-armor-textures-replace_title = Vorhandene Dateien ersetzen?
ui-armor-textures-replace_description = { $count } dieser Dateien befinden sich bereits in { $folder }. Beim Extrahieren werden sie ersetzt.
ui-armor-textures-replace = Ersetzen

## ui.settings.mcp
ui-settings-mcp-enabled = MCP-Server aktivieren
ui-settings-mcp-enabled_description = Stellt Replays und Spieldaten KI-Assistenten per MCP bereit. Zeigt ein Steuerelement in der Statusleiste.
ui-settings-mcp-start_on_launch = Mit dem Programm starten
ui-settings-mcp-start_on_launch_description = Startet den Server, wenn WoWs Toolkit geöffnet wird.
ui-settings-mcp-bind_address = Bind-Adresse
ui-settings-mcp-bind_address_description = Loopback-IP-Adresse, auf der der Server lauscht, etwa 127.0.0.1 oder ::1. Nur dieser Computer erreicht den Server.
ui-settings-mcp-port = Port
ui-settings-mcp-port_description = TCP-Port, auf dem gelauscht wird. Beim ersten Aktivieren zufällig gewählt und beibehalten.
ui-settings-mcp-port_unset = Wird beim ersten Aktivieren gewählt
ui-settings-mcp-address_not_loopback = { $input } ist keine Loopback-Adresse. Der Server lauscht nur auf diesem Computer, etwa auf 127.0.0.1.
ui-settings-mcp-address_invalid = { $input } ist keine IP-Adresse
ui-settings-mcp-port_invalid = { $input } ist kein Port von 1 bis 65535
ui-settings-mcp-port_in_use = Ein anderes Programm verwendet diesen Port. Wähle einen anderen Port.

## ui.mcp
ui-mcp-started = MCP-Server läuft unter { $url }
ui-mcp-copy_url = URL kopieren
ui-mcp-start_failed = MCP-Server kann nicht gestartet werden: { $reason }
ui-mcp-port_in_use = Port { $port } auf { $address } wird von einem anderen Programm verwendet
ui-mcp-no_watchdog = die Überwachung der Hintergrundprozesse läuft nicht
ui-mcp-status_running = MCP-Server läuft unter { $url }. Klicken zum Beenden.
ui-mcp-status_starting = MCP-Server startet auf { $address }
ui-mcp-status_failed = MCP-Server konnte nicht starten. Klicken zum Wiederholen.
ui-mcp-status_stopped = MCP-Server gestoppt. Klicken zum Starten.

## ui.launcher.opt_in
ui-launcher-opt_in-setting = Spiel-Launcher aktivieren
ui-launcher-opt_in-description = Spielen Sie World of Warships unter Linux direkt aus dem Toolkit, angemeldet mit Ihrem Wargaming-Konto.
ui-launcher-opt_in-title = Spiel-Launcher aktivieren?
ui-launcher-opt_in-notice_account = Durch die Nutzung der Launcher-Funktion erkennen Sie an, dass diese Funktion kein vom Spieleentwickler unterstütztes Szenario ist. Sie geben Ihr Passwort direkt in diese Anwendung ein, die, falls Sie ihr nicht vertrauen, dazu verwendet werden könnte, Ihr Konto zu stehlen (obwohl der Entwickler der Anwendung dies nicht beabsichtigt).
ui-launcher-opt_in-notice_reports = Ferner erkennen Sie an, dass Sie bei Nutzung dieser Funktion keine Grafik- oder Stabilitätsfehler an den Spieleentwickler melden werden, es sei denn, Sie sind wirklich überzeugt, dass es sich um ein plattformübergreifendes Problem handelt, das NICHT NUR Linux betrifft.
ui-launcher-opt_in-continue = Weiter
ui-launcher-opt_in-cancel = Abbrechen

## ui.launcher.keyring
ui-launcher-keyring-unencrypted = Ihre Anmeldung wird unverschlüsselt im Datenordner der App gespeichert, weil kein Secret-Service-Anbieter (etwa GNOME Keyring, KeePassXC oder KDE Wallet) verfügbar ist.
ui-launcher-keyring-how = So bleibt sie verschlüsselt
ui-launcher-keyring-how_detail = Installieren und starten Sie einen Secret-Service-Anbieter wie GNOME Keyring, KeePassXC oder KDE Wallet und starten Sie das Toolkit dann neu. Gespeicherte Anmeldungen werden dorthin übernommen.
ui-launcher-keyring-unlock_hint = Ihr System-Schlüsselbund fragt möglicherweise nach dem Entsperren, damit das Toolkit Ihre gespeicherte Anmeldung lesen kann.

## ui.launcher.play
ui-launcher-play-play = Spielen
ui-launcher-play-play_through_steam = Über Steam spielen
ui-launcher-play-set_up_game = Spiel einrichten
ui-launcher-play-set_up_runner = Proton einrichten
ui-launcher-play-sign_in_again = Erneut anmelden
ui-launcher-play-update = Aktualisieren { $size }
ui-launcher-play-updating = Wird aktualisiert { $percent }%
ui-launcher-play-updating_unknown = Wird aktualisiert
ui-launcher-play-paused = Pausiert
ui-launcher-play-starting = Wird gestartet...
ui-launcher-play-running = Läuft
ui-launcher-play-in_battle = Im Gefecht
ui-launcher-play-menu = Konten und Spieloptionen

## ui.launcher.popover
ui-launcher-popover-game = World of Warships
ui-launcher-popover-no_install = Keine Spielinstallation festgelegt.
ui-launcher-popover-update_available = Update verfügbar
ui-launcher-popover-check_failed = Update-Prüfung fehlgeschlagen
ui-launcher-popover-accounts = Konten
ui-launcher-popover-no_accounts = Noch keine Konten. Fügen Sie eines hinzu, um angemeldet zu spielen.
ui-launcher-popover-close_to_switch = Schließen Sie das Spiel, um das Konto zu wechseln.
ui-launcher-popover-default = Standard
ui-launcher-popover-ready = Bereit
ui-launcher-popover-restoring = Wird geprüft...
ui-launcher-popover-not_saved = Nicht gespeichert
ui-launcher-popover-unencrypted = Unverschlüsselt gespeichert
ui-launcher-popover-keyring_locked = Schlüsselbund gesperrt: Entsperren Sie ihn, um gespeicherte Anmeldungen wiederherzustellen
ui-launcher-popover-forget = Entfernen
ui-launcher-popover-sign_out = Abmelden
ui-launcher-popover-expires_in = Läuft ab in { $duration }
ui-launcher-popover-expired = Sitzung abgelaufen
ui-launcher-popover-expiry_unknown = Ablauf unbekannt
ui-launcher-popover-make_default = Als Standard festlegen
ui-launcher-popover-account_actions = Kontoaktionen
ui-launcher-popover-play_anyway = Trotzdem spielen
ui-launcher-popover-play_safe_mode = Im abgesicherten Modus spielen
ui-launcher-popover-add_account = Konto hinzufügen...
ui-launcher-popover-game_settings = Spieleinstellungen...
ui-launcher-popover-installs = Installationen und Einrichtung...
ui-launcher-popover-open_replay = Replay im Spiel öffnen...

## ui.launcher.palette
ui-launcher-palette-play = Spielen
ui-launcher-palette-play_as = Spielen als { $nickname } ({ $realm })
ui-launcher-palette-game_settings = Spieleinstellungen
ui-launcher-palette-add_account = Konto hinzufügen
ui-launcher-palette-update = World of Warships aktualisieren
ui-launcher-palette-open_replay = Replay im Spiel öffnen

## ui.launcher.installs
ui-launcher-installs-toolkit = Spielverzeichnis des Toolkits
ui-launcher-installs-played = Standardkonto spielt diese
ui-launcher-installs-play_from = Von hier spielen
ui-launcher-installs-update = Aktualisieren

## ui.launcher.wgc_login
ui-launcher-wgc_login-label = Wargaming-Anmeldung verwenden
ui-launcher-wgc_login-about = Startet diese Steam-Installation über die Anmeldung des Toolkits statt über Steam. Einstellungen und Replays werden getrennt von Steam-Starts gespeichert. Steam-Guthaben, Errungenschaften und Spielzeit funktionieren in diesem Modus nicht.
ui-launcher-wgc_login-steam_accounts = Über Steam erstellte Konten benötigen eine E-Mail-Adresse und ein Passwort. Fügen Sie diese zuerst in der Wargaming-Kontoverwaltung hinzu.
ui-launcher-wgc_login-steam_accounts_link = So fügen Sie E-Mail-Adresse und Passwort hinzu
ui-launcher-wgc_login-ready = Game-Center-Dateien bereit
ui-launcher-wgc_login-fetchable = Beim Einschalten werden die Game-Center-Dateien von WG für diesen Build heruntergeladen ({ $size }).
ui-launcher-wgc_login-downloading = Game-Center-Dateien werden heruntergeladen: { $percent }%
ui-launcher-wgc_login-downloading_unknown = Game-Center-Dateien werden heruntergeladen
ui-launcher-wgc_login-checking = Game-Center-Dateien für diesen Build werden gesucht...
ui-launcher-wgc_login-needs_sign_in = Fügen Sie dem Toolkit zuerst ein Wargaming-Konto hinzu.
ui-launcher-wgc_login-unavailable = Für diesen Build nicht verfügbar: { $error }

## ui.launcher.replay
ui-launcher-replay-no_install = Keine Installation hat den Spiel-Build { $build }. Behalten oder installieren Sie eine Kopie dieses Builds, um dieses Replay anzusehen.

## ui.launcher.setup
ui-launcher-setup-title = World of Warships einrichten
ui-launcher-setup-game = Spiel
ui-launcher-setup-proton = Proton
ui-launcher-setup-accounts = Konten
ui-launcher-setup-replays = Replays
ui-launcher-setup-game_question = Welche Kopie des Spiels soll das Toolkit verwenden?
ui-launcher-setup-none_found = Auf diesem Computer wurde keine Installation gefunden.
ui-launcher-setup-choose_folder = Ordner auswählen...
ui-launcher-setup-install_new = Neue Kopie installieren...
ui-launcher-setup-proton_explain = Unter Linux läuft das Spiel über Proton: das von Steam, wenn Steam installiert ist, sonst umu-run.
ui-launcher-setup-proton_ready = Proton wurde gefunden.
ui-launcher-setup-accounts_explain = Melden Sie sich bei den Wargaming-Konten an, mit denen Sie spielen. Spielen meldet das Spiel mit dem Standardkonto an.
ui-launcher-setup-replays_explain = Das Spiel löscht seine ältesten Replays, sobald es diese Anzahl erreicht.
ui-launcher-setup-skip = Einrichtung überspringen
ui-launcher-setup-back = Zurück
ui-launcher-setup-next = Weiter
ui-launcher-setup-done = Fertig

## ui.launcher.game_settings
ui-launcher-game_settings-title = Spieleinstellungen
ui-launcher-game_settings-build = Build { $build }
ui-launcher-game_settings-display = Anzeige
ui-launcher-game_settings-graphics = Grafik
ui-launcher-game_settings-sound = Ton
ui-launcher-game_settings-replays = Replays
ui-launcher-game_settings-locked = Das Spiel läuft. Es überschreibt diese Dateien beim Beenden, daher sind sie bis dahin schreibgeschützt.
ui-launcher-game_settings-footer = Wird in die Einstellungsdateien des Spiels geschrieben. Eine Sicherung wird angelegt.
ui-launcher-game_settings-cancel = Abbrechen
ui-launcher-game_settings-save = Speichern
ui-launcher-game_settings-saved = Spieleinstellungen gespeichert.
ui-launcher-game_settings-no_install = Keine Spielinstallation festgelegt.
ui-launcher-game_settings-window_mode = Fenstermodus
ui-launcher-game_settings-windowed = Fenster
ui-launcher-game_settings-borderless = Rahmenlos
ui-launcher-game_settings-fullscreen = Vollbild
ui-launcher-game_settings-monitor = Monitor
ui-launcher-game_settings-monitor_name = Monitorname
ui-launcher-game_settings-resolution = Vollbildauflösung
ui-launcher-game_settings-width = Breite
ui-launcher-game_settings-height = Höhe
ui-launcher-game_settings-refresh = Bildwiederholrate
ui-launcher-game_settings-display_rate = Anzeigerate
ui-launcher-game_settings-vsync = Vertikale Synchronisation
ui-launcher-game_settings-ui_scale = Oberflächenskalierung
ui-launcher-game_settings-battle_fps = Bildratenlimit im Gefecht
ui-launcher-game_settings-port_fps = Bildratenlimit im Hafen
ui-launcher-game_settings-unlimited = Unbegrenzt
ui-launcher-game_settings-skip_intro = Introvideo überspringen
ui-launcher-game_settings-master = Gesamtlautstärke
ui-launcher-game_settings-music_port = Musik im Hafen
ui-launcher-game_settings-music_battle = Musik im Gefecht
ui-launcher-game_settings-preset = Voreinstellung
ui-launcher-game_settings-antialiasing = Kantenglättung
ui-launcher-game_settings-textures = Texturqualität
ui-launcher-game_settings-filtering = Texturfilterung
ui-launcher-game_settings-shadows = Schattenqualität
ui-launcher-game_settings-graphics_note = Ändern Sie die Grafik im Spiel. Es prüft sie anhand Ihrer Hardware.
ui-launcher-game_settings-replays_kept = Zu behaltende Replays
ui-launcher-game_settings-replays_kept_zero = Behalten Sie mindestens ein Replay.
ui-launcher-game_settings-versioned = Replays nach Spielversion sortieren
ui-launcher-game_settings-replays_note = Wird in res_mods dieses Builds geschrieben. Der abgesicherte Modus verwendet es nicht.
ui-launcher-game_settings-absent = Nicht in den Spieldateien festgelegt
ui-launcher-game_settings-unrecognized = Unbekannter Wert: { $value }
ui-launcher-game_settings-not_a_number = { $field }: Geben Sie eine Zahl ein.

## ui.settings.launcher
ui-settings-launcher-installs = Spielinstallationen
ui-settings-launcher-installs_description = Die Installationen, die der Launcher für das Spiel startet und aktualisiert.
ui-settings-launcher-runner = Proton (Linux)
ui-settings-launcher-runner_description = Wie das Spiel unter Linux läuft. Proton Experimental oder das neueste Proton von Steam wird automatisch gefunden, sonst umu-run.
ui-settings-launcher-launch_args = Startargumente
ui-settings-launcher-launch_args_description = Zusätzliche Befehlszeilenargumente für das Spiel, eines pro Zeile.
ui-settings-launcher-environment = Umgebung
ui-settings-launcher-environment_description = Umgebungsvariablen für das Spiel, KEY=VALUE, eine pro Zeile.
ui-settings-launcher-safe_mode_args = Argumente für den abgesicherten Modus
ui-settings-launcher-safe_mode_args_description = Argumente, die beim Spielen im abgesicherten Modus hinzugefügt werden, eines pro Zeile.
