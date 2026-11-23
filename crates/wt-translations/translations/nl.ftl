# You are encouraged to suggest edits to this file on GitHub by either filing
# an issue, editing the file directly and creating a pull request, or just
# mentioning it in the Discord.
## meta
meta-language_name = Nederlands

# -- Taalselectie dialoog --

## dialog
dialog-continue_in_english = Continue in English
dialog-continue_in_language = Doorgaan in het Nederlands
dialog-select_language = Taal selecteren
dialog-machine_translation_warning = De meeste vertalingen zijn machinaal gegenereerd en kunnen onnauwkeurig zijn.

# -- Tabtitels --

## ui.tabs
ui-tabs-unpacker = Bronnen uitpakken
ui-tabs-settings = Instellingen
ui-tabs-replay_parser = Replay-inspecteur
ui-tabs-player_tracker = Spelersvolger
ui-tabs-armor_viewer = Pantserweergave
ui-tabs-model_viewer = Modelweergave
ui-tabs-stats = Statistieken
ui-tabs-directory_stats = Statistieken { $dir }
ui-tabs-close_directory_stats = Mapstatistieken sluiten
ui-tabs-search = Zoeken
ui-tabs-search_these_replays_unavailable = Deze replays zijn nog niet geïndexeerd
ui-tabs-zoom_in_panel = Inzoomen
ui-tabs-zoom_out_panel = Uitzoomen
ui-tabs-close_panel = Sluiten
ui-tabs-close_tab = Sluiten
ui-tabs-close_other_tabs = Andere sluiten
ui-tabs-close_tabs_to_the_right = Rechts sluiten

# -- Algemene knoppen --

## ui.buttons
ui-buttons-yes = Ja
ui-buttons-no = Nee
ui-buttons-send_replays = Replays sturen
ui-buttons-send_build_data = Alleen buildgegevens sturen
ui-buttons-send_nothing = Niets sturen
ui-buttons-switch_to_replays = Overschakelen naar replays
ui-buttons-keep_current = Huidige instelling behouden
ui-buttons-stop = Stoppen
ui-buttons-copy = Kopiëren
ui-buttons-cancel = Annuleren
ui-buttons-continue_ = Doorgaan
ui-buttons-install_update = Update installeren
ui-buttons-view_release = Release bekijken
ui-buttons-clear_settings = Instellingen wissen
ui-buttons-more_options = Meer opties
ui-buttons-create_issue = Issue aanmaken
ui-buttons-discord = Discord
ui-buttons-github = GitHub
ui-buttons-view_github = Project op GitHub bekijken
ui-buttons-choose = Kiezen...
ui-buttons-reset = Herstellen
ui-buttons-clear = Wissen
ui-buttons-select_all = Alles selecteren
ui-buttons-download = Downloaden
ui-buttons-retry = Opnieuw proberen
ui-buttons-refresh_persisted_data_confirm = Vernieuwen

# -- Menu-items --

## ui.menu
ui-menu-file = Bestand
ui-menu-check_updates = Controleren op updates
ui-menu-about = Over
ui-menu-quit = Afsluiten

# -- Venstertitels --

## ui.windows
ui-windows-build_consent = Toestemming buildverzameling
ui-windows-replay_migration = Overschakelen naar replays delen
ui-windows-crash_detected = Applicatiecrash gedetecteerd
ui-windows-update_available = Update beschikbaar
ui-windows-network_warning = Netwerkwaarschuwing
ui-windows-download_game_data = Spelgegevens downloaden
ui-windows-error = Fout
ui-windows-software_encode = Geen GPU-video-encoder
ui-windows-about = Over
ui-windows-refresh_persisted_data = Opgeslagen replaygegevens vernieuwen
ui-windows-tactics_board = Tactiekbord
ui-windows-minimize = Minimaliseren
ui-windows-maximize = Maximaliseren
ui-windows-restore = Herstellen
ui-windows-close = Sluiten

# -- Dialoog- / berichttekst --

## ui.dialogs
ui-dialogs-build_consent_message = Wilt u buildinformatie van spelers uit classificatie- en willekeurige gevechten naar de ontwikkelaar sturen? Deze gegevensverzameling helpt de gemeenschap om builds van spelers in bulk te analyseren. U kunt dit op elk moment uitschakelen in de instellingen.
ui-dialogs-replay_migration_message = Toolkit kan nu volledige replays delen, wat een veel rijkere matchmaking-analyse mogelijk maakt dan alleen buildgegevens. Gevechten waarin u een testschip speelt worden niet gedeeld. Wilt u overschakelen naar het delen van ruwe replays?
ui-dialogs-crash_message = Het lijkt erop dat WoWs Toolkit de vorige keer is gecrasht. Als u dit probleem wilt melden, maak dan een issue aan op GitHub of sluit u aan bij de Discord-server en geef de onderstaande informatie.
ui-dialogs-crash_clear_settings = Als om de een of andere reden de opgeslagen gegevens van de applicatie verantwoordelijk zouden kunnen zijn, kunt u proberen de instellingen te wissen door op de onderstaande knop te drukken. Dit wist alle instellingen. Gevolgde spelers blijven behouden. Uw replays en WoWs-gegevens blijven veilig.
ui-dialogs-update_message = Versie { $tag } van WoWs Toolkit is beschikbaar
ui-dialogs-update_notes = Release-opmerkingen
ui-dialogs-update_asset_pending = De Windows-download is nog niet beschikbaar. Bekijk de release voor de buildstatus.
ui-dialogs-update_windows_only = Update beschikbaar, maar momenteel wordt alleen Windows ondersteund.
ui-dialogs-p2p_warning = Samenwerkingssessies gebruiken P2P-netwerken. Andere gebruikers in de sessie kunnen mogelijk uw IP-adres zien.
ui-dialogs-software_encode_message = Er is geen ondersteunde GPU-video-encoder gevonden, dus deze export wordt softwarematig gecodeerd. Dat is veel trager en een lang gevecht kan enkele minuten duren.
ui-dialogs-download_game_data_intro = Sommige replays hebben spelgegevens nodig die niet zijn geïnstalleerd. Kies de builds die u wilt downloaden.
ui-dialogs-download_build_row = Versie { $version } (build { $build })
ui-dialogs-download_replays_needing = nodig voor { $count ->
    [one] { $count } replay
   *[other] { $count } replays
}
ui-dialogs-download_availability_exact = gepubliceerd
ui-dialogs-download_availability_nearest = geen exacte overeenkomst, dichtstbijzijnde is { $version } (build { $build })
ui-dialogs-download_availability_unpublished = nooit gepubliceerd
ui-dialogs-download_availability_unreachable = gepubliceerd, maar de bestandslijst kon niet worden gelezen
ui-dialogs-download_plan_pending = Bepalen wat er opgehaald moet worden...
ui-dialogs-download_objects_to_fetch = Op te halen objecten voor de aangevinkte builds: { $count }
ui-dialogs-download_plan_failed = Kan niet bepalen wat er opgehaald moet worden
ui-dialogs-download_plan_no_cache_dir = Er is geen cachemap voor spelgegevens ingesteld. Kies er een in Instellingen en probeer het opnieuw.
ui-dialogs-refresh_persisted_data_message = Elke replay in uw replaymap opnieuw parsen en de opgeslagen gegevens herschrijven. Vermeldingen die zijn vastgelegd met een replaydatamapping die niet overeenkomt met hun spelbuild blijven ongewijzigd, behalve dat persoonlijke beoordeling en spelmodus voor elke replay opnieuw worden berekend op basis van de huidige verwachte waarden. Dit kan even duren.
ui-dialogs-file_dialog_failed = Het bestandsvenster kon niet worden geopend: { $reason }

# -- Labels --

## ui.labels
ui-labels-error_occurred = Er is een fout opgetreden:
ui-labels-made_by = Gemaakt door landaire.
ui-labels-credits = Met dank aan Trackpad, TTaro, lkolbly voor hun bijdragen.
ui-labels-pr_credits = Persoonlijke beoordeling (PR) berekeningsgegevens en formule geleverd door WoWs Numbers.
ui-labels-suppress_warning = Niet meer weergeven
ui-labels-more_info = Meer informatie
ui-labels-app_version = Versie { $version }
ui-labels-project_license = WoWs Toolkit wordt verspreid onder de { $license }-licentie.
ui-labels-third_party_notices = Kennisgevingen van derden
ui-labels-notices_search_hint = Kennisgevingen doorzoeken
ui-labels-no_matching_notices = Geen kennisgevingen komen overeen met de zoekopdracht.

# -- Toastberichten --

## ui.messages
ui-messages-bug_report_info_copied = Informatie voor bugrapport gekopieerd.
ui-messages-update_downloading = Update downloaden: { $read } van { $total }
ui-messages-update_downloading_unknown = Update downloaden: { $read } tot nu toe
ui-messages-app_up_to_date = Applicatie is up-to-date
ui-messages-checking_app_updates = Controleren op app-updates...
ui-messages-update_starting = Update downloaden...
ui-messages-update_check_failed = Kan niet controleren op app-updates
ui-messages-game_data_loaded = Spelgegevens succesvol geladen
ui-messages-no_replays_detected = Geen replays gevonden. Is uw WoWs-map correct geconfigureerd?
ui-messages-game_data_builds_downloaded = Spelgegevens gedownload voor { $count ->
    [one] { $count } build
   *[other] { $count } builds
}
ui-messages-game_data_download_failed = Kan spelgegevens niet downloaden
ui-messages-game_data_up_to_date = Spelgegevens zijn up-to-date
ui-messages-game_data_updates_available = { $count ->
    [one] Voor { $count } gecachte build zijn nieuwere gegevens beschikbaar
   *[other] Voor { $count } gecachte builds zijn nieuwere gegevens beschikbaar
}
ui-messages-game_data_cache_valid = Spelgegevenscache is geldig en komt overeen met de externe repository
ui-messages-game_data_cache_invalid = { $count ->
    [one] { $count } gecachte build is verouderd of beschadigd
   *[other] { $count } gecachte builds zijn verouderd of beschadigd
}
ui-messages-replay_load_failed = Kan replay niet laden
ui-messages-twitch_token_invalid = Twitch-token is ongeldig of verlopen. Werk het bij in Instellingen.
ui-messages-wows_dir_invalid = World of Warships-map is niet geldig. Controleer het pad in Instellingen.
ui-messages-wows_dir_not_set = World of Warships-map is niet ingesteld
ui-messages-constants_version_mismatch =
    De versie van het replaydatamappingbestand komt niet overeen met de spelversie.
    Resultaten na het gevecht zijn mogelijk niet accuraat. Wees geduldig terwijl de projectbeheerders de mapping op de server bijwerken.
ui-messages-session_started = Sessie gestart
ui-messages-user_joined = { $name } is toegetreden
ui-messages-user_timeout = { $name } is verlopen
ui-messages-user_left = { $name } is vertrokken
ui-messages-session_ended = Sessie beëindigd
ui-messages-session_error = Sessiefout: { $msg }
ui-messages-connected_to_session = Verbonden met sessie
ui-messages-host_opened_replay = Host heeft replay geopend: { $name }
ui-messages-host_closed_replay = Host heeft een replay gesloten
ui-messages-session_rejected = Sessie geweigerd: { $reason }
ui-messages-drop_not_a_replay = Dat is geen replaybestand
ui-messages-drop_not_replays = Dat zijn geen replaybestanden
ui-messages-drop_skipped = { $count ->
    [one] { $count } neergezet bestand overgeslagen dat geen replay of map is
   *[other] { $count } neergezette bestanden overgeslagen die geen replays of mappen zijn
}
ui-messages-log_copied = { $bytes } KB uit { $file } naar het klembord gekopieerd
ui-messages-log_copy_failed = Kan een logbestand niet lezen: { $error }
ui-messages-log_not_found = Geen logbestand gevonden in { $dir }

# -- Statistieknamen --

## stat
stat-damage = Schade
stat-spotting_damage = Verkenningsschade
stat-frags = Eliminaties
stat-raw_xp = Ruwe XP
stat-base_xp = Basis XP
stat-win_rate = Winstpercentage
stat-personal_rating = Persoonlijke beoordeling
stat-match_contribution = Gevechtsbijdrage
stat-capture_points = Veroveringspunten
stat-planes_shot_down = Neergeschoten vliegtuigen
stat-anti_air_damage = Luchtafweerschade
stat-survival_rate = Overlevingspercentage
stat-torpedoes_spotted = Ontdekte torpedo's
stat-avg_pr = Gem. PR
stat-avg_prefix = Gem. { $name }

## chart
chart-combined = Gecombineerd
chart-combined_suffix = (Gecombineerd)
chart-rolling_average_suffix = (Voortschrijdend gemiddelde)

# -- Bevestigingsdialogen --

## confirm
confirm-open_in_game = Dit start World of Warships. Doorgaan?
confirm-clear_all_session_stats = Dit wist alle sessiestatistieken. Doorgaan?
confirm-set_as_session_stats = Dit vervangt uw huidige sessiestatistieken. Doorgaan?

# -- Foutmeldingen --

## error
error-file_watcher_creation = Kan bestandswatcher niet aanmaken: { $error }
error-replay_dir_watch = Kan replaymap niet bewaken: { $error }

# -- Instellingen --

## ui.settings.app
ui-settings-app-check_for_updates = Controleren op updates bij opstarten
ui-settings-app-enable_logging = Logboek inschakelen (van kracht na herstart)
ui-settings-app-enable_logging_tooltip = Schrijf debuglogboeken naar een bestand in de applicatiegegevensmap. Nuttig voor probleemoplossing.
ui-settings-app-data_sharing_mode = Gegevens delen (classificatie- en willekeurige gevechten)
ui-settings-app-theme = Thema
ui-settings-app-theme_system = Systeem
ui-settings-app-theme_dark = Donker
ui-settings-app-theme_light = Licht
ui-settings-app-code_integrity = Niet-ondertekende code blokkeren (van kracht na herstart)
ui-settings-app-code_integrity_tooltip = Weigert code te laden die niet door Microsoft is ondertekend, waardoor overlays, opnametools en audiohulpprogramma's buiten deze applicatie blijven. Die zijn een veelvoorkomende oorzaak van crashes en vastlopers tijdens het renderen. Grafische stuurprogramma's zijn via Windows ondertekend en blijven werken. Dit kan niet meer worden uitgeschakeld zodra een sessie is gestart, dus een wijziging geldt vanaf de volgende start. Automatisch slaat dit over wanneer u een invoermethode van derden hebt geïnstalleerd, die anders geblokkeerd zou worden. Let op: andere beveiligingen blijven actief ongeacht deze instelling; start met --no-hardening om ze allemaal uit te schakelen.
ui-settings-app-code_integrity_automatic = Automatisch
ui-settings-app-code_integrity_always = Altijd
ui-settings-app-code_integrity_never = Nooit
ui-settings-app-language = Taal
ui-settings-app-open_data_dir_tooltip = Open de applicatiegegevensmap (instellingen, logboeken, gecachte gegevens) in uw bestandsbeheerder.
ui-settings-app-proxy_url = HTTP-proxy
ui-settings-app-proxy_url_hint_live = host:poort, of leeg laten om de proxy-instellingen van het systeem te gebruiken

## ui.settings.wows
ui-settings-wows-description = Pad naar uw World of Warships-installatie
ui-settings-wows-directory_hint = World of Warships-map
ui-settings-wows-browse = Bladeren...

## ui.settings.wows.cache
ui-settings-wows-cache-heading = Spelgegevenscache
ui-settings-wows-cache-description = Uitgepakte spelgegevens, zodat replays van oudere builds nog steeds openen
ui-settings-wows-cache-auto_dump = Spelgegevens cachen voor compatibiliteit met oudere replays
ui-settings-wows-cache-browse = Bladeren
ui-settings-wows-cache-open_folder = Map openen
ui-settings-wows-cache-delete_old = Oude versies verwijderen
ui-settings-wows-cache-stats = Spelgegevenscache: { $size } in { $count ->
    [one] { $count } versie
   *[other] { $count } versies
}
ui-settings-wows-cache-check_updates = Controleren op updates
ui-settings-wows-cache-actions = Cache-acties
ui-settings-wows-cache-actions_tooltip = Controleren op updates of gecachte builds valideren
ui-settings-wows-cache-update_all = Alles bijwerken
ui-settings-wows-cache-updates_available = { $count ->
    [one] Voor { $count } gecachte build zijn nieuwere gegevens beschikbaar
   *[other] Voor { $count } gecachte builds zijn nieuwere gegevens beschikbaar
}
ui-settings-wows-cache-validate = Cache valideren
ui-settings-wows-cache-repair = Repareren
ui-settings-wows-cache-repair_needed = { $count ->
    [one] { $count } gecachte build is verouderd of beschadigd
   *[other] { $count } gecachte builds zijn verouderd of beschadigd
}

## ui.settings.replay
ui-settings-replay-show_raw_xp = Ruwe XP weergeven
ui-settings-replay-show_entity_id = Entiteits-ID weergeven
ui-settings-replay-show_observed_damage = Waargenomen schade weergeven
ui-settings-replay-enable_previews = Minimapvoorbeeld tonen bij aanwijzen
ui-settings-replay-enable_previews_tooltip = Bij het aanwijzen van een replay wordt deze op de achtergrond gelezen en afgespeeld om een doorlopend minimapvoorbeeld op te bouwen. Schakel dit uit bij trage opslag.
ui-settings-replay-auto_export_data = Automatische gegevensexport
ui-settings-replay-export_path_hint = Map voor automatische gegevensexport
ui-settings-replay-export_path_missing = De map bestaat niet. Er kunnen geen gegevens naar worden geëxporteerd.
ui-settings-replay-index_all_replays = Alle replays indexeren
ui-settings-replay-show_heals = Reparaties weergeven
ui-settings-replay-current_path = Huidig replaypad

## ui.settings.session
ui-settings-session-display_name = Weergavenaam
ui-settings-session-suppress_ip_warning = P2P IP-adreswaarschuwing niet tonen
ui-settings-session-ip_warning_tooltip = Samenwerkingssessies gebruiken P2P-netwerken. Andere gebruikers in de sessie kunnen mogelijk uw IP-adres zien.
ui-settings-session-disable_auto_open = Automatisch openen van sessievensters uitschakelen
ui-settings-session-auto_open_tooltip = Indien ingeschakeld, worden sessievensters (replays, tactiekborden) niet automatisch geopend. U kunt ze nog steeds openen vanuit de sessiepopover.

## ui.settings.twitch
ui-settings-twitch-get_token = Inlogtoken ophalen
ui-settings-twitch-get_token_tooltip = We gebruiken de inlogpagina van Chatterino omdat deze een token levert met de benodigde rechten (in feite een moderatortoken met chatrechten), en het maakt het niet nodig voor de WoWs Toolkit-ontwikkelaar om een eigen inlogpagina te hosten die hetzelfde resultaat zou opleveren.
ui-settings-twitch-paste_token_no_token = Token plakken (geen token ingesteld)
ui-settings-twitch-paste_token_tooltip = Leest de inloggegevens van het klembord
ui-settings-twitch-paste_token_valid = Token plakken (token geldig)
ui-settings-twitch-paste_token_unvalidated = Token controleren
ui-settings-twitch-paste_token_invalid = Token plakken (token ongeldig / verlopen)
ui-settings-twitch-monitored_channel = Bewaakt kanaal (standaard: uzelf)
ui-settings-twitch-signed_in_as = Aangemeld als { $who }

# -- Replay-inspecteur --

## ui.replay
ui-replay-copy_bug_report_info = Informatie voor bugrapport kopiëren
ui-replay-match_age = { $age } geleden
ui-replay-listing_caption = Replays
ui-replay-loading_game_data_version = Spelgegevens laden voor { $version }...
ui-replay-spectator = Toeschouwer
ui-replay-loading = Laden...
ui-replay-preview_loading = Voorbeeld opbouwen...
ui-replay-preview_no_game_data = Geen spelgegevens voor clientversie { $value }
ui-replay-preview_unreadable_because = Deze replay kon niet worden gelezen: { $value }
ui-replay-preview_unwalkable = Het gevecht van deze replay kon niet worden verwerkt: { $value }
ui-replay-preview_unknown_version = Deze replay meldt een onleesbare clientversie: { $value }
ui-replay-preview_no_map = Geen kaartgegevens voor { $value }
ui-replay-preview_render_failed = Het voorbeeld kon niet worden getekend: { $value }
ui-replay-incomplete_results = Onvolledige wedstrijdresultaten
ui-replay-incomplete_results_tooltip = De replay heeft nog geen eindresultaten. Gegevens worden automatisch opnieuw geladen wanneer de wedstrijd eindigt en de eindresultaten aan de replay worden toegevoegd.
ui-replay-no_chat = Er zijn geen chatberichten verzonden in deze replay
ui-replay-chat_copy_all = Alles kopiëren
ui-replay-chat_save_to_file = Opslaan als bestand
ui-replay-chat_save_title = Spelchat opslaan
ui-replay-export_results_title = Resultaten exporteren
ui-replay-results_exported = Resultaten geëxporteerd
ui-replay-chat_saved = Chat opgeslagen in { $path }
ui-replay-chat_save_failed = De chat is niet opgeslagen: { $error }
ui-replay-team_damage = Teamschade:{" "}
ui-replay-team_share = { $pct } van het teamtotaal ({ $team })
ui-replay-hide_my_stats = Mijn testschipstatistieken verbergen
ui-replay-open_manually = Replaybestand handmatig openen...
ui-replay-open_directory = Replaymap openen...
ui-replay-search_these_replays = Deze replays doorzoeken
ui-replay-import_constants = constants.json-bestand importeren...
ui-replay-refresh_persisted_data = Opgeslagen replaygegevens vernieuwen...
ui-replay-contributing = { $done } van { $total ->
    [one] { $total } replay
   *[other] { $total } replays
} bijdragen
ui-replay-contributed = { $sent } van { $total ->
    [one] { $total } replay
   *[other] { $total } replays
} bijgedragen
ui-replay-contribute_nothing_shared = Kies eerst in Instellingen wat u wilt delen
ui-replay-contribute_none_listed = Er staan geen replays in de lijst om bij te dragen
ui-replay-contribute_not_ready = Replays kunnen worden bijgedragen zodra de spelgegevens en de instellingen zijn geladen
ui-replay-contribute_already_running = Er loopt al een bijdrageronde
ui-replay-auto_export_failed = Een gevecht kon niet worden weggeschreven: { $error }
ui-replay-auto_exported = Gevecht geëxporteerd naar { $file }
ui-replay-opened_new_battle = Het nieuwe gevecht is geopend: { $battle }
ui-replay-new_battle_listed = Nieuw gevecht: { $battle }
ui-replay-results_arrived = De gevechtsresultaten voor { $battle } zijn binnen
ui-replay-constants_written = { $count ->
    [one] De resultaatmapping voor { $count } build is gedownload
   *[other] Resultaatmappings voor { $count } builds zijn gedownload
}
ui-replay-constants_import_title = Een constants.json importeren
ui-replay-constants_imported = De resultaatmapping voor build { $build } is geïmporteerd
ui-replay-constants_import_failed = Die mapping is niet geïmporteerd: { $reason }
ui-replay-constants_no_build = Laad eerst een spelbuild
ui-replay-constants_latest_written = De nieuwste resultaatmapping is gedownload
ui-replay-constants_check_failed = De resultaatmappings konden niet worden gecontroleerd: { $reason }
ui-replay-constants_build_label = build { $build }
ui-replay-constants_generating = Spelconstanten bijwerken voor { $version }...
ui-replay-constants_generated = Spelconstanten voor { $version } gegenereerd in { $seconds } s
ui-replay-constants_generation_failed = Spelconstanten voor { $version } konden niet worden bijgewerkt: { $reason }. { $fallback }
ui-replay-constants_fallback_kept = De al opgeslagen constanten worden behouden.
ui-replay-constants_fallback_published = De gepubliceerde constanten voor build { $build } worden gebruikt.
ui-replay-constants_fallback_embedded = De ingebouwde constanten worden gebruikt.
ui-replay-copy_latest_log = Nieuwste logboek naar klembord kopiëren
ui-replay-load_alt_perspective = Perspectief van ander team laden...
ui-replay-load_alt_perspective_failed = Samenvoegen van replay mislukt: { $error }
ui-replay-load_alt_perspective_limit = Dit gevecht wordt al gelezen via { $limit ->
    [one] { $limit } andere opname
   *[other] { $limit } andere opnamen
}
ui-replay-consumable_charges_remaining = { $remaining } over van { $total ->
    [one] { $total } lading
   *[other] { $total } ladingen
}
ui-replay-consumable_charges_unlimited = { $used } gebruikt (onbeperkte ladingen)
ui-replay-consumable_header_remaining = Resterend
ui-replay-consumable_header_total = Totaal
ui-replay-consumable_charges_infinite = ∞
ui-replay-autoload_latest = Nieuwste replay automatisch laden
ui-replay-autoload_latest_tooltip = Een wedstrijd openen zodra het spel klaar is met wegschrijven
ui-replay-anonymize_names = Spelersnamen anonimiseren
ui-replay-anonymize_names_tooltip = Gegenereerde namen tonen in plaats van spelersnamen en clantags. Exports behouden de echte namen.
ui-replay-group_win_rate_all_modes = Alle spelmodi meetellen in het winstpercentage van groepen
ui-replay-group_win_rate_all_modes_tooltip = Standaard tellen groepskoppen alleen willekeurige en gerangschikte gevechten. Als dit aan staat, tellen alle spelmodi mee en krijgt het winstpercentage geen kleur, omdat de kleuren zijn afgestemd op willekeurige gevechten.
ui-replay-select_replay = Selecteer een replay
ui-replay-drop_hint = Sleep hier een replay naartoe om die te openen, of een map om de replays erin te tonen
ui-replay-scanning = Replays scannen...
ui-replay-scanning_count = Replay { $done } van { $total } lezen
ui-replay-no_replays_in = Geen replays in { $folder }
ui-replay-drop_folder_hint = Sleep een map naar het venster om die in een eigen tabblad te tonen
ui-replay-new_tag = Nieuw
ui-replay-loading_game_data = Spelgegevens laden...
ui-replay-game_data_failed = Spelgegevens konden niet worden geladen: { $error }
ui-replay-group_label = Replaygroepering
ui-replay-column_filters = Kolomfilters
ui-replay-row_elements = Rij-elementen
ui-replay-row_element_personal_rating = Persoonlijke beoordeling
ui-replay-row_element_match_contribution = Gevechtsbijdrage
ui-replay-row_element_division = Divisie
ui-replay-row_element_twitch = Twitch-chat
ui-replay-row_element_damage = Schade
ui-replay-row_element_kills = Eliminaties
ui-replay-row_element_time = Gevechtstijd
ui-replay-listing_options = Lijstopties
ui-replay-open = Openen
ui-replay-open_file = Replaybestand openen...
ui-replay-check_game_data = Spelgegevens controleren
ui-replay-check_game_data_tooltip = Controleren op spelgegevens die nodig zijn voor replays in deze map
ui-replay-close_side_panel = Paneel sluiten
ui-replay-nda = NDA
ui-replay-chat = Chat
ui-replay-game_chat = Spelchat
ui-replay-save_to_file = Opslaan als bestand
ui-replay-chat_copied = Chat gekopieerd
ui-replay-copy_message = Bericht kopiëren
ui-replay-actions = Acties
ui-replay-row_actions_hint = Spelersacties (of klik met rechts op de rij)
ui-replay-section_match = Wedstrijd
ui-replay-section_export = Exporteren
ui-replay-section_debug = Debug
ui-replay-timeline = Wedstrijdtijdlijn
ui-replay-row_not_indexed = niet geïndexeerd
ui-replay-hover_survived = Overleefd
ui-replay-hover_sunk = Gezonken
ui-replay-hover_not_indexed = Niet geïndexeerd
ui-replay-row_kills = { $count ->
    [one] { $count } eliminatie
   *[other] { $count } eliminaties
}
ui-replay-row_survived = in leven
ui-replay-row_sunk = gezonken
ui-replay-row_division = Divisie: { $members }
ui-replay-export_results_json = Resultaten: JSON
ui-replay-export_results_cbor = Resultaten: CBOR
ui-replay-export_results_csv = Resultaten: CSV
ui-replay-timeline_filter_all = Alle
ui-replay-timeline_filter_none = Geen
ui-replay-timeline_search_hint = Zoek schip of speler
ui-replay-timeline_no_events = Geen belangrijke gebeurtenissen gedetecteerd.
ui-replay-timeline_no_matches = Geen gebeurtenissen komen overeen met het huidige filter.
ui-replay-timeline_parsing = Gebeurtenissen parsen...
ui-replay-timeline_failed = Kan de wedstrijdtijdlijn niet extraheren: { $error }
ui-replay-timeline_retry = Opnieuw proberen
ui-replay-timeline_export_tooltip = Geldt voor de gebeurtenissen die na filtering zichtbaar zijn.
ui-replay-timeline_copied = Tijdlijn gekopieerd
ui-replay-timeline_event_copied = Gebeurtenis gekopieerd
ui-replay-timeline_saved = Tijdlijn opgeslagen in { $path }
ui-replay-timeline_save_failed = De tijdlijn is niet opgeslagen: { $error }
ui-replay-timeline_kind_health_lost = Zware schade
ui-replay-timeline_kind_death = Vernietigd
ui-replay-timeline_kind_cap_contested = Innamepunt betwist
ui-replay-timeline_kind_cap_flipped = Innamepunt veroverd
ui-replay-timeline_kind_cap_being_captured = Innamepunt wordt veroverd
ui-replay-timeline_kind_radar = Radar gebruikt
ui-replay-timeline_kind_advantage = Voordeelwijziging
ui-replay-timeline_kind_disconnected = Verbinding verbroken

## ui.replay.context
ui-replay-context-open_in_new_tab = Openen in nieuw tabblad
ui-replay-context-copy_cell = Cel kopiëren
ui-replay-context-copy_row = Rij kopiëren
ui-replay-context-copy_table = Tabel kopiëren
ui-replay-context-pin_column = Kolom vastzetten
ui-replay-context-unpin_column = Kolom losmaken
ui-replay-context-copied = Gekopieerd
ui-replay-context-copy_path = Pad kopiëren
ui-replay-context-copy_replays = { $count ->
    [one] Replay kopiëren
   *[other] { $count } replays kopiëren
}
ui-replay-context-copy_paths = { $count ->
    [one] Pad kopiëren
   *[other] { $count } paden kopiëren
}
ui-replay-context-show_in_explorer = Weergeven in Verkenner
ui-replay-context-show_replay_controls = Replaybediening weergeven
ui-replay-context-open_in_game = Openen in spel
ui-replay-context-render_replay = Replay renderen
ui-replay-context-set_session_stats = { $count ->
    [one] Instellen als sessiestatistieken ({ $count } replay)
   *[other] Instellen als sessiestatistieken ({ $count } replays)
}
ui-replay-context-add_session_stats = { $count ->
    [one] Toevoegen aan sessiestatistieken ({ $count } replay)
   *[other] Toevoegen aan sessiestatistieken ({ $count } replays)
}
ui-replay-context-render_to_video = { $count ->
    [one] Exporteren als video
   *[other] { $count } replays exporteren als video
}
ui-replay-context-render_to_clipboard = { $count ->
    [one] Exporteren naar klembord
   *[other] { $count } replays exporteren naar klembord
}

## ui.replay.column
ui-replay-column-player_name = Spelersnaam
ui-replay-column-base_xp = Basis XP
ui-replay-column-ship_name = Scheepsnaam
ui-replay-column-hits = Treffers
ui-replay-column-heals = Reparaties
ui-replay-column-heals_tooltip = Aantal waargenomen activaties van het Reparatieteam voor deze speler. Kan onnauwkeurig zijn voor schepen die niet op het scherm worden weergegeven (d.w.z. vijandelijke schepen die nooit zijn gespot).
ui-replay-column-heals_no_repair_tooltip = Dit schip heeft geen Reparatieteam-verbruiksartikel.
ui-replay-column-kills = Eliminaties
ui-replay-column-observed_damage = Waargenomen schade
ui-replay-column-actual_damage = Werkelijke schade
ui-replay-column-potential_damage = Potentiële schade
ui-replay-column-time_lived = Tijd geleefd
ui-replay-column-fires = Branden
ui-replay-column-floods = Overstromingen
ui-replay-column-citadels = Citadellen
ui-replay-column-crits = Kritiek
ui-replay-column-received_damage = Ontvangen schade
ui-replay-column-distance_traveled = Afgelegde afstand
ui-replay-column-skills = Vaardigheden
ui-replay-column-personal_rating = PR
ui-replay-column-match_contribution = MC

## ui.rating
ui-rating-personal_rating = Persoonlijke beoordeling
ui-rating-match_contribution = Gevechtsbijdrage
ui-rating-versus_average = t.o.v. gemiddelde
ui-rating-damage = Schade
ui-rating-kills = Eliminaties
ui-rating-wins = Overwinning
ui-rating-team_damage_share = Aandeel in teamschade
ui-rating-tanking = Aangetrokken vuur
ui-rating-spotting = Spotten
ui-rating-objectives = Doelen
ui-rating-anti_air = Luchtafweer
ui-rating-torpedo_warnings = Torpedowaarschuwingen
ui-rating-critical_bursts = Kritieke salvo's
ui-rating-unrated_early_exit = Geen MC: de opname eindigde voor het gevecht
ui-rating-mc_unrated_battle_type = Geen MC: gevechtsbijdrage geldt alleen voor willekeurige en classificatiegevechten

## ui.rating.detail
ui-rating-detail-versus_expected = { $actual } t.o.v. { $expected } verwacht
ui-rating-detail-wins = { $actual } gewonnen t.o.v. { $expected } verwacht
ui-rating-detail-weighted_damage = { $actual } t.o.v. { $expected } verwacht, schade aan torpedobootjagers en onderzeeërs x1.2
ui-rating-detail-team_damage_share = { $share }% van de teamschade aan vijanden, { $even }% is een gelijk aandeel
ui-rating-detail-tanking = { $potential } potentiële schade, { $class }x het klassegemiddelde, { $team }x een gelijk aandeel van het team
ui-rating-detail-spotting = { $damage } spotschade (klassegemiddelde { $baseline }), { $ships } HP aan schepen gespot (gemiddeld { $ships_baseline })
ui-rating-detail-objectives = Scorepunten: { $capture } veroveren, { $block } blokkeren, { $reset } terugzetten, { $battle_end } vastgehouden aan het einde, { $hold } vasthouden (voor een kwart geteld)
ui-rating-detail-anti_air_planes = { $strike ->
    [one] { $strike } aanvalsvliegtuig
   *[other] { $strike } aanvalsvliegtuigen
}, { $support ->
    [one] { $support } jager of verkenner
   *[other] { $support } jagers en verkenners
}
ui-rating-detail-anti_air_damage = { $damage } luchtafweerschade
ui-rating-detail-torpedo_warnings = { $threatening ->
    [one] { $threatening } bedreigende torpedo gespot
   *[other] { $threatening } bedreigende torpedo's gespot
}
ui-rating-detail-critical_bursts = { $count ->
    [one] { $count } kritiek salvo
   *[other] { $count } kritieke salvo's
}: { $bursts }. Alleen zichtbaar voor schepen bij de opnemende speler
ui-rating-detail-critical_burst = { $share }% van { $ship } in { $seconds } s
ui-rating-detail-critical_burst_at_once = { $share }% van { $ship } in één keer

## ui.personal_rating
ui-personal_rating-unrated_hover = De persoonlijke beoordeling geldt alleen voor willekeurige en gerangschikte gevechten.

## ui.rating.basis
ui-rating-basis-static = Statisch
ui-rating-basis-dynamic = Dynamisch
ui-rating-basis-static_explained = Statisch: gemeten tegen een vaste constante of een klassebasislijn
ui-rating-basis-dynamic_explained = Dynamisch: gemeten tegen verwachte waarden van wows-numbers of de andere spelers in dit gevecht

## ui.replay.group
ui-replay-group-date = Datum
ui-replay-group-ship = Schip
ui-replay-group-none = Geen

## ui.replay.player
ui-replay-player-hidden_profile = Speler heeft een verborgen profiel
ui-replay-player-never_connected = Speler heeft nooit verbinding gemaakt met de wedstrijd

## ui.replay.build
ui-replay-build-open_shiptool = Build openen in ShipTool
ui-replay-build-open_in_ship_builder = Build openen in WoWs SB
ui-replay-build-copy_link = Buildlink kopiëren
ui-replay-build-copy_short_link = Korte buildlink kopiëren
ui-replay-build-link_copied = Buildlink gekopieerd
ui-replay-build-open_wows_numbers = WoWs Numbers-pagina openen
ui-replay-build-not_spotted = Dit schip is nooit gespot. Buildinformatie niet beschikbaar.

## ui.replay.sections
ui-replay-sections-achievements = Prestaties
ui-replay-sections-ribbons = Linten
ui-replay-sections-modules = Modules:
ui-replay-sections-modules_none = Geen modules
ui-replay-sections-signals = Signalen:
ui-replay-sections-loadout = Uitrusting:
ui-replay-sections-loadout_none = Geen uitrusting
ui-replay-sections-captain_skills = Kapiteinvaardigheden:
ui-replay-sections-captain_skills_none = Geen kapiteinvaardigheden
ui-replay-sections-consumables = Verbruiksartikelen:
ui-replay-sections-fire_chance = Effectieve brandkans
ui-replay-sections-fire_chance_expected = verwacht
ui-replay-sections-fire_chance_fires = { $fires ->
    [one] { $fires } brand
   *[other] { $fires } branden
}
ui-replay-sections-fire_chance_eligible_hits = { $hits ->
    [one] { $hits } treffer die er een had kunnen veroorzaken
   *[other] { $hits } treffers die er een hadden kunnen veroorzaken
}
ui-replay-sections-fire_chance_ships = verdeeld over { $ships ->
    [one] { $ships } doelschip
   *[other] { $ships } doelschepen
}
ui-replay-sections-fire_chance_expected_fires = verwacht { $fires } branden
ui-replay-sections-fire_chance_per_ship = Per doelschip
ui-replay-sections-fire_chance_no_eligible_hits = geen treffers die een brand hadden kunnen veroorzaken
ui-replay-sections-fire_chance_formula = Formule brandkans aanvaller
ui-replay-sections-fire_chance_formula_base = basis burnProb
ui-replay-sections-fire_chance_formula_clamped = begrensd
ui-replay-sections-fire_chance_breakdown = Uitsplitsing
ui-replay-sections-fire_chance_shells_fired = { $count ->
    [one] HE-granaat afgevuurd
   *[other] HE-granaten afgevuurd
}
ui-replay-sections-fire_chance_he_hits = { $count ->
    [one] HE-treffer op een schip
   *[other] HE-treffers op een schip
}
ui-replay-sections-fire_chance_no_target_ship = { $count ->
    [one] HE-treffer die niet aan een doelschip is toe te wijzen
   *[other] HE-treffers die niet aan een doelschip zijn toe te wijzen
}
ui-replay-sections-fire_chance_eligible = had een brand kunnen veroorzaken
ui-replay-sections-fire_chance_ribbons = { $count ->
    [one] SetFire-lint
   *[other] SetFire-linten
}
ui-replay-sections-fire_chance_ribbons_credited = toegewezen aan een granaat
ui-replay-sections-fire_chance_ribbons_uncredited = niet toegewezen
ui-replay-sections-fire_chance_unattributed_no_burn_state = geen wijziging in brandstatus om het aan te koppelen
ui-replay-sections-fire_chance_unattributed_already_credited = een eerdere brand gebruikte de nabijgelegen treffer al
ui-replay-sections-fire_chance_unattributed_secondary_contest = een van onze eigen secundaire kanonnen had hem net zo goed kunnen veroorzaken
ui-replay-sections-fire_chance_unattributed_hit_contest = elke nabijgelegen treffer van ons was dubbelzinnig met een andere
ui-replay-sections-fire_chance_unattributed_all_excluded = elke nabijgelegen treffer van ons was al uitgesloten
ui-replay-sections-fire_chance_unattributed_none_could_burn = geen enkele nabijgelegen treffer van ons had een brand kunnen veroorzaken
ui-replay-sections-fire_chance_unattributed_no_hit = geen enkele treffer van ons viel binnen het tijdvenster
ui-replay-sections-fire_chance_not_applicable = slachtoffer al vernietigd, niet van toepassing
ui-replay-sections-fire_chance_copied = Uitsplitsing brandkans gekopieerd
ui-replay-sections-fire_chance_click_to_copy = Klik om deze uitsplitsing te kopiëren
ui-replay-sections-fire_chance_exclusion_already_burning = sectie brandt al
ui-replay-sections-fire_chance_exclusion_victim_build_unknown = build van slachtoffer onbekend, brandzones kunnen samengevoegd zijn
ui-replay-sections-fire_chance_exclusion_damage_control_active = Schadebeheersingsteam actief
ui-replay-sections-fire_chance_exclusion_damage_control_unknown = status van schadebeheersing onbekend
ui-replay-sections-fire_chance_exclusion_observation_gap = hiaat in de waarneming
ui-replay-sections-fire_chance_exclusion_consumable_unreliable = model voor activering van verbruiksartikelen onbetrouwbaar
ui-replay-sections-fire_chance_exclusion_victim_fate_unknown = lot van slachtoffer onbekend
ui-replay-sections-fire_chance_exclusion_hit_type_does_not_roll = dit treffertype rolt niet voor brand
ui-replay-sections-fire_chance_exclusion_no_geometry = geen geometrie van brandsecties
ui-replay-sections-fire_chance_exclusion_impact_unplaceable = kon de inslag niet plaatsen op het schip waaraan we hem hebben gekoppeld
ui-replay-sections-fire_chance_exclusion_victim_pose_unknown = positie van slachtoffer bij inslag onbekend
ui-replay-sections-fire_chance_exclusion_ambiguous_hit = dubbelzinnig met een andere treffer van ons
ui-replay-sections-damage_events = Schadegebeurtenissen

## ui.replay.debug
ui-replay-debug-view_raw_metadata = Ruwe spelersmetadata bekijken
ui-replay-debug-raw_metadata = Ruwe metadata
ui-replay-debug-raw_results = Ruwe resultaten
ui-replay-debug-mapped_results = Toegewezen resultaten
ui-replay-debug-player_metadata = Spelersmetadata
ui-replay-debug-search = Zoeken
ui-replay-debug-previous_match = Vorige overeenkomst (Shift+Enter)
ui-replay-debug-next_match = Volgende overeenkomst (Enter)

## ui.replay.controls
ui-replay-controls-window_title = Replaybediening
ui-replay-controls-unavailable = Bediening niet beschikbaar (commands.scheme.xml niet gevonden in spelbestanden).

# -- Samenwerking / Sessie --

## ui.collab
ui-collab-session = Sessie
ui-collab-starting = Sessie starten...
ui-collab-session_active = Sessie actief
ui-collab-connected_to_session = Verbonden met sessie
ui-collab-session_token = Sessietoken:
ui-collab-toggle_visibility = Tokenzichtbaarheid wisselen
ui-collab-copy_token = Token kopiëren
ui-collab-token_copied = Token gekopieerd naar klembord
ui-collab-copy_web_link = Weblink kopiëren
ui-collab-web_link_copied = Weblink gekopieerd naar klembord
ui-collab-copy_localhost_link = Localhost-link kopiëren
ui-collab-localhost_copied = Localhost-link gekopieerd naar klembord
ui-collab-connected_count = { $count } verbonden
ui-collab-promote_cohost = Promoveer tot co-host
ui-collab-permissions = Rechten
ui-collab-lock_annotations = Annotaties vergrendelen
ui-collab-lock_settings = Instellingen vergrendelen
ui-collab-reset_overrides = Clientoverschrijvingen herstellen
ui-collab-reset_overrides_tooltip = Alle wijzigingen in clientweergave-instellingen herstellen
ui-collab-leave = Verlaten
ui-collab-role_host = Host
ui-collab-role_cohost = Co-host
ui-collab-you = (jij)
ui-collab-display_name = Weergavenaam:
ui-collab-display_name_hint = Uw naam...
ui-collab-enter_display_name = Voer eerst een weergavenaam in
ui-collab-host_session = Een sessie hosten
ui-collab-start_session = Sessie starten
ui-collab-join_session = Deelnemen aan een sessie
ui-collab-paste_and_join = Token plakken en deelnemen
ui-collab-clipboard_empty = Klembord is leeg
ui-collab-invalid_token = Ongeldig token: { $error }
ui-collab-no_runtime = De sessie kon niet starten: geen runtime
ui-collab-shared_windows = Gedeelde vensters
ui-collab-watching = Kijken
ui-collab-waiting_for_frames = Wachten tot het gevecht binnenkomt
ui-collab-open = Openen
ui-collab-open_for_everyone = Openen voor iedereen
ui-collab-tactics_board = Tactiekbord

# -- Gevechtsresultaattekst --

## battle
battle-victory = OVERWINNING
battle-defeat = NEDERLAAG
battle-draw = GELIJKSPEL
battle-battle_starts_in = GEVECHT BEGINT OVER

# -- Beschrijvingen aflooptype --

## finish_type
finish_type-unknown = Gevecht beëindigd
finish_type-extermination = Alle vijandelijke schepen vernietigd
finish_type-base_captured = Basis veroverd
finish_type-timeout = Tijd verstreken
finish_type-failure = Missie mislukt
finish_type-technical = Technische beëindiging
finish_type-score = Scorelimiet bereikt
finish_type-score_on_timeout = Scorevoorsprong bij tijdslimiet
finish_type-pve_success = Missie volbracht
finish_type-pve_failed = Missie mislukt
finish_type-score_zero = Score uitgeput
finish_type-score_excess = Scorelimiet overschreden

# -- Statistiekentab --

## ui.stats
ui-stats-overview = Overzicht
ui-stats-sample_notice = Voorbeeldgegevens, geen opgenomen gevechten
ui-stats-div_all = Alle
ui-stats-div_solo = Solo
ui-stats-div_div = Div
ui-stats-filters = Filters
ui-stats-own_filters = Deze grafiek afzonderlijk filteren
ui-stats-own_filters_tooltip = Deze grafiek verfijnen zonder te wijzigen wat de andere panelen tonen
ui-stats-mode_label = Modus:
ui-stats-clear = Wissen
ui-stats-pr = PR:
ui-stats-mc = MC:
ui-stats-best_frags = Beste: { $ship } ({ $count ->
    [one] { $count } eliminatie
   *[other] { $count } eliminaties
})
ui-stats-max_damage = Max schade: { $ship } ({ $damage })
ui-stats-copy_markdown = Kopiëren als Markdown
ui-stats-copy_image = Kopiëren als afbeelding
ui-stats-copy_image_done = Grafiek gekopieerd naar klembord
ui-stats-copy_image_failed = De grafiek kon niet worden gekopieerd
ui-stats-copy_csv = Kopiëren als CSV
ui-stats-remove_games_hint = Alle { $ship }-spellen verwijderen (Ctrl+Click om bevestiging over te slaan)
ui-stats-settings = Instellingen
ui-stats-options = Opties
ui-stats-ships = Schepen
ui-stats-filter_ships = Schepen in grafiek filteren
ui-stats-all_ships = Alle
ui-stats-no_ships = Geen
ui-stats-search_ships = Schepen zoeken...
ui-stats-no_ship_matches = Geen schepen komen overeen met deze zoekopdracht
ui-stats-search_statistics = Statistieken zoeken
ui-stats-no_statistic_matches = Geen statistieken komen overeen met deze zoekopdracht
ui-stats-ship_selection_count = { $selected } van { $total } geselecteerd
ui-stats-all_ships_tooltip = Alle overeenkomende schepen selecteren
ui-stats-no_ships_tooltip = Selectie van overeenkomende schepen wissen
ui-stats-limit_to_recent = Beperken tot recente spellen
ui-stats-limit_to_recent_tooltip = De limiet geldt per schip in Schepen en Grafieken, en over alle spellen in Overzicht.
ui-stats-division_label = Divisie:
ui-stats-add_chart = Grafiek toevoegen
ui-stats-clear_tooltip = Alle vastgelegde spellen vergeten
ui-stats-clear_failed = Niet gewist: { $reason }
ui-stats-clear_ship_confirm = Druk nogmaals om de spellen van dit schip te vergeten
ui-stats-copy_table = Deze tabel kopiëren
ui-stats-copied = Gekopieerd naar klembord
ui-stats-no_games = Geen spellen vastgelegd voor de huidige filters
ui-stats-nothing_to_plot = Niets te tonen voor de huidige filters
ui-stats-column_ship = Schip
ui-stats-column_battles = Gevechten
ui-stats-column_win_rate = Winstpercentage
ui-stats-column_avg_damage = Gem. schade
ui-stats-column_avg_frags = Gem. eliminaties
ui-stats-frags = { $count ->
    [one] { $count } eliminatie
   *[other] { $count } eliminaties
}
ui-stats-statistic = Statistiek
ui-stats-open_directory_stats = Mapstatistieken
ui-stats-open_directory_stats_tooltip = Statistieken openen voor elke replay in deze map en de submappen ervan
ui-stats-directory_stats_unavailable = Spelgegevens worden nog geladen
ui-stats-directory_reading_task = Replays lezen voor mapstatistieken...
ui-stats-directory_walking = Replays zoeken...
ui-stats-directory_scanning = { $done } van { $total ->
    [one] { $total } replay controleren...
   *[other] { $total } replays controleren...
}
ui-stats-directory_reading = { $done } van { $total ->
    [one] { $total } replay lezen...
   *[other] { $total } replays lezen...
}
ui-stats-directory_reload = Opnieuw laden
ui-stats-directory_reload_tooltip = De replays in deze map opnieuw lezen
ui-stats-directory_skipped = { $count ->
    [one] { $count } replay overgeslagen
   *[other] { $count } replays overgeslagen
}
ui-stats-directory_skipped_detail =
    Onleesbaar: { $unreadable }
    Geen spelgegevens voor hun build: { $missing_build }
    Geen gevechtsresultaten: { $no_results }
ui-stats-chart = Grafiek
ui-stats-combine_ships = Schepen combineren
ui-stats-running_average = Voortschrijdend gemiddelde
ui-stats-value_labels = Waardelabels
ui-stats-reset_view = Weergave herstellen
ui-stats-keep_current_layout = Huidige indeling behouden
ui-stats-session_title = Sessiestatistieken
ui-stats-session_manual = Handmatig
ui-stats-session_since_opened = Sinds de app is geopend
ui-stats-session_manual_tooltip = De sessie bevat de gevechten die je speelt en de replays die je vanuit de replaylijst instelt of toevoegt. Deze blijft bewaard tussen starts.
ui-stats-session_since_opened_tooltip = De sessie bevat elk gevecht dat om of na { $time } begon, toen de app werd geopend. Je handmatige sessie blijft bewaard en komt terug als je naar Handmatig overschakelt.
ui-stats-session_battles = { $count ->
    [one] { $count } gevecht
   *[other] { $count } gevechten
}
ui-stats-session_chip_manual = Sessie: { $battles }
ui-stats-session_chip_since = Sessie sinds { $time }: { $battles }
ui-stats-session_empty_title = Nog geen gevechten in deze sessie
ui-stats-session_empty_manual = Gevechten die je speelt worden aan de sessie toegevoegd zodra ze eindigen. Je kunt ook replays instellen of toevoegen via het menu van de replaylijst.
ui-stats-session_empty_since = Gevechten die na { $time } beginnen worden aan de sessie toegevoegd zodra ze eindigen.
ui-stats-session_set_add_unavailable = De sessie bevat gevechten sinds { $time }. Schakel over naar Handmatig om replays in te stellen of toe te voegen.
ui-stats-clear_unavailable_since = Een sessie sinds het openen van de app wordt niet opgeslagen. Schakel over naar Handmatig om de opgeslagen sessie te wissen.
ui-stats-charts_save_failed = Grafiekinstellingen niet opgeslagen: { $reason }
ui-stats-layout_save_failed = Grafiekindeling niet opgeslagen: { $reason }
ui-stats-axis_game = Spel
ui-stats-chart_hover_game = Spel { $number }, { $time }
ui-stats-series_average = { $stat } (gemiddelde)
ui-stats-series_running = { $stat } (voortschrijdend)
ui-stats-series_more = +{ $count } meer
ui-stats-mode_random = Willekeurig
ui-stats-mode_ranked = Gerangschikt
ui-stats-mode_other = Overig
ui-stats-win_rate_rated_only = Het winstpercentage telt alleen willekeurige en gerangschikte gevechten
ui-stats-win_rate_breakdown_line = { $mode }: { $rate } (gevechten: { $battles })
ui-stats-matches = Spellen
ui-stats-column_time = Tijd
ui-stats-column_mode = Modus
ui-stats-column_result = Resultaat
ui-stats-keep_game = Dit spel meetellen in de statistieken
ui-stats-leave_out_game = Dit spel weglaten uit de statistieken
ui-stats-leave_out_ship = Alle spellen met { $ship } weglaten
ui-stats-leave_out_ship_hint = { $ship } weglaten uit de statistieken
ui-stats-keep_all_games = Alle spellen meetellen
ui-stats-keep_all_games_tooltip = Alle handmatig weggelaten spellen weer meetellen
ui-stats-left_out_only = Alleen weggelaten
ui-stats-match_count = { $shown } van { $total ->
    [one] { $total } spel
   *[other] { $total } spellen
}, { $left_out } weggelaten
ui-stats-no_matches = Geen spellen komen overeen met de huidige filters
ui-stats-search_matches = Zoeken op schip...
ui-stats-ship_filter_tooltip = Kies welke schepen meetellen in de statistieken
ui-stats-keep_ships_tooltip = Alle overeenkomende schepen meetellen
ui-stats-leave_out_ships_tooltip = Alle overeenkomende schepen weglaten uit de statistieken
ui-stats-dates = Datums
ui-stats-all_dates = Alle datums
ui-stats-clear_dates = Alle datums tonen
ui-stats-games_left_out = { $count ->
    [one] { $count } spel weggelaten
   *[other] { $count } spellen weggelaten
}
ui-stats-reset_filters = Filters herstellen
ui-stats-reset_filters_tooltip = Alle spellen tonen: geen limiet, periode, divisie, modus of weggelaten schepen en spellen

## ui.stats.table
ui-stats-table-min = Min
ui-stats-table-max = Max
ui-stats-table-total = Totaal
ui-stats-table-average = Gemiddelde

# -- Spelersvolger --

## ui.player_tracker
ui-player_tracker-clear_stats = Statistieken wissen
ui-player_tracker-populate_from_replays = Gegevens vullen vanuit replays
ui-player_tracker-find_matches = Wedstrijden zoeken
ui-player_tracker-open_wows_numbers = Openen op wows-numbers
ui-player_tracker-open_shipbuilds = Builds openen op shipbuilds
ui-player_tracker-subtab_players = Spelers
ui-player_tracker-filter_hint = Filteren op speler of clan...
ui-player_tracker-time_period = Tijdsperiode
ui-player_tracker-allies = Geallieerden
ui-player_tracker-enemies = Vijanden
ui-player_tracker-live_no_directory = Stel de World of Warships-map in om gevechten te volgen
ui-player_tracker-live_checking = Controleren op een lopend gevecht...
ui-player_tracker-live_none = Geen gevecht bezig
ui-player_tracker-live_loading_build = Scheepsgegevens van deze build laden; namen en klassen worden ingevuld zodra ze binnen zijn.
ui-player_tracker-waiting_for_index = Wachten op de replayindex
ui-player_tracker-loading_clans = Clans laden...
ui-player_tracker-loading_players = Spelers laden...
ui-player_tracker-index_failed = Kan de index niet lezen: { $reason }
ui-player_tracker-no_players = Geen spelers geïndexeerd voor deze periode
ui-player_tracker-note_not_saved = Niet opgeslagen: { $reason }
ui-player_tracker-subtab_current_match = Huidige wedstrijd
ui-player_tracker-encounters_hover = In totaal { $total } keer gezien, { $range } in het geselecteerde tijdsbereik. Laatst gezien { $last }.
ui-player_tracker-never_encountered = Niet eerder gezien
ui-player_tracker-copy_wg_id = Klik om de WG-ID te kopiëren
ui-player_tracker-copied_wg_id = WG-ID { $id } gekopieerd
ui-player_tracker-aliases_hover = Ook bekend als: { $names }
ui-player_tracker-notes_hint = Notities over deze speler
ui-player_tracker-arena_count = { $count ->
    [one] { $count } wedstrijd vastgelegd
   *[other] { $count } wedstrijden vastgelegd
}
ui-player_tracker-last_encountered_exact = Laatst ontmoet { $timestamp }
ui-player_tracker-subtab_clans = Clans
ui-player_tracker-clan_no_data = Nog geen clanontmoetingen vastgelegd.
ui-player_tracker-find_clan_matches = Zoek wedstrijden waarin deze tag voorkomt. De index heeft geen apart clanveld, dus spelersnamen die de tag bevatten komen ook overeen.
ui-player_tracker-clan_sightings_hover = { $range } hiervan vallen in het geselecteerde tijdsbereik.
ui-player_tracker-clan_member_matches = gezien in { $count ->
    [one] { $count } wedstrijd
   *[other] { $count } wedstrijden
}
ui-player_tracker-show_division_mates = Divisiegenoten tonen
ui-player_tracker-show_division_mates_hover = Tel de wedstrijden mee die u in een divisie hebt gespeeld. Standaard uit, omdat u die spelers daar bent tegengekomen doordat u ze zelf hebt uitgenodigd. Andere wedstrijden tegen dezelfde spelers worden altijd meegeteld.
ui-player_tracker-seen_in_twitch_chat = Gezien in Twitch-chat
ui-player_tracker-seen_in_twitch_chat_hover = Toon alleen de spelers van wie de naam tijdens een wedstrijd met u overeenkwam met een login in de Twitch-chat.
ui-player_tracker-twitch_seen_hover = Gezien in Twitch-chat als { $logins } in { $count ->
    [one] { $count } wedstrijd
   *[other] { $count } wedstrijden
} met u
ui-player_tracker-win_rate_overall = Algemeen
ui-player_tracker-win_rate_ship = Schip
ui-player_tracker-view_compact = Compact
ui-player_tracker-view_detailed = Gedetailleerd
ui-player_tracker-ship_stats = Scheepsstatistieken
ui-player_tracker-stats_resolving = De spelerslijst van de wedstrijd lezen...
ui-player_tracker-stats_fetching = Spelersstatistieken ophalen...
ui-player_tracker-stats_unavailable = Statistieken niet beschikbaar: { $reason }
ui-player_tracker-stats_hidden = Het profiel van deze speler is privé
ui-player_tracker-stats_no_data = Geen statistieken voor deze speler
ui-player_tracker-pr_overall_hover = Account-PR over alle schepen
ui-player_tracker-pr_ship_hover = PR in dit schip
ui-player_tracker-win_rate_hover_line = { $label }: { $rate } over { $count ->
    [one] { $battles } gevecht
   *[other] { $battles } gevechten
}
ui-player_tracker-win_rate_hover_line_damage = { $label }: { $rate } over { $count ->
    [one] { $battles } gevecht
   *[other] { $battles } gevechten
}, { $damage } gemiddelde schade
ui-player_tracker-team_average_win_rate = Gem. WR: { $rate }
ui-player_tracker-team_players = { $count ->
    [one] { $count } speler
   *[other] { $count } spelers
}
ui-player_tracker-debug_replay_picker = Debug: een replay als de huidige wedstrijd behandelen
ui-player_tracker-debug_replay_picker_hover = Laadt de spelerslijst van een voltooide replay in dit tabblad, zodat het statistiekenpad buiten een gevecht kan worden getest.
ui-player_tracker-debug_replay_none = Kies een replay

## ui.player_tracker.column
ui-player_tracker-column-win_rate = WR
ui-player_tracker-column-personal_rating = PR
ui-player_tracker-column-battles = Gevechten
ui-player_tracker-column-avg_damage = Schade
ui-player_tracker-column-encounters = Gezien
ui-player_tracker-column-clan = Clan
ui-player_tracker-column-player_name = Spelersnaam
ui-player_tracker-column-total_encounters = Totale ontmoetingen
ui-player_tracker-column-encounters_in_range = Ontmoetingen in tijdsbereik
ui-player_tracker-column-last_encountered = Laatst ontmoet
ui-player_tracker-column-members = Leden
ui-player_tracker-column-matches = Wedstrijden
ui-player_tracker-column-sightings = Waarnemingen

## ui.player_tracker.period
ui-player_tracker-period-past_hour = Afgelopen uur
ui-player_tracker-period-past_six_hours = Afgelopen 6 uur
ui-player_tracker-period-past_day = Afgelopen 24 uur
ui-player_tracker-period-past_week = Afgelopen week
ui-player_tracker-period-past_month = Afgelopen maand
ui-player_tracker-period-all_time = Altijd

# -- Modbeheer --

## ui.twitch
ui-twitch-possible_name = Mogelijke streamnaam: { $name }
ui-twitch-seen_minutes = Gezien: { $minutes } minuten na start wedstrijd
ui-twitch-click_to_copy = Klik om te kopiëren
ui-twitch-copied = { $name } gekopieerd

# -- Replayrenderer --

## ui.renderer
ui-renderer-preparing_export = Export voorbereiden...
ui-renderer-encoding = Coderen
ui-renderer-muxing = Muxing

## ui.renderer.advantage
ui-renderer-advantage-breakdown = Voordeelanalyse
ui-renderer-advantage-team_eliminated = Een team is geëlimineerd
ui-renderer-advantage-score_projection = Scoreprojectie
ui-renderer-advantage-fleet_power = Vlootkracht
ui-renderer-advantage-strategic_threat = Strategische dreiging
ui-renderer-advantage-total = Totaal
ui-renderer-advantage-hp_incomplete = HP/scheepsgegevens onvolledig. Alleen scorefactoren weergegeven

## ui.renderer.context
ui-renderer-context-show_trail = Spoor tonen
ui-renderer-context-disable_other_trails = Alle andere sporen uitschakelen
ui-renderer-context-ranges = Bereiken
ui-renderer-context-detection = Detectie
ui-renderer-context-main_battery = Hoofdbatterij
ui-renderer-context-secondary = Secundair
ui-renderer-context-torpedo = Torpedo
ui-renderer-context-radar = Radar
ui-renderer-context-hydro = Hydro
ui-renderer-context-enable_all = Alles inschakelen
ui-renderer-context-disable_all = Alles uitschakelen
ui-renderer-context-disable_other_ranges = Alle andere bereiken uitschakelen
ui-renderer-context-enable_all_ranges = Bereiken van alle schepen inschakelen
ui-renderer-context-realtime_armor = Realtime pantser tonen

## ui.renderer.controls
ui-renderer-controls-jump_to_start = Naar begin springen
ui-renderer-controls-previous_event = Vorig evenement (Shift+Left)
ui-renderer-controls-back_10s = 10s terug (Left)
ui-renderer-controls-forward_10s = 10s vooruit (Right)
ui-renderer-controls-next_event = Volgend evenement (Shift+Right)
ui-renderer-controls-jump_to_end = Naar einde springen
ui-renderer-controls-speed = Afspeelsnelheid

## ui.renderer.settings
ui-renderer-settings-title = Weergave-instellingen
ui-renderer-settings-hp_bars = HP-balken
ui-renderer-settings-armament = Bewapening
ui-renderer-settings-dead_ships = Vernietigde schepen
ui-renderer-settings-dead_ship_names = Namen vernietigde schepen
ui-renderer-settings-player_names = Spelersnamen
ui-renderer-settings-ship_ranges = Scheepsbereiken
ui-renderer-settings-ship_names = Scheepsnamen
ui-renderer-settings-camera_direction = Geschutskoepelrichting
ui-renderer-settings-heat_trail = Warmtespoor
ui-renderer-settings-speed_trails = Snelheidssporen
ui-renderer-settings-dead_ship_trails = Sporen vernietigde schepen
ui-renderer-settings-buildings = Gebouwen
ui-renderer-settings-capture_points = Innamepunten
ui-renderer-settings-consumables = Verbruiksartikelen
ui-renderer-settings-planes = Vliegtuigen
ui-renderer-settings-smoke = Rook
ui-renderer-settings-torpedoes = Torpedo's
ui-renderer-settings-tracers = Spoorlichtkogels
ui-renderer-settings-battle_result = Gevechtsresultaat
ui-renderer-settings-buff_counters = Bufftellers
ui-renderer-settings-chat_label = Chat
ui-renderer-settings-kill_feed = Eliminatiefeed
ui-renderer-settings-score_label = Score
ui-renderer-settings-score_timers = Scoretimers
ui-renderer-settings-team_advantage = Teamvoordeel
ui-renderer-settings-team_rosters = Teamopstellingen
ui-renderer-settings-stats_panel = Statistiekenpaneel
ui-renderer-settings-timer = Timer
ui-renderer-settings-export_settings = Exportinstellingen
ui-renderer-settings-prefer_cpu = CPU-encoder voorkeur
ui-renderer-settings-prefer_cpu_tooltip = Gebruik software (CPU)-encoder in plaats van GPU voor video-export
ui-renderer-settings-include_pre_battle = Fase voor het gevecht meenemen
ui-renderer-settings-include_pre_battle_tooltip = Neem het spawnen en het aftellen voor de start van het gevecht op in de geëxporteerde video
ui-renderer-settings-export_timing = Videotiming
ui-renderer-settings-export_timing_length = Aan lengte aanpassen
ui-renderer-settings-export_timing_speed = Snelheid
ui-renderer-settings-export_timing_result_speed = Speelt af op ongeveer { $speed }x
ui-renderer-settings-export_timing_result_length = Video duurt { $length }
ui-renderer-settings-codec = Codec
ui-renderer-settings-codec_auto = Automatisch
ui-renderer-settings-save_defaults = Standaarden opslaan
ui-renderer-settings-save_defaults_done = Opgeslagen als beginstand van een nieuwe renderer
ui-renderer-settings-save_defaults_failed = De standaarden konden niet worden opgeslagen
ui-renderer-settings-event_timeline = Evenementtijdlijn

# -- Bestandsuitpakker --

## ui.unpacker
ui-unpacker-assets_bin = Assets.bin
ui-unpacker-loading_assets = Assets.bin laden...
ui-unpacker-load_failed = Kan niet laden: { $error }
ui-unpacker-no_game_data = Geen spelgegevens geladen. Stel uw World of Warships-map in bij Instellingen.
ui-unpacker-filter_files = Bestanden filteren...
ui-unpacker-search_in_files = Zoeken in bestanden
ui-unpacker-search_hint = Zoeken in bestanden...
ui-unpacker-path_filter_hint = Padfilter (bijv. *.xml)
ui-unpacker-queue_all_files = Alle bestanden in wachtrij
ui-unpacker-view_as_json = Bekijken als JSON
ui-unpacker-extract_as_json = Extraheren als JSON
ui-unpacker-view_contents = Inhoud bekijken
ui-unpacker-version = Versie:
ui-unpacker-decode_prototypes = Prototypen decoderen als JSON
ui-unpacker-decode_prototypes_tooltip = Ondersteunde Assets.bin-prototypen (.visual, .model, .mfm) decoderen naar JSON bij extractie
ui-unpacker-extraction_queue = Extractiewachtrij
ui-unpacker-collapse_folders = Mappenstructuur inklappen
ui-unpacker-expand_folders = Mappenstructuur uitklappen
ui-unpacker-clear_all = Alles wissen
ui-unpacker-browse = Bladeren
ui-unpacker-output_dir_hint = Uitvoermap...
ui-unpacker-remove_from_queue = Verwijderen uit wachtrij
ui-unpacker-queue_folder = Hele map in wachtrij
ui-unpacker-queue_file = Dit bestand in wachtrij zetten
ui-unpacker-extraction_output = Uitpakken naar
ui-unpacker-queue_empty = Vink een bestand of map in de lijst aan om het in de wachtrij te zetten
ui-unpacker-packages = Pakketten
ui-unpacker-item_count = { $count ->
    [one] { $count } item
   *[other] { $count } items
}
ui-unpacker-search_title = Zoeken: { $query }
ui-unpacker-search_in = in { $source }
ui-unpacker-image_decode_failed = Kan deze afbeelding niet decoderen: { $reason }
ui-unpacker-find = Zoeken
ui-unpacker-find_previous = Vorige overeenkomst
ui-unpacker-find_next = Volgende overeenkomst
ui-unpacker-find_close = Zoeken sluiten
ui-unpacker-find_count = { $current } van { $total }
ui-unpacker-find_none = Geen overeenkomsten
ui-unpacker-find_match_case = Hoofdlettergevoelig
ui-unpacker-queue = Wachtrij
ui-unpacker-reveal_in_tree = Tonen in pakketstructuur
ui-unpacker-hit_offset = op { $offset }
ui-unpacker-queued_count = { $count } in wachtrij
ui-unpacker-counting_files = Bestanden tellen...
ui-unpacker-extracting = Uitpakken { $done } van { $total }
ui-unpacker-extract_failed = Uitpakken mislukt: { $reason }
ui-unpacker-extracted = { $count ->
    [one] { $count } bestand uitgepakt
   *[other] { $count } bestanden uitgepakt
}
ui-unpacker-cancelled = Geannuleerd na { $count ->
    [one] { $count } bestand
   *[other] { $count } bestanden
}
ui-unpacker-extract = { $count ->
    [0] Uitpakken
    [one] { $count } item uitpakken
   *[other] { $count } items uitpakken
}
ui-unpacker-choose_directory = Kies eerst een map om naar uit te pakken
ui-unpacker-dump_parameters = GameParams exporteren
ui-unpacker-dump_parameters_tooltip = GameParams.data van deze build exporteren
ui-unpacker-load_a_build = Laad eerst een build
ui-unpacker-parameters_written = Parameters weggeschreven
ui-unpacker-save_parameters_title = Spelparameters opslaan
ui-unpacker-writing = Bezig met schrijven...
ui-unpacker-prototype_written = Prototype weggeschreven
ui-unpacker-whole_tree = Volledige structuur
ui-unpacker-base_only = Alleen basisparameters

## ui.unpacker.column
ui-unpacker-column-name = Naam
ui-unpacker-column-size = Grootte
ui-unpacker-column-type_col = Type

# -- Pantserweergave --

## ui.armor
ui-armor-load_settings = Laad spelgegevens in Instellingen om de Pantserweergave te gebruiken.
ui-armor-load_failed = Kan scheepsgegevens niet laden: { $error }
ui-armor-empty = Leeg
ui-armor-mirror_cameras = Camera's spiegelen
ui-armor-sync_options = Opties synchroniseren
ui-armor-show_armor_by_default = Pantser standaard tonen
ui-armor-sync_tooltip = Houd pantser-/rompzichtbaarheid en weergave-overlays (scheepscentrum, waterlijn, cameraringen) gesynchroniseerd over alle panelen
ui-armor-common_settings = Algemene instellingen
ui-armor-tier_filter = Tiers { $lowest } tot { $highest }
ui-armor-fortifications = Versterkingen
ui-armor-show_hidden_ships = Verborgen schepen tonen
ui-armor-show_hidden_ships_tooltip = Ook de schepen tonen die de lijst weglaat, zoals clanhuurschepen
ui-armor-compare_split = Vergelijken in nieuw paneel
ui-armor-open_in_new_tab = Openen in nieuw tabblad
ui-armor-export_model = Scheepsmodel exporteren
ui-armor-export_disclaimer = Deze 3D-modellen en texturen zijn intellectueel eigendom van Wargaming en elk gebruik van deze modellen moet in overeenstemming zijn met uw lokale wetgeving.
ui-armor-export_button = Exporteren
ui-armor-hull_toggle = Romp
ui-armor-splash_tooltip = Splashbox weergeven/verbergen
ui-armor-show_hidden = Verborgen tonen
ui-armor-show_hidden_tooltip = (Mogelijk) verborgen panelen weergeven/verbergen
ui-armor-gaps = Openingen
ui-armor-gaps_tooltip = Markeer grenzen waar pantserpanelen niet aansluiten (mogelijke openingen)
ui-armor-display = Weergave
ui-armor-marker_opacity = Markeringsdekking
ui-armor-export_tooltip = Scheepsmodel exporteren naar OBJ-bestand
ui-armor-pen_check = Penetratiecontrole
ui-armor-trajectory = Baan
ui-armor-trajectory_range = Afstand
ui-armor-trajectory_clear = Alles wissen
ui-armor-arc_heading = Baan { $number }
ui-armor-arc_summary = { $hits ->
    [one] { $hits } treffer
   *[other] { $hits } treffers
}, { $armor } mm op { $range } km
ui-armor-arc_delete = Deze baan verwijderen
ui-armor-arc_isolate_plates = Platen isoleren
ui-armor-arc_isolate_plates_tooltip = Alleen de pantserplaten tonen die deze baan heeft doorkruist
ui-armor-arc_isolate_zones = Zones isoleren
ui-armor-arc_isolate_zones_tooltip = Alle delen tonen van de pantserzones die deze baan is binnengegaan
ui-armor-arc_simulation_caveat = Deze simulatie is gebaseerd op gereconstrueerde gegevens en komt mogelijk niet overeen met hoe het spel ballistiek berekent.
ui-armor-arc_no_shell = Geen granaat afgevuurd; de doorkruiste platen worden zonder granaat getoond
ui-armor-arc_detonated_in = Ontploft in { $zone }
ui-armor-arc_detonated_past_armor = Ontploft voorbij al het pantser
ui-armor-arc_ricochet = Ricochet op { $plate }
ui-armor-arc_shatter = Versplinterd op { $plate }
ui-armor-arc_stopped = Gestopt op { $plate }
ui-armor-arc_overpen = Overpenetratie
ui-armor-arc_overpen_unarmed = Overpenetratie (ontsteker nooit geactiveerd)
ui-armor-arc_plate_unknown = een niet-vastgelegde plaat
ui-armor-angle_good = onder 30 deg
ui-armor-angle_mid = 30 tot 45 deg
ui-armor-angle_bad = boven 45 deg
ui-armor-trajectory_ship = Schip:
ui-armor-trajectory_select_ship = Schip selecteren
ui-armor-trajectory_ship_tooltip = Kies het schip dat de granaat voor de baan levert
ui-armor-trajectory_add_ship = Vurend schip toevoegen
ui-armor-trajectory_add_ship_tooltip = Penetratiecontrole openen om een schip te zoeken en toe te voegen
ui-armor-trajectory_shell = Granaat:
ui-armor-trajectory_select_shell = Granaat selecteren
ui-armor-trajectory_shell_tooltip = Kies de granaat die voor banen wordt gebruikt
ui-armor-continue_ricochet = Doorgaan na ricochet
ui-armor-splash_mode = Splash
ui-armor-splash_no_data = Geen splashgegevens voor dit schip
ui-armor-splash_no_shells = Voeg een schip met HE/SAP-granaten toe aan de vergelijkingslijst
ui-armor-splash_tooltip_mode = Klik op pantser om HE-splashschadevolume te visualiseren
ui-armor-select_ship = Selecteer een schip uit de lijst
ui-armor-all_btn = Alle
ui-armor-none_btn = Geen
ui-armor-opaque = Ondoorzichtig
ui-armor-upgrade = Upgrade:
ui-armor-camo_none = Standaard (geen)
ui-armor-camo_group_universal = Universele camo's
ui-armor-camo_group_expendable = Verbruikbare camo's
ui-armor-camo_group_other = Overige camo's
ui-armor-show_armor_thickness = Legenda pantserdikte tonen
ui-armor-plate_edges = Plaatranden
ui-armor-waterline = Waterlijn
ui-armor-camera_rings = Cameraringen
ui-armor-realtime_readout = { $health } HP  -  { $hits ->
    [one] { $hits } treffer
   *[other] { $hits } treffers
}
ui-armor-show_camera_rings = Cameraringen tonen
ui-armor-camera_fov = Camera-FOV
ui-armor-camera_height = Camerahoogte
ui-armor-show_camera_zoom_path = Zoompad tonen
ui-armor-zoom_path_regular_fov = Normale FOV
ui-armor-zoom_path_max_fov = Max. FOV
ui-armor-camera_perspective = Cameraperspectief
ui-armor-perspective_fov = Weergave-FOV
ui-armor-perspective_projection = Projectie
ui-armor-perspective_proj_game = Spel
ui-armor-perspective_proj_center = Door het centrum
ui-armor-ship_center = Scheepscentrum
ui-armor-grid = Raster
ui-armor-length_marks = Lengtemarkeringen
ui-armor-length_readout = Lengte { $length }, breedte { $beam }
ui-armor-length_metres = { $value } m
ui-armor-length_centimetres = { $value } cm
ui-armor-filter_options = Opties filteren...
ui-armor-filter_zones = Zones filteren...
ui-armor-filter_parts = Onderdelen filteren...
ui-armor-tree_matches = Overeenkomsten: { $count }
ui-armor-tree_no_matches = Geen overeenkomsten
ui-armor-no_matching_options = Geen opties komen overeen met het filter
ui-armor-opacity = Dekking
ui-armor-opaque_hull = Ondoorzichtige romp
ui-armor-lighting = Rompverlichting
ui-armor-lighting_enabled = Verlichting inschakelen
ui-armor-lighting_preset_ingame = In-game
ui-armor-lighting_preset_flat = Plat
ui-armor-lighting_preset_studio = Studio
ui-armor-lighting_flat = Plat (ambient)
ui-armor-lighting_intensity = Lichtintensiteit
ui-armor-lighting_azimuth = Lichtazimut
ui-armor-lighting_elevation = Lichthoogte
ui-armor-lighting_rim = Randlicht
ui-armor-lighting_specular = Speculair
ui-armor-lighting_shininess = Glans
ui-armor-lighting_key_color = Hoofdlichtkleur
ui-armor-lighting_ambient_color = Ambientkleur
ui-armor-zero_mm_plates = 0 mm-platen
ui-armor-armor_opacity = Pantserdekking
ui-armor-ballistic_disclaimer = Ballistische resultaten zijn gebaseerd op reverse engineering/schattingen van hoe de simulatie werkt
ui-armor-roll = Rollen
ui-armor-reset_plates = Platen herstellen
ui-armor-armor_options = Pantser
ui-armor-no_ship_loaded = Geen schip geladen
ui-armor-lod = LOD:
ui-armor-lod_highest = 0 (hoogste)
ui-armor-camouflage = Camouflage
ui-armor-ships = Schepen
ui-armor-compare = Vergelijken
ui-armor-compare_tooltip = Een nieuw vergelijkingspaneel openen, of klik met rechts op een schip om dat te vergelijken
ui-armor-mirror_cameras_tooltip = De camera's van alle panelen samen bewegen
ui-armor-show_armor_by_default_tooltip = Modellen openen met het pantser in beeld in plaats van de romp
ui-armor-xray_highlight = Röntgenmarkering bij aanwijzen in de weergave
ui-armor-xray_always = Altijd
ui-armor-xray_while_held = Terwijl { $key } ingedrukt is
ui-armor-xray_off = Uit
ui-armor-stack_panes = Panelen stapelen
ui-armor-stack_panes_tooltip = De vergelijkingspanelen boven elkaar plaatsen in plaats van naast elkaar
ui-armor-viewports = Weergavevensters
ui-armor-options_panel = Optiepaneel
ui-armor-options_pane = Paneel { $number } van { $count }
ui-armor-splash_boxes = Splashboxen
ui-armor-cast_here_hint = Houd { $key } ingedrukt en klik om hier een granaat af te vuren
ui-armor-trajectory_next_cast = Volgend schot
ui-armor-arc_pick_hint = Klik op een markering om de bijbehorende baan te bewerken.
ui-armor-arc_marker_tooltip = Baan { $number }. Klik om te bewerken. Klik met rechts om te verwijderen.
ui-armor-zone_toggle_tooltip = Toont of verbergt alle platen in deze zone. Houd { $key } ingedrukt om alleen deze zone te tonen.
ui-armor-mode_solo_zone = alleen zone
ui-armor-mode_cast_trajectory = baan afvuren
ui-armor-loading_catalog = Scheepscatalogus laden...
ui-armor-legend_collapse = Inklappen
ui-armor-legend_hide = Legenda verbergen
ui-armor-armor_thickness_title = Pantserdikte
ui-armor-initializing = 3D-weergave initialiseren...
ui-armor-init_failed = Pantserweergave kon niet worden geïnitialiseerd: { $reason }
ui-armor-loading_named_ship = { $ship } laden...
ui-armor-options = Opties

## ui.armor.compass
ui-armor-compass-north = N
ui-armor-compass-east = O
ui-armor-compass-south = Z
ui-armor-compass-west = W

## ui.armor.realtime
ui-armor-realtime-incoming_fire = Inkomend vuur
ui-armor-realtime-attacker_filter = Aanvallerfilter
ui-armor-realtime-all_enemies = Alle vijanden
ui-armor-realtime-show_secondaries = Secundaire bewapening tonen
ui-armor-realtime-seek_to_salvo = Naar salvo zoeken
ui-armor-realtime-no_data = Granatgegevens niet meer beschikbaar
ui-armor-realtime-sim_agrees = Simulatie bevestigt
ui-armor-realtime-sim_unchecked = { $count ->
    [one] { $count } granaat kon niet worden gesimuleerd
   *[other] { $count } granaten konden niet worden gesimuleerd
}
ui-armor-realtime-sim_rng_zone = Willekeurige zone ({ $angle } deg in { $from }-{ $to })
ui-armor-realtime-sim_disagrees = Simulatie: { $sim } / Server: { $server }
ui-armor-realtime-no_armor_hit = Geen pantserpanelen geraakt
ui-armor-realtime-tracked = { $salvos ->
    [one] { $salvos } salvo
   *[other] { $salvos } salvo's
}, { $shells ->
    [one] { $shells } granaat gevolgd
   *[other] { $shells } granaten gevolgd
}
ui-armor-realtime-nothing_landed = Er is nog niets op dit schip ingeslagen
ui-armor-realtime-shells = { $count }x
ui-armor-realtime-salvo_shell = { $caliber } mm { $shell }
ui-armor-realtime-salvo_range = { $km } km
ui-armor-realtime-unknown_attacker = Schip { $id }
ui-armor-realtime-server_outcome = Server: { $outcome }
ui-armor-realtime-not_cast = Deze granaat kan niet door de romp worden gevolgd: de vlucht of de positie van het schip is niet vastgelegd
ui-armor-realtime-pick_hint = Klik op een salvo of granaat om die door de romp te volgen

## ui.armor.plates
ui-armor-plates-heading = Geraakte platen ({ $count })
ui-armor-plates-arrival = { $velocity } m/s bij inslag, { $time } s vlucht, valhoek { $fall } deg
ui-armor-plates-overmatch = Overmatch: { $penetration } mm penetratie bij { $velocity } m/s
ui-armor-plates-penetrated = Penetreert: { $penetration } mm tegen { $effective } mm effectief bij { $velocity } m/s
ui-armor-plates-ricochet = Ricochet bij { $angle } deg
ui-armor-plates-shatter = Spat uiteen: { $penetration } mm tegen { $effective } mm effectief
ui-armor-plates-fuse_armed = Ontsteker op scherp
ui-armor-plates-detonation = Ontploft in { $zone }, { $distance } m na plaat #{ $number }
ui-armor-plates-detonation_past_armor = Ontploft voorbij alle bepantsering, { $distance } m na plaat #{ $number }
ui-armor-plates-flat_penetrates = { $ammo } penetreert: { $penetration } mm tegen { $thickness } mm
ui-armor-plates-flat_shatters = { $ammo } spat uiteen: { $penetration } mm tegen { $thickness } mm
ui-armor-plates-server_angle = server { $angle } deg

# -- Labels voordeelniveau --

## advantage
advantage-absolute = Absoluut
advantage-strong = Sterk
advantage-moderate = Matig
advantage-weak = Zwak

## ui.search
ui-search-edit_value = Waarde bewerken
ui-search-add_filter = Filter toevoegen
ui-search-pill_select = Selecteren
ui-search-pill_deselect = Deselecteren
ui-search-pill_group_all = Groeperen met en
ui-search-pill_group_any = Groeperen met of
ui-search-pill_ungroup = Groep opheffen
ui-search-pill_negate = Omkeren
ui-search-pill_flip_connector = En/of wisselen
ui-search-pill_delete = Verwijderen
ui-search-match_count_truncated = { $count }+ wedstrijden
ui-search-outcome_win = Winst
ui-search-outcome_loss = Verlies
ui-search-outcome_draw = Gelijkspel
ui-search-outcome_unknown = Onbekend
ui-search-bool_true = waar
ui-search-bool_false = onwaar
ui-search-match_count = { $count ->
    [one] { $count } wedstrijd
   *[other] { $count } wedstrijden
}
ui-search-row_actions_hint = Replayacties (of klik met rechts op de rij)
ui-search-looking_up_values = Waarden opzoeken...
ui-search-type_a_query = Typ een zoekopdracht en druk op Enter
ui-search-parse_failed = Die zoekopdracht kon niet worden verwerkt: { $reason }
ui-search-searching = Zoeken...
ui-search-failed = Het zoeken is mislukt: { $reason }
ui-search-no_matches = Geen overeenkomsten
ui-search-game_mode_gap_hint = { $count ->
    [one] Bij { $count } geïndexeerde wedstrijd is geen spelmodus vastgelegd. Gebruik Alle replays opnieuw indexeren in Instellingen om dit aan te vullen.
   *[other] Bij { $count } geïndexeerde wedstrijden is geen spelmodus vastgelegd. Gebruik Alle replays opnieuw indexeren in Instellingen om dit aan te vullen.
}
ui-search-open = Openen
ui-search-open_missing = Replaybestand bestaat niet meer
ui-search-free_text = bevat "{ $text }"
ui-search-quant_any = een of meer
ui-search-quant_none = geen
ui-search-quant_ge = { $n } of meer
ui-search-quant_eq = precies { $n }
ui-search-quant_le = hoogstens { $n }
ui-search-quant_gt = meer dan { $n }
ui-search-quant_lt = minder dan { $n }
ui-search-quant_ne = niet { $n }
ui-search-scope_self = Zelf
ui-search-scope_ally = Bondgenoot
ui-search-scope_enemy = Vijand
ui-search-scope_div = Div

## ui.search.suggest
ui-search-suggest-preset_divmate_test_ship = Iemand in mijn divisie speelde een testschip
ui-search-suggest-preset_no_enemy_cv = Geen vijandelijk vliegdekschip
ui-search-suggest-preset_all_enemies_died = Alle vijanden zijn vernietigd
ui-search-suggest-preset_high_damage_enemies = 3 of meer vijanden boven 100k
ui-search-suggest-preset_stream_sniper = Bevat een stream sniper
ui-search-suggest-preset_i_survived = Ik heb overleefd
ui-search-suggest-preset_i_disconnected = Mijn verbinding werd verbroken
ui-search-suggest-my_damage = Mijn schade
ui-search-suggest-enemy_ship = Vijandelijk schip
ui-search-suggest-allied_ship = Geallieerd schip
ui-search-suggest-player_in_match = Speler zat in de wedstrijd
ui-search-suggest-someone_in_division = Iemand in mijn divisie
ui-search-suggest-scope_self = Mijn
ui-search-suggest-scope_ally = Bondgenoot
ui-search-suggest-scope_enemy = Vijand
ui-search-suggest-scope_division = Divisie
ui-search-suggest-scope_anyone = Elke speler

## ui.search.field
ui-search-field-outcome = Resultaat
ui-search-field-map = Kaart
ui-search-field-class = Klasse
ui-search-field-tier = Tier
ui-search-field-date = Datum
ui-search-field-player_present = Speler
ui-search-field-group = Replaygroep
ui-search-field-stat_damage = Schade
ui-search-field-stat_kills = Eliminaties
ui-search-field-stat_spotting = Verkenning
ui-search-field-stat_potential = Potentiële schade
ui-search-field-stat_received = Ontvangen schade
ui-search-field-stat_pr = PR
ui-search-field-rating = Beoordeling
ui-search-field-stat_survived = Overleefd
ui-search-field-stat_disconnected = Verbinding verbroken
ui-search-field-game_type = Speltype
ui-search-field-game_mode = Spelmodus
ui-search-field-match_group = Wedstrijdgroep
ui-search-field-build = Build
ui-search-field-results_available = Resultaten beschikbaar
ui-search-field-relation = Relatie
ui-search-field-division = Divisie
ui-search-field-name = Naam
ui-search-field-clan = Clan
ui-search-field-realm = Server
ui-search-field-ship = Schip
ui-search-field-ship_index = Scheepsindex
ui-search-field-nation = Natie
ui-search-field-test_ship = Testschip
ui-search-field-stream_sniper = Streamsniper
ui-search-field-sniper_login = Twitch-login van sniper

## ui.search.op
ui-search-op-contains = bevat
ui-search-op-equals = is gelijk aan
ui-search-op-not_equals = is niet gelijk aan
ui-search-op-eq = =
ui-search-op-ne = !=
ui-search-op-gt = >
ui-search-op-ge = >=
ui-search-op-lt = <
ui-search-op-le = <=
ui-search-op-is = is
ui-search-op-is_not = is niet
ui-search-op-is_set = is ingesteld
ui-search-op-is_not_set = is niet ingesteld

## ui.search.value
ui-search-value-relation_self = Zelf
ui-search-value-relation_ally = Bondgenoot
ui-search-value-relation_enemy = Vijand
ui-search-value-division_mine = De mijne
ui-search-value-division_any = Elke
ui-search-value-division_none = Geen
ui-search-value-class_air_carrier = Vliegdekschip
ui-search-value-class_battleship = Slagschip
ui-search-value-class_cruiser = Kruiser
ui-search-value-class_destroyer = Torpedobootjager
ui-search-value-class_submarine = Onderzeeër
ui-search-value-class_auxiliary = Hulpschip
ui-search-value-rating_bad = Slecht
ui-search-value-rating_below_average = Onder gemiddeld
ui-search-value-rating_average = Gemiddeld
ui-search-value-rating_good = Goed
ui-search-value-rating_very_good = Zeer goed
ui-search-value-rating_great = Uitstekend
ui-search-value-rating_unicum = Unicum
ui-search-value-rating_super_unicum = Super Unicum
ui-search-value-game_mode_test = Test
ui-search-value-game_mode_standard = Standaard
ui-search-value-game_mode_singlebase = Singlebase
ui-search-value-game_mode_domination = Dominantie
ui-search-value-game_mode_tutorial = Tutorial
ui-search-value-game_mode_megabase = Megabase
ui-search-value-game_mode_forts = Forten
ui-search-value-game_mode_standard_domination = Standaard dominantie
ui-search-value-game_mode_epicenter = Epicentrum
ui-search-value-game_mode_assault_defense = Aanval en verdediging
ui-search-value-game_mode_pve = PVE
ui-search-value-game_mode_arms_race = Wapenwedloop
ui-search-value-game_mode_epicenter_ring = Epicentrumring
ui-search-value-game_mode_anti_standard = Anti-standaard
ui-search-value-game_mode_attack_defense = Aanval en verdediging
ui-search-value-game_mode_torpedo_beat = Torpedo Beat
ui-search-value-game_mode_team_battle_royale = Team-Battle Royale
ui-search-value-game_mode_escape_to_portal = Ontsnap naar het portaal
ui-search-value-game_mode_domination_asymm = Asymmetrische dominantie
ui-search-value-game_mode_key_battle = Sleutelgevecht
ui-search-value-game_mode_portal_2021 = Portaal 2021
ui-search-value-game_mode_team_battle_royale_2021 = Team-Battle Royale 2021
ui-search-value-game_mode_convoy_event = Konvooi-evenement
ui-search-value-game_mode_convoy_airship = Konvooi-luchtschip
ui-search-value-game_mode_two_teams_battle_royale = Battle Royale met twee teams
ui-search-value-game_mode_pinata_event = Piñata-evenement
ui-search-value-game_mode_respawns = Respawns
ui-search-value-game_mode_respawns_sectors = Respawns met sectoren

## ui.search.column
ui-search-column-date = Datum
ui-search-column-map = Kaart
ui-search-column-mode = Modus
ui-search-column-ship = Uw schip
ui-search-column-result = Resultaat
ui-search-column-damage = Schade
ui-search-column-kills = Eliminaties
ui-search-column-pr = PR
ui-search-column-mc = MC

## ui.settings.index
ui-settings-index-heading = Replayindex
ui-settings-index-description = Het doorzoekbare overzicht van elke replay in uw map
ui-settings-index-build = Index opbouwen
ui-settings-index-rebuild = Alles opnieuw indexeren
ui-settings-index-rebuild_hint = Elke replay opnieuw lezen, voor rijen die een oudere verwerking verkeerd had. Index opbouwen leest alleen wat ontbreekt.
ui-settings-index-progress = { $done } van { $total } gelezen, { $failed } niet leesbaar
ui-settings-index-progress_skipped = { $done } van { $total } gelezen, { $skipped } al aanwezig, { $failed } niet leesbaar
ui-settings-index-built = { $indexed ->
    [one] { $indexed } replay geïndexeerd
   *[other] { $indexed } replays geïndexeerd
}, { $failed ->
    [one] { $failed } kon niet worden gelezen
   *[other] { $failed } konden niet worden gelezen
}
ui-settings-index-built_with_skipped = { $indexed ->
    [one] { $indexed } replay geïndexeerd
   *[other] { $indexed } replays geïndexeerd
}, { $skipped ->
    [one] { $skipped } was al aanwezig
   *[other] { $skipped } waren al aanwezig
}, { $failed ->
    [one] { $failed } kon niet worden gelezen
   *[other] { $failed } konden niet worden gelezen
}

## ui.settings
ui-settings-loading = Instellingen laden...
ui-settings-load_failed = Kan instellingen niet laden
ui-settings-not_set = (niet ingesteld)

## ui.settings.panel
ui-settings-panel-mcp = MCP-server
ui-settings-panel-open = Instellingen openen
ui-settings-panel-search_placeholder = Instellingen zoeken
ui-settings-panel-count = { $count ->
    [one] { $count } instelling
   *[other] { $count } instellingen
}
ui-settings-panel-no_matches = Geen instellingen komen overeen met de zoekopdracht
ui-settings-panel-close = Instellingen sluiten
ui-settings-panel-in_error = { $name }: een instelling vereist aandacht
ui-settings-panel-general = Algemeen
ui-settings-panel-appearance = Weergave
ui-settings-panel-game = World of Warships
ui-settings-panel-replays = Replays
ui-settings-panel-collaboration = Samenwerking
ui-settings-panel-twitch = Twitch
ui-settings-panel-language_description = Taal van de interface
ui-settings-panel-data_sharing_description = Wat na gerangschikte en willekeurige gevechten naar ShipBuilds.com wordt verzonden
ui-settings-panel-code_integrity_description = Of code die Microsoft niet heeft ondertekend in deze toepassing mag worden geladen
ui-settings-panel-check_for_updates_description = Bij elke start van de toepassing naar een nieuwere versie zoeken
ui-settings-panel-data_folder = App-gegevensmap
ui-settings-panel-theme_description = Kleuren van de interface. Wijs een keuze aan voor een voorbeeld.
ui-settings-panel-zoom_description = Grootte van alle tekst en bedieningselementen
ui-settings-panel-zoom = Zoom
ui-settings-panel-zoom_shortcuts = { $zoom_in } zoomt in, { $zoom_out } zoomt uit, { $reset } zet terug op 1x
ui-settings-panel-zoom_changed = Zoom: { $factor }x
ui-settings-panel-cache_directory = Cachemap
ui-settings-panel-cache_directory_description = Waar de gecachte builds worden opgeslagen
ui-settings-panel-cached_builds = Gecachte builds
ui-settings-panel-current_path_description = Het meest recent geopende replaybestand
ui-settings-panel-show_raw_xp_description = Een kolom met ruwe XP aan de resultatentabel toevoegen
ui-settings-panel-show_observed_damage_description = Een kolom met waargenomen schade aan de resultatentabel toevoegen
ui-settings-panel-show_entity_id_description = Een kolom met entiteit-ID aan de resultatentabel toevoegen
ui-settings-panel-show_heals_description = Een kolom met genezingen aan de resultatentabel toevoegen
ui-settings-panel-auto_export_description = De gegevens van elk gevecht naar een bestand in het gekozen formaat en de gekozen map schrijven
ui-settings-panel-display_name_description = De naam die anderen in een sessie zien
ui-settings-panel-get_token_description = Een aanmeldpagina openen die een chattoken uitgeeft
ui-settings-panel-token = Aanmeldtoken
ui-settings-panel-monitored_channel_description = Te volgen kanaal. Laat leeg om je eigen kanaal te volgen.
ui-settings-panel-armor_defaults = Opgeslagen standaarden
ui-settings-panel-armor_saved_description = Opgeslagen vanuit de opties van de Modelweergave

## ui.settings.licenses
ui-settings-licenses-title = Over en licenties
ui-settings-licenses-back = Terug naar instellingen
ui-settings-licenses-third_party_intro = Deze toepassing bevat de onderstaande componenten van derden. Selecteer een item om de pakketten die het gebruiken en de volledige licentietekst te tonen.

## ui.settings.armor
ui-settings-armor-no_defaults = (geen opgeslagen standaarden)

## ui.app
ui-app-status_cache_checking = Spelgegevenscache controleren
ui-app-status_cache_validating = Spelgegevenscache verifiëren
ui-app-status_cache_downloading = Spelgegevens downloaden
ui-app-status_cache_planning = Opvragen welke spelgegevens zijn gepubliceerd
ui-app-drop_to_load = Loslaten om { $file } te laden
ui-app-drop_to_list = Loslaten om de replays in { $folder } te tonen
ui-app-drop_to_list_replays = Neerzetten om { $count ->
    [one] { $count } replay
   *[other] { $count } replays
} weer te geven
ui-app-drop_to_list_folders = Neerzetten om de replays in { $count ->
    [one] { $count } map
   *[other] { $count } mappen
} weer te geven
ui-app-drop_to_open_and_list = Neerzetten om { $file } te openen en de replays in { $folders } weer te geven
ui-app-drop_to_list_replays_and_folders = Neerzetten om { $count ->
    [one] { $count } replay
   *[other] { $count } replays
} en de replays in { $folders } weer te geven
ui-app-drop_folder_count = { $count ->
    [one] { $count } map
   *[other] { $count } mappen
}
ui-app-reading_replay = { $replay } lezen
ui-app-reading_replays = { $count ->
    [one] { $count } replay lezen
   *[other] { $count } replays lezen
}
ui-app-status_constants_generating = Spelconstanten bijwerken voor { $version }
ui-app-worker_constants = Het spelconstantenproces
ui-app-worker_live_match = De gevechtsbewaking
ui-app-worker_replay_loader = De replaylader
ui-app-worker_restarting = { $worker } is gestopt en wordt opnieuw gestart
ui-app-worker_stalled = { $worker } reageert niet
ui-app-worker_stopped = { $worker } is gestopt
ui-app-status_indexing = Replays indexeren
ui-app-status_of = { $done } van { $total }
ui-app-status_tasks = { $count ->
    [one] { $count } taak actief
   *[other] { $count } taken actief
}
ui-app-status_show_panel = Paneel tonen: { $panel }
ui-app-status_hide_panel = Paneel verbergen: { $panel }
ui-app-panel_left = Links
ui-app-panel_right = Rechts
ui-app-panel_bottom = Onder
ui-app-debug_build = Debugbuild

## ui.palette
ui-palette-title = Opdrachten
ui-palette-placeholder = Typ een opdracht of zoek...
ui-palette-go_to = Ga naar: { $tab }
ui-palette-advanced_search = Geavanceerd zoeken...
ui-palette-set_theme = Thema: { $theme }
ui-palette-games_i_died_in = Spellen waarin ik ben vernietigd
ui-palette-games_i_won = Spellen die ik heb gewonnen
ui-palette-send_all_replays = Alle replays naar ShipBuilds sturen
ui-palette-send_all_replays_again = Alle replays naar ShipBuilds sturen (register negeren)
ui-palette-search_players = Spelers zoeken...
ui-palette-my_matches_in_ship = Mijn wedstrijden in schip...
ui-palette-open_ship_in_armor_viewer = Schip openen in Modelweergave...
ui-palette-open_ship_in_shiptool = Schip openen in ShipTool...
ui-palette-players_title = Spelers zoeken
ui-palette-my_ships_title = Mijn wedstrijden in schip
ui-palette-armor_ships_title = Schip openen in Modelweergave
ui-palette-shiptool_ships_title = Schip openen in ShipTool
ui-palette-players_placeholder = Spelers zoeken op naam...
ui-palette-my_ships_placeholder = Schepen zoeken die u hebt gespeeld...
ui-palette-armor_ships_placeholder = Alle schepen zoeken op naam...
ui-palette-armor_ship_row = { $name } -- Tier { $tier } { $class }, { $nation }
ui-palette-back_hint = Esc: terug
ui-palette-ships_loading = Scheepslijst laden...
ui-palette-ships_unavailable = Geen scheepslijst: { $reason }
ui-palette-matches_found = { $count ->
    [one] { $count } resultaat
   *[other] { $count } resultaten
}
ui-palette-sample_stats = Debug: voorbeeldgegevens tonen in Statistieken
ui-palette-restore_stats = Debug: opgenomen gegevens tonen in Statistieken

## ui.replay.hover
ui-replay-hover-ship = Schip
ui-replay-hover-map = Kaart
ui-replay-hover-mode = Modus
ui-replay-hover-blank_mode = Leeg
ui-replay-hover-played = Gespeeld
ui-replay-hover-damage = Schade
ui-replay-hover-kills = Eliminaties
ui-replay-hover-result = Resultaat
ui-replay-hover-division = Divisie
ui-replay-hover-twitch = Twitch-chat

## ui.replay.renderer
ui-replay-renderer-export_video = Renderen naar video
ui-replay-renderer-export_clipboard = Renderen naar klembord
ui-replay-renderer-export_saved = Video opgeslagen in { $path }
ui-replay-renderer-export_copied = Video naar het klembord gekopieerd
ui-replay-renderer-export_not_written = De video kon niet worden gerenderd: { $reason }
ui-replay-renderer-pop_out = Openen in een eigen venster
ui-replay-renderer-export_path_unusable = Naar die locatie kan niet worden geschreven
ui-replay-renderer-batch_started = { $count ->
    [one] { $count } replay renderen
   *[other] { $count } replays renderen
} naar { $dir }
ui-replay-renderer-batch_finished = { $written ->
    [one] { $written } replay gerenderd
   *[other] { $written } replays gerenderd
}; { $failed ->
    [one] { $failed } kon niet worden gerenderd
   *[other] { $failed } konden niet worden gerenderd
}
ui-replay-renderer-batch_all_written = { $written ->
    [one] { $written } replay gerenderd
   *[other] { $written } replays gerenderd
}
ui-replay-renderer-batch_copied = { $written ->
    [one] { $written } replay gerenderd en op het klembord gezet
   *[other] { $written } replays gerenderd en op het klembord gezet
}
ui-replay-renderer-batch_not_copied = Gerenderd in { $dir }, maar het klembord weigerde ze: { $reason }
ui-replay-renderer-batch_progress = Renderen { $done } van { $total }: { $replay }
ui-replay-renderer-baking = Replay lezen en gevecht doorlopen...
ui-replay-renderer-failed = Deze replay kon niet worden afgespeeld
ui-replay-renderer-play = Afspelen
ui-replay-renderer-pause = Pauze

## ui.renderer.annotations
ui-renderer-annotations-title = Tekengereedschap
ui-renderer-annotations-undo = Ongedaan maken
ui-renderer-annotations-redo = Opnieuw
ui-renderer-annotations-ship_hint = Zoek een schip
ui-renderer-annotations-no_ship = Geen schip gekozen
ui-renderer-annotations-nib = Lijndikte
ui-renderer-annotations-thinner = Dunnere lijn
ui-renderer-annotations-thicker = Dikkere lijn
ui-renderer-annotations-shortcuts_title = Sneltoetsen
ui-renderer-annotations-shortcuts_tools = Gereedschappen
ui-renderer-annotations-shortcuts_actions = Acties
ui-renderer-annotations-tool_select = Selecteren
ui-renderer-annotations-tool_arrow = Pijl
ui-renderer-annotations-tool_freehand = Uit de vrije hand
ui-renderer-annotations-tool_line = Lijn
ui-renderer-annotations-tool_circle = Cirkel
ui-renderer-annotations-tool_rectangle = Rechthoek
ui-renderer-annotations-tool_triangle = Driehoek
ui-renderer-annotations-tool_measurement = Meten
ui-renderer-annotations-tool_eraser = Gum
ui-renderer-annotations-action_undo = Ongedaan maken
ui-renderer-annotations-action_multi_select = Meervoudige selectie
ui-renderer-annotations-action_stroke_width = Lijndikte
ui-renderer-annotations-action_delete = Verwijderen
ui-renderer-annotations-action_cancel = Annuleren / Deselecteren
ui-renderer-annotations-toggle_fill = Klik nogmaals om de vulling aan of uit te zetten
ui-renderer-annotations-clear_all = Alles wissen
ui-renderer-annotations-edit = Tekening bewerken
ui-renderer-annotations-size = Grootte
ui-renderer-annotations-color = Kleur
ui-renderer-annotations-custom_ink = Aangepaste kleur
ui-renderer-annotations-filled = Gevuld
ui-renderer-annotations-locked = Annotaties vergrendeld door de host

## ui.armor.cast
ui-armor-cast-prompt = { $key }+klik op de romp om een inslagpunt toe te voegen
ui-armor-cast-empty = Nog geen inslagpunten
ui-armor-cast-miss_nothing_under_pointer = Geen pantserplaat onder de aanwijzer
ui-armor-cast-miss_thickness_unknown = De platen van dit model hebben geen bekende dikte om op te vuren
ui-armor-cast-miss_no_plate_on_path = De baan van de granaat vanuit deze hoek kruist geen pantserplaat

## ui.armor.pen
ui-armor-pen-search_hint = Schepen zoeken om te vergelijken...
ui-armor-pen-add_a_ship = Zoek en voeg hierboven schepen toe om penetratie te vergelijken
ui-armor-pen-hover_a_plate = Wijs een plaat op het schip aan om deze te controleren
ui-armor-pen-awaiting_plate = wijs een plaat aan
ui-armor-pen-clear_all = Alles wissen
ui-armor-pen-remove = Verwijderen uit vergelijking
ui-armor-pen-firing_ship_tooltip = De granaten van dit schip gebruiken voor banen
ui-armor-pen-use_for_trajectory = Gebruiken voor baan
ui-armor-pen-no_shells = { $ship } heeft geen granaten voor het hoofdgeschut
ui-armor-pen-ifhe = IFHE
ui-armor-pen-ifhe_tooltip = Traagheidsontsteker voor brisantgranaten verhoogt de HE-penetratie met een kwart
ui-armor-pen-penetrates = penetreert
ui-armor-pen-shatters = versplintert
ui-armor-pen-angle_dependent = hangt af van de hoek
ui-armor-pen-no_data = geen penetratiegegevens

## ui.armor.splash
ui-armor-splash-hint = Klik in splashmodus op pantser om te zien welke zones een explosie bereikt
ui-armor-splash-half_extent = Halve omvang van de explosie: { $value } modeleenheden
ui-armor-splash-triangles = Pantserdriehoeken in de explosie: { $total }, gepenetreerd: { $penetrated }
ui-armor-splash-no_zones = Op dit punt overlappen geen splashboxen
ui-armor-splash-zones = Zones in de explosie
ui-armor-splash-direct_hit = Voltreffer
ui-armor-splash-hp = { $value } HP
ui-armor-splash-section = Splashzones

## ui.armor.export
ui-armor-export-hull = Romp
ui-armor-export-contents = Inhoud
ui-armor-export-contents_mesh = Mesh
ui-armor-export-contents_armor = Pantser
ui-armor-export-contents_both = Mesh en pantser
ui-armor-export-lod = Meshdetail
ui-armor-export-lod_tooltip = 0 is het hoogste detail. Pantser heeft geen detailniveaus en wordt niet beïnvloed.
ui-armor-export-res = Textuurresolutie
ui-armor-export-res_full = Volledig
ui-armor-export-res_2048 = 2048 px
ui-armor-export-res_1024 = 1024 px
ui-armor-export-res_512 = 512 px
ui-armor-export-camos = Camouflages
ui-armor-export-camo_select_none = Niets selecteren
ui-armor-export-camo_reset = Standaard herstellen
ui-armor-export-loading_details = Laden...
ui-armor-export-size_estimate = Geschatte bestandsgrootte: { $size }
ui-armor-export-size_breakdown = Geometrie { $geometry } (exact), texturen { $textures } (geschat)
ui-armor-export-size_incomplete = Grootte van { $count ->
    [one] { $count } camo
   *[other] { $count } camo's
} kon niet worden bepaald; grootte is mogelijk onvolledig
ui-armor-export-exporting = { $ship } exporteren...
ui-armor-export-already_running = Er loopt al een export
ui-armor-export-exported = { $ship } geëxporteerd
ui-armor-export-exported_with_size = { $ship } geëxporteerd ({ $size })
ui-armor-export-vehicle_not_found = Schip niet gevonden
ui-armor-export-export_failed = Exporteren mislukt: { $error }

## ui.armor.outcome
ui-armor-outcome-penetration = Penetratie
ui-armor-outcome-citadel = Citadel
ui-armor-outcome-ricochet = Ricochet
ui-armor-outcome-shatter = Versplinterd
ui-armor-outcome-overpenetration = Overpenetratie
ui-armor-outcome-underwater = Onder water
ui-armor-outcome-unknown = Niet herkend ({ $raw })
ui-armor-outcome-stopped = Gestopt in het pantser

## ui.armor.sim
ui-armor-sim-overmatch_rules_out_ricochet = Overmatch, dus geen ricochet
ui-armor-sim-always_ricochet_band = Ricochet, in de zone van gegarandeerde ricochet
ui-armor-sim-ricochet_angle_too_low = Geen ricochet mogelijk, hoek te klein

## ui.tactics
ui-tactics-a_peers_board = Een bord gedeeld door iemand anders in deze sessie
ui-tactics-map = Kaart
ui-tactics-map_hint = Zoek een kaart
ui-tactics-mode = Modus
ui-tactics-pick_a_map = Kies een kaart om op te tekenen
ui-tactics-drawing = Kaart tekenen...
ui-tactics-reset_view = Weergave herstellen
ui-tactics-add_cap = Innamepunt toevoegen
ui-tactics-add_cap_tooltip = Een innamepunt plaatsen met de volgende klik op de kaart
ui-tactics-selected_cap = Innamepunt { $letter }
ui-tactics-cap_radius = { $km } km
ui-tactics-cap_team = Team
ui-tactics-cap_team_tooltip = Het team van het geselecteerde innamepunt wisselen
ui-tactics-team_neutral = Neutraal
ui-tactics-team_one = Team 1 (groen)
ui-tactics-team_two = Team 2 (rood)
ui-tactics-delete_cap = Verwijderen
ui-tactics-delete_cap_tooltip = Het geselecteerde innamepunt verwijderen
ui-tactics-cap_replay_delete_tooltip = Een innamepunt dat uit een replay is geladen kan niet worden verwijderd
ui-tactics-cap_narrow_tooltip = Straal van het geselecteerde innamepunt verkleinen
ui-tactics-cap_widen_tooltip = Straal van het geselecteerde innamepunt vergroten
ui-tactics-undo_tooltip = Laatste wijziging aan het bord ongedaan maken ({ $shortcut })
ui-tactics-redo_tooltip = Laatst ongedaan gemaakte wijziging opnieuw uitvoeren ({ $shortcut } of { $alternate })
ui-tactics-clear_caps = Toegevoegde innamepunten wissen
ui-tactics-clear_added_caps_tooltip = Door de gebruiker toegevoegde innamepunten verwijderen
ui-tactics-preset_name = Bordnaam
ui-tactics-preset_save = Opslaan
ui-tactics-preset_delete = Verwijderen
ui-tactics-reset_board = Bord herstellen
ui-tactics-reset_board_tooltip = Innamepunten van de geselecteerde modus herstellen en tekeningen wissen
ui-tactics-preset_needs_a_name = Geef het bord eerst een naam
ui-tactics-preset_needs_a_map = Kies eerst een kaart voor het bord
ui-tactics-preset_saved = { $name } opgeslagen
ui-tactics-preset_save_failed = Het bord kon niet worden opgeslagen: { $error }
ui-tactics-preset_load_failed = Het bord kon niet worden geopend: { $error }
ui-tactics-preset_delete_failed = Het bord kon niet worden verwijderd: { $error }
ui-tactics-presets = Opgeslagen borden
ui-tactics-preset_open = Openen
ui-tactics-clear_drawing = Tekening wissen
ui-tactics-scan_replays = Innamepunten vullen vanuit replays
ui-tactics-scan_replays_tooltip = De replays lezen voor de indeling van innamepunten van de modi waarin ze zijn gespeeld
ui-tactics-scan_needs_game_data = Laad eerst een spelbuild
ui-tactics-scan_running = Replays lezen...
ui-tactics-scan_progress = Replays lezen... { $read } / { $total }
ui-tactics-scan_done = { $total ->
    [one] { $total } replay gelezen
   *[other] { $total } replays gelezen
} en { $added ->
    [one] { $added } nieuwe indeling gevonden
   *[other] { $added } nieuwe indelingen gevonden
}
ui-tactics-session_boards_full = De sessie heeft al { $limit ->
    [one] { $limit } bord
   *[other] { $limit } borden
} open, dus dit bord blijft op zijn kaart
ui-tactics-placing = { $ship } plaatsen
ui-tactics-side_friendly = Bondgenoot
ui-tactics-side_enemy = Vijand
ui-tactics-ink_white = Witte inkt
ui-tactics-ink_gray = Grijze inkt
ui-tactics-ink_red = Rode inkt
ui-tactics-ink_orange = Oranje inkt
ui-tactics-ink_yellow = Gele inkt
ui-tactics-ink_green = Groene inkt
ui-tactics-ink_blue = Blauwe inkt
ui-tactics-ink_purple = Paarse inkt
ui-tactics-ink_pink = Roze inkt
ui-tactics-no_map_art = Deze build bevat geen afbeelding voor die kaart
ui-tactics-ship_config = Scheepsconfiguratie
ui-tactics-hull = Romp
ui-tactics-hull_default = Standaard
ui-tactics-modifiers = Modificatoren
ui-tactics-concealment_expert = Camouflage-expert
ui-tactics-gun_range_mod = Mod. kanonbereik
ui-tactics-secondary_range_mod = Mod. secundair bereik
ui-tactics-ship_ranges = Bereikcirkels
ui-tactics-shape = Vorm
ui-tactics-shape_size = Grootte
ui-tactics-shape_size_km = Grootte (km)
ui-tactics-shape_filled = Gevuld
ui-tactics-shape_smaller = Vorm verkleinen
ui-tactics-shape_larger = Vorm vergroten

## ui.launcher
ui-launcher-rescan = Opnieuw scannen
ui-launcher-add_install = Installatie toevoegen...
ui-launcher-add_install_title = Installatiemap van World of Warships
ui-launcher-no_installs = Geen installaties gevonden. Voeg er een toe, of installeer hieronder een spel.
ui-launcher-managed_by_steam = beheerd door Steam
ui-launcher-checking_updates = controleren op updates
ui-launcher-up_to_date = Up-to-date
ui-launcher-update_available = Update beschikbaar: { $parts }
ui-launcher-not_installed = niet geïnstalleerd: { $parts }
ui-launcher-check_failed = updatecontrole mislukt: { $error }
ui-launcher-recheck = Opnieuw controleren
ui-launcher-download = Downloaden / bijwerken...
ui-launcher-download_title = Waar de build moet worden geïnstalleerd
ui-launcher-download_hint = Een nieuwe installatie is tientallen GB; delta-updates worden nog niet ondersteund, dus een gewijzigd onderdeel wordt volledig opnieuw gedownload.
ui-launcher-pause = Pauzeren
ui-launcher-cancel = Annuleren
ui-launcher-resume = Hervatten
ui-launcher-paused = Download gepauzeerd; tijdelijke bestanden blijven bewaard.
ui-launcher-installed = Geïnstalleerd
ui-launcher-installed_skipped = Geïnstalleerd ({ $count ->
    [one] { $count } delta-onderdeel
   *[other] { $count } delta-onderdelen
} overgeslagen: nog niet ondersteund, gedownload maar niet toegepast)
ui-launcher-download_failed = Download mislukt: { $error }
ui-launcher-update_in_place = Een update komt in de map van de installatie zelf. Kies { $path } om deze bij te werken.
ui-launcher-cancelled = Download geannuleerd.
ui-launcher-launch_failed = Starten mislukt: { $error }
ui-launcher-use_for_toolkit = Gebruiken voor Toolkit
ui-launcher-install_heading = Een spel installeren
ui-launcher-install_hint = Een nieuwe installatie is tientallen GB.
ui-launcher-install_title = Waar het spel moet worden geïnstalleerd
ui-launcher-install = Installeren...
ui-launcher-checking_version = beschikbare versie controleren
ui-launcher-version = versie { $version }
ui-launcher-version_unreported = clientversie niet gemeld
ui-launcher-unavailable = niet beschikbaar: { $error }
ui-launcher-retry = Opnieuw proberen

## ui.launcher.account
ui-launcher-account-sign_in = Aanmelden
ui-launcher-account-add_title = Account toevoegen
ui-launcher-account-again_title = Opnieuw aanmelden
ui-launcher-account-memory_only = Alleen voor deze sessie aangemeld. De aanmelding wordt niet opgeslagen, dus u moet zich na een herstart opnieuw aanmelden.
ui-launcher-account-password_note = Uw wachtwoord gaat alleen naar Wargaming en wordt nooit opgeslagen.
ui-launcher-account-realm_fixed = Regio: { $realm }
ui-launcher-account-email = E-mail
ui-launcher-account-password = Wachtwoord
ui-launcher-account-code = Tweefactorcode
ui-launcher-account-code_required = Tweefactorcode vereist.
ui-launcher-account-use_backup_code = Back-upcode gebruiken
ui-launcher-account-submit_code = Code verzenden
ui-launcher-account-captcha = Deze aanmelding vereist een CAPTCHA, die deze launcher nog niet kan tonen. Meld u eenmalig aan via het officiële Wargaming Game Center of via wargaming.net in een browser en probeer het daarna opnieuw.

## ui.launcher.sign_in_error
ui-launcher-sign_in_error-wrong_credentials = Onjuist e-mailadres of wachtwoord.
ui-launcher-sign_in_error-two_factor_required = Voor dit account is tweestapsgoedkeuring nodig. Keur de aanmelding goed in Wargaming Auth of met de gemailde code en meld u daarna opnieuw aan.
ui-launcher-sign_in_error-two_factor_invalid = De tweestapscode is onjuist of verlopen. Voer een nieuwe code in.
ui-launcher-sign_in_error-captcha_required = Wargaming vraagt om een CAPTCHA. Meld u eenmaal aan in het officiële Game Center of op wargaming.net en probeer het daarna opnieuw.
ui-launcher-sign_in_error-rate_limited = Te veel aanmeldpogingen. Wacht een paar minuten en probeer het daarna opnieuw.
ui-launcher-sign_in_error-account_banned = Dit account is verbannen.
ui-launcher-sign_in_error-network_unreachable = Wargaming is niet bereikbaar. Controleer uw internetverbinding.
ui-launcher-sign_in_error-service_unavailable = De aanmelddienst van Wargaming is niet beschikbaar. Probeer het later opnieuw.
ui-launcher-sign_in_error-session_expired = De opgeslagen aanmelding is verlopen. Meld u opnieuw aan.
ui-launcher-sign_in_error-keyring_locked = Uw systeemsleutelbos is vergrendeld. Ontgrendel het en meld u daarna opnieuw aan.
ui-launcher-sign_in_error-not_saved = Aangemeld, maar de aanmelding kon niet worden opgeslagen. Na een herstart moet u zich opnieuw aanmelden.
ui-launcher-sign_in_error-unexpected = Aanmelden mislukt. Het logboek bevat de details.

## ui.launcher.realm
ui-launcher-realm-eu = Europa
ui-launcher-realm-na = Noord-Amerika
ui-launcher-realm-asia = Azië

## ui.launcher.runner
ui-launcher-runner-looking = Zoeken naar Proton.
ui-launcher-runner-steam_proton = { $name } van Steam
ui-launcher-runner-umu = umu-run met { $name }
ui-launcher-runner-umu_default = umu-run met UMU-Proton
ui-launcher-runner-native = Het spel draait native.
ui-launcher-runner-first_run_prefix = De eerste start maakt een Wine-prefix aan, dit duurt enkele seconden.
ui-launcher-runner-first_run_download = De eerste start downloadt UMU-Proton en een Steam-runtime, ongeveer 700 MB.
ui-launcher-runner-choose = Kies een Proton-map
ui-launcher-runner-automatic = Automatisch zoeken
ui-launcher-runner-detect = Opnieuw zoeken
ui-launcher-runner-needed = Proton is nodig om op Linux te starten. Installeer Steam met Proton, of umu-launcher.

## ui.replay.damage_share
ui-replay-damage_share-mode = Schade als aandeel van het totaal van het andere schip
ui-replay-damage_share-dealt = { $pct } van de schade die { $ship } toebracht ging naar { $other }.
ui-replay-damage_share-dealt_inverse = { $pct } van de schade die { $other } opliep kwam van { $ship }.
ui-replay-damage_share-received = { $pct } van de schade die { $ship } opliep kwam van { $other }.
ui-replay-damage_share-received_inverse = { $pct } van de schade die { $other } toebracht ging naar { $ship }.
ui-replay-damage_share-hold = Houd { $key } ingedrukt om om te keren.
ui-replay-damage_share-release = Laat { $key } los om terug te keren.

## ui.reindex
ui-reindex-context = { $count ->
    [one] Opnieuw indexeren afdwingen
   *[other] { $count } replays opnieuw indexeren afdwingen
}
ui-reindex-palette_by_version = Replays opnieuw indexeren afdwingen: per versie
ui-reindex-palette_all = Replays opnieuw indexeren afdwingen: alle
ui-reindex-versions_title = Replays opnieuw indexeren afdwingen: per versie
ui-reindex-versions_placeholder = Spelversies filteren...
ui-reindex-version_row = { $version } -- { $count ->
    [one] { $count } replay
   *[other] { $count } replays
}
ui-reindex-no_versions = Geen enkele vermelde replay noemt een spelversie
ui-reindex-confirm_title = Replays opnieuw indexeren afdwingen
ui-reindex-confirm_ok = Opnieuw indexeren
ui-reindex-confirm_count = { $count ->
    [one] { $count } replay wordt opnieuw gelezen en het indexitem herschreven.
   *[other] { $count } replays worden opnieuw gelezen en hun indexitems herschreven.
}
ui-reindex-confirm_range = Gespeeld van { $oldest } tot { $newest }.
ui-reindex-confirm_undated = { $count ->
    [one] { $count } te lezen replay heeft geen leesbare gevechtsdatum en valt niet binnen die periode.
   *[other] { $count } te lezen replays hebben geen leesbare gevechtsdatum en vallen niet binnen die periode.
}
ui-reindex-confirm_downloads = Spelgegevens voor deze versies worden eerst gedownload:
ui-reindex-confirm_unobtainable = Voor deze versies zijn geen spelgegevens te verkrijgen, dus { $count ->
    [one] hun { $count } replay wordt overgeslagen:
   *[other] hun { $count } replays worden overgeslagen:
}
ui-reindex-confirm_no_cache_dir = Er is geen cachemap voor spelgegevens ingesteld, dus spelgegevens voor deze versies kunnen niet worden gedownload en { $count ->
    [one] hun { $count } replay wordt overgeslagen:
   *[other] hun { $count } replays worden overgeslagen:
}
ui-reindex-confirm_no_build = { $count ->
    [one] { $count } replay noemt geen spelbuild en wordt overgeslagen.
   *[other] { $count } replays noemen geen spelbuild en worden overgeslagen.
}
ui-reindex-confirm_no_version = { $count ->
    [one] { $count } replay noemt geen leesbare spelversie en wordt overgeslagen.
   *[other] { $count } replays noemen geen leesbare spelversie en worden overgeslagen.
}
ui-reindex-build_row = { $version } (build { $build }): { $count ->
    [one] { $count } replay
   *[other] { $count } replays
}
ui-reindex-nothing_listed = De lijst bevat geen replays om opnieuw te indexeren.
ui-reindex-nothing_readable = Geen van deze replays kan opnieuw worden geïndexeerd: er zijn geen spelgegevens beschikbaar ({ $count } overgeslagen).
ui-reindex-no_game_data = Replays kunnen pas worden gelezen als de spelmap is ingesteld.
ui-reindex-busy_index = Er loopt al een indexering.
ui-reindex-busy_cache = Er loopt al een taak voor spelgegevens. Probeer het opnieuw wanneer deze klaar is.
ui-reindex-planning = Controleren welke spelgegevens beschikbaar zijn...
ui-reindex-no_index = De replay-index is niet geopend, dus er is niets opnieuw geïndexeerd.
ui-reindex-done = { $indexed ->
    [one] { $indexed } replay opnieuw geïndexeerd
   *[other] { $indexed } replays opnieuw geïndexeerd
}; { $skipped } overgeslagen, { $failed ->
    [one] { $failed } kon niet worden gelezen
   *[other] { $failed } konden niet worden gelezen
}
ui-reindex-stopped = Opnieuw indexeren gestopt: { $indexed } opnieuw geïndexeerd, { $skipped } overgeslagen, { $failed } konden niet worden gelezen
ui-reindex-stopped_before_reading = Opnieuw indexeren gestopt voordat er een replay was gelezen.

## ui.index_folder
ui-index_folder-tooltip = Alle replays in deze lijst indexeren die nog niet in de replay-index staan
ui-index_folder-confirm_title = Replays indexeren
ui-index_folder-confirm_ok = Indexeren
ui-index_folder-confirm_count = { $count ->
    [one] { $count } replay in deze lijst staat nog niet in de replay-index en wordt ingelezen en toegevoegd.
   *[other] { $count } replays in deze lijst staan nog niet in de replay-index en worden ingelezen en toegevoegd.
}
ui-index_folder-note_live = Geïndexeerde replays worden toegevoegd aan de replay-index die Zoeken en de Spelersvolger lezen.
ui-index_folder-note_folder = Geïndexeerde replays worden toegevoegd aan de replay-index onder de eigen bron van deze map ({ $dir }). Zoeken en de spelerslijst van de Spelersvolger nemen ze op. De clanlijst van de Spelersvolger leest alleen de eigen replays van het spel.
ui-index_folder-nothing_listed = Deze lijst bevat geen replays om te indexeren.
ui-index_folder-nothing_missing = Alle replays in deze lijst zijn al geïndexeerd.
ui-index_folder-unreadable_index = De replay-index kon niet worden gelezen.
ui-index_folder-done = { $indexed ->
    [one] { $indexed } replay geïndexeerd
   *[other] { $indexed } replays geïndexeerd
}; { $skipped } overgeslagen, { $failed ->
    [one] { $failed } kon niet worden gelezen
   *[other] { $failed } konden niet worden gelezen
}
ui-index_folder-stopped = Indexeren gestopt: { $indexed } geïndexeerd, { $skipped } overgeslagen, { $failed } konden niet worden gelezen
ui-index_folder-stopped_before_reading = Het indexeren is gestopt voordat er een replay werd gelezen.

## ui.replay_folders
ui-replay_folders-install = Spel
ui-replay_folders-watched = Bewaakt: nieuwe en gewijzigde replays verschijnen hier
ui-replay_folders-unwatched = Niet bewaakt: gebruik Vernieuwen om opnieuw te lezen
ui-replay_folders-unseen = { $count ->
    [one] { $count } nieuwe of gewijzigde replay sinds je laatste bezoek
   *[other] { $count } nieuwe of gewijzigde replays sinds je laatste bezoek
}
ui-replay_folders-missing = Deze map bestaat niet meer: { $dir }
ui-replay_folders-missing_hint = Hij wordt bij elke start hier geopend totdat je hem losmaakt.
ui-replay_folders-pin = Vastzetten
ui-replay_folders-unpin = Losmaken
ui-replay_folders-refresh = Vernieuwen
ui-replay_folders-close = Sluiten
ui-replay_folders-pins_unreadable = Vastgezette replaymappen konden niet worden gelezen.
ui-replay_folders-game_data_available = Spelgegevens zijn beschikbaar voor de getoonde replays.
ui-replay_folders-browse = Bladeren...
ui-replay_folders-recent_title = Replaymap openen
ui-replay_folders-recent_placeholder = Recente mappen filteren...
ui-replay_folders-recent_missing = Deze map bestaat niet meer en is uit de lijst met recente mappen verwijderd: { $dir }
ui-replay_folders-checking = Zoeken naar { $dir }...
ui-replay_folders-indexing = { $dir } indexeren
ui-replay_folders-indexing_in_background = Deze map wordt nog geïndexeerd
ui-replay_folders-search_while_indexing = Deze map wordt nog geïndexeerd, dus de resultaten zijn nog niet volledig. Zoek opnieuw zodra het klaar is.
ui-replay_folders-source_failed = De replays in deze map kunnen niet worden geïndexeerd: { $error }
ui-replay_folders-restart_watch = Bewaking herstarten
ui-replay_folders-watch_stopped = De bewaking van { $folder } is gestopt: { $cause }.
ui-replay_folders-watch_restarting = Herstart over { $seconds } s (poging { $attempt } van { $attempts }).
ui-replay_folders-watch_gave_up = Herstarten is { $attempts } keer mislukt en wordt niet opnieuw geprobeerd.
ui-replay_folders-watch_restart_hint = Klik op de rode stip of kies Bewaking herstarten in het tabbladmenu om haar nu te herstarten.
ui-replay_folders-watch_resumed = De bewaking van { $folder } is hervat.
ui-replay_folders-watch_cause_backend = het bestandssysteem meldde een fout: { $error }
ui-replay_folders-watch_cause_gone = de map is verplaatst of verwijderd
ui-replay_folders-watch_cause_panicked = interne fout: { $error }
ui-replay_folders-watch_cause_exited = de bewaking is onverwacht beëindigd
ui-replay_folders-dropped = Neergezet
ui-replay_folders-dropped_numbered = Neergezet { $number }
ui-replay_folders-dropped_count = { $count ->
    [one] { $count } neergezette replay
   *[other] { $count } neergezette replays
}
ui-replay_folders-dropped_no_source = Neergezette replays worden met hun eigen mappen geïndexeerd. Open een map om deze te doorzoeken of te indexeren.
ui-replay_folders-unreadable_replay = Kon niet worden gelezen: { $error }

## ui.models
ui-models-heading = Modellen
ui-models-loading = Modellen laden...
ui-models-unavailable = Modellen niet beschikbaar
ui-models-search = Schepen en modellen zoeken...
ui-models-open = Openen
ui-models-export = Model exporteren
ui-models-copy_path = Modelpad kopiëren
ui-models-path_copied = { $path } gekopieerd
ui-models-skin_replaces = Vervangt { $model } als { $exterior } is uitgerust
ui-models-geometry_absent = Niet in je spelbestanden
ui-models-geometry_none = Dit model heeft geen geometrie
ui-models-hardpoints_tooltip = Bevestigingspunten tonen
ui-models-common = Gemeenschappelijke modellen

## ui.models.group
ui-models-group-gameplay = Spel
ui-models-group-building = Gebouwen
ui-models-group-character = Personages
ui-models-group-location = Locaties
ui-models-group-port = Havens
ui-models-group-style = Stijlen
ui-models-group-other = Overige modellen

## ui.models.building
ui-models-building-weapon = Gebouwwapens
ui-models-building-unassigned = Niet-toegewezen gebouwen
ui-models-building-unspecified = Gebouwen zonder type

## ui.models.misc
ui-models-misc-crane = Kranen
ui-models-misc-anchor = Ankers
ui-models-misc-deck_fitting = Dekbeslag
ui-models-misc-instrument = Instrumenten
ui-models-misc-propeller = Schroeven
ui-models-misc-searchlight = Zoeklichten
ui-models-misc-boat = Boten
ui-models-misc-flag = Vlaggen
ui-models-misc-insignia = Insignes
ui-models-misc-railing = Relingen
ui-models-misc-hatch = Luiken
ui-models-misc-ventilation = Ventilatie
ui-models-misc-antenna = Antennes
ui-models-misc-weapon = Wapens
ui-models-misc-stowage = Opslag
ui-models-misc-other = Overig

## ui.models.variant
ui-models-variant-dead = Wrak
ui-models-variant-skin = Uiterlijk

## ui.models.kind
ui-models-kind-ship = Schepen
ui-models-kind-gun = Kanonnen
ui-models-kind-director = Vuurleidingsposten
ui-models-kind-finder = Afstandsmeters
ui-models-kind-radar = Radars
ui-models-kind-catapult = Katapulten
ui-models-kind-aircraft = Vliegtuigen
ui-models-kind-projectile = Projectielen
ui-models-kind-misc = Overig
ui-models-kind-cap = Veroveringspunten
ui-models-kind-other = Anders

## ui.models.sub_kind
ui-models-sub_kind-battleship = Slagschepen
ui-models-sub_kind-cruiser = Kruisers
ui-models-sub_kind-destroyer = Torpedobootjagers
ui-models-sub_kind-aircraft_carrier = Vliegdekschepen
ui-models-sub_kind-submarine = Onderzeeërs
ui-models-sub_kind-auxiliary = Hulpschepen
ui-models-sub_kind-main_battery = Hoofdbatterij
ui-models-sub_kind-secondary_battery = Secundaire batterij
ui-models-sub_kind-anti_aircraft = Luchtafweer
ui-models-sub_kind-torpedo_tubes = Torpedobuizen
ui-models-sub_kind-depth_charge_launcher = Dieptebomwerpers
ui-models-sub_kind-missile_launcher = Raketwerpers
ui-models-sub_kind-search_radar = Zoekradars
ui-models-sub_kind-fire_control_radar = Vuurleidingsradars
ui-models-sub_kind-fighter = Jachtvliegtuigen
ui-models-sub_kind-bomber = Bommenwerpers
ui-models-sub_kind-dive_bomber = Duikbommenwerpers
ui-models-sub_kind-scout = Verkenners
ui-models-sub_kind-torpedo = Torpedo's
ui-models-sub_kind-bomb = Bommen
ui-models-sub_kind-rocket = Raketten
ui-models-sub_kind-depth_charge = Dieptebommen
ui-models-sub_kind-mine = Mijnen
ui-models-sub_kind-missile = Geleide raketten
ui-models-sub_kind-jato = Startraketten
ui-models-sub_kind-shell = Granaten

## ui.models.part
ui-models-part-hull = Romp
ui-models-part-main_battery = Hoofdbatterij
ui-models-part-secondaries = Secundair
ui-models-part-anti_aircraft = Luchtafweer
ui-models-part-torpedoes = Torpedo's
ui-models-part-directors = Vuurleidingsposten
ui-models-part-finders = Afstandsmeters
ui-models-part-radars = Radars
ui-models-part-depth_charges = Dieptebommen
ui-models-part-aircraft = Vliegtuigen
ui-models-part-other = Anders
ui-models-part-misc = Overig

## ui.model_view
ui-model_view-thickness_from_building = Dikte volgens { $building }
ui-model_view-thickness_from = Dikte van { $ship } ({ $hp })
ui-model_view-thickness_unknown = Dikte onbekend: geen schip voert dit model
ui-model_view-thickness_other_ships = Andere schepen
ui-model_view-export_title = Model exporteren
ui-model_view-export_tooltip = Het model naar een GLB-bestand exporteren
ui-model_view-part_model = Model: { $model }
ui-model_view-part_hardpoint = Bevestigingspunt: { $hardpoint }

## ui.model_view.part_group
ui-model_view-part_group-hull = Romp
ui-model_view-part_group-main_battery = Hoofdbatterij
ui-model_view-part_group-secondary_battery = Secundair
ui-model_view-part_group-aa_guns = Luchtafweer
ui-model_view-part_group-torpedoes = Torpedo's
ui-model_view-part_group-depth_charges = Dieptebommen
ui-model_view-part_group-fire_control = Vuurleiding
ui-model_view-part_group-radar = Radars
ui-model_view-part_group-missiles = Raketwerpers
ui-model_view-part_group-decorations = Decoraties
ui-model_view-part_group-misc = Overig
ui-model_view-part_group-armor = Pantser
ui-model_view-part_group-other = Anders

## ui.scene_problem
ui-scene_problem-cannot_show = { $name } kan niet worden weergegeven
ui-scene_problem-geometry_absent = De geometrie van dit model staat niet in je spelbestanden.
ui-scene_problem-model_unreadable = De gegevens van dit model konden niet worden gelezen. Het logboek bevat de details.
ui-scene_problem-ship_unreadable = De gegevens van dit schip konden niet worden gelezen. Het logboek bevat de details.
ui-scene_problem-no_meshes = Dit model heeft geen meshes.
ui-scene_problem-no_meshes_at_lod = Dit model heeft geen meshes op het gekozen detailniveau.
ui-scene_problem-no_visible_surface = Dit model heeft geen zichtbaar oppervlak om te tekenen.
ui-scene_problem-section_stand_in = Dit rompdeel heeft geen eigen geometrie. Het schip tekent dit deel van de romp in een ander deel.

## ui.armor.option_tabs
ui-armor-option_tabs-label = Optietabbladen
ui-armor-option_tabs-ship = Schip
ui-armor-option_tabs-layers = Lagen
ui-armor-option_tabs-analysis = Analyse
ui-armor-option_tabs-configuration = Configuratie
ui-armor-option_tabs-hull_parts = Rompdelen
ui-armor-option_tabs-tab_matches = { $tab }, { $count } overeenkomsten
ui-armor-option_tabs-empty = Niets op dit tabblad is van toepassing op het getoonde model.
ui-armor-option_tabs-no_choices = Dit model heeft geen romp, module, camouflage of detailniveau om te kiezen.

## ui.armor.hidden_ship
ui-armor-hidden_ship-clan_rental = Clanhuurschip
ui-armor-hidden_ship-other_class = Geen scheepsklasse

## ui.armor.textures
ui-armor-textures-title = Texturen
ui-armor-textures-hint = Het spel leest een bestand in de res_mods-map van de build in plaats van het verpakte bestand op hetzelfde pad. Klik met de rechtermuisknop op een textuur om dat pad te kopiëren.
ui-armor-textures-under_pointer = Onder de aanwijzer: { $material }
ui-armor-textures-reading = Texturen lezen...
ui-armor-textures-summary = { $materials ->
    [one] { $materials } materiaal
   *[other] { $materials } materialen
}, { $textures ->
    [one] { $textures } textuur
   *[other] { $textures } texturen
}
ui-armor-textures-no_material = Geen materiaal
ui-armor-textures-unresolved = Het pad staat niet in assets.bin
ui-armor-textures-absent = Niet in de geïnstalleerde pakketten
ui-armor-textures-unreadable = De DDS-header kon niet worden gelezen
ui-armor-textures-unknown_format = Onbekend formaat
ui-armor-textures-details = { $format }, { $size }, { $count ->
    [one] { $count } bestand
   *[other] { $count } bestanden
}, { $bytes }
ui-armor-textures-role_albedo = Albedo
ui-armor-textures-role_normal = Normaal
ui-armor-textures-role_metal_gloss = Metaal-glans
ui-armor-textures-role_ambient_occlusion = Ambient occlusion
ui-armor-textures-role_detail = Detail
ui-armor-textures-role_name_matched = Albedo, gevonden op bestandsnaam
ui-armor-textures-copy_path = Pad kopiëren
ui-armor-textures-copy_overrides = { $count ->
    [one] res_mods-pad kopiëren
   *[other] { $count } res_mods-paden kopiëren
}
ui-armor-textures-paths_copied = { $count ->
    [one] { $count } pad gekopieerd
   *[other] { $count } paden gekopieerd
}
ui-armor-textures-no_build = De spelgegevens zijn niet geladen, dus de modmap van de build is onbekend.
ui-armor-textures-show_in_unpacker = Tonen in Bronnen uitpakken
ui-armor-textures-show_textures = Texturen tonen
ui-armor-textures-extract = Bestanden uitpakken...
ui-armor-textures-extract_title = Textuurbestanden uitpakken naar
ui-armor-textures-extracting = Textuurbestanden uitpakken...
ui-armor-textures-extracted = { $count ->
    [one] { $count } bestand uitgepakt
   *[other] { $count } bestanden uitgepakt
} naar { $folder }
ui-armor-textures-extract_failed = De textuurbestanden konden niet worden uitgepakt: { $reason }
ui-armor-textures-replace_title = Bestaande bestanden vervangen?
ui-armor-textures-replace_description = { $count ->
    [one] { $count } van deze bestanden staat al in { $folder }. Uitpakken vervangt het.
   *[other] { $count } van deze bestanden staan al in { $folder }. Uitpakken vervangt ze.
}
ui-armor-textures-replace = Vervangen

## ui.settings.mcp
ui-settings-mcp-enabled = MCP-server inschakelen
ui-settings-mcp-enabled_description = Biedt replays en spelgegevens aan AI-assistenten via MCP. Toont een knop in de statusbalk.
ui-settings-mcp-start_on_launch = Starten met het programma
ui-settings-mcp-start_on_launch_description = Start de server wanneer WoWs Toolkit opent.
ui-settings-mcp-bind_address = Luisteradres
ui-settings-mcp-bind_address_description = Loopback-IP-adres waarop de server luistert, zoals 127.0.0.1 of ::1. Alleen deze computer kan de server bereiken.
ui-settings-mcp-port = Poort
ui-settings-mcp-port_description = TCP-poort om op te luisteren. Bij de eerste keer willekeurig gekozen en bewaard.
ui-settings-mcp-port_unset = Gekozen bij de eerste keer inschakelen
ui-settings-mcp-address_not_loopback = { $input } is geen loopback-adres. De server luistert alleen op deze computer, bijvoorbeeld op 127.0.0.1.
ui-settings-mcp-address_invalid = { $input } is geen IP-adres
ui-settings-mcp-port_invalid = { $input } is geen poort van 1 tot 65535
ui-settings-mcp-port_in_use = Een ander programma gebruikt deze poort. Kies een andere poort.

## ui.mcp
ui-mcp-started = MCP-server actief op { $url }
ui-mcp-copy_url = URL kopiëren
ui-mcp-start_failed = Kan de MCP-server niet starten: { $reason }
ui-mcp-port_in_use = poort { $port } op { $address } wordt door een ander programma gebruikt
ui-mcp-no_watchdog = de bewaking van achtergrondtaken draait niet
ui-mcp-status_running = MCP-server actief op { $url }. Klik om te stoppen.
ui-mcp-status_starting = MCP-server start op { $address }
ui-mcp-status_failed = MCP-server kon niet starten. Klik om opnieuw te proberen.
ui-mcp-status_stopped = MCP-server gestopt. Klik om te starten.

## ui.launcher.opt_in
ui-launcher-opt_in-setting = Spellauncher inschakelen
ui-launcher-opt_in-description = Speel World of Warships vanuit de toolkit op Linux, aangemeld met uw Wargaming-account.
ui-launcher-opt_in-title = Spellauncher inschakelen?
ui-launcher-opt_in-notice_account = Door de launcherfunctie te gebruiken, erkent u dat deze functie geen scenario is dat door de spelontwikkelaar wordt ondersteund. U voert uw wachtwoord rechtstreeks in deze applicatie in, die, als u deze niet vertrouwt, kan worden gebruikt om uw account te stelen (hoewel de ontwikkelaar van de applicatie dat niet wil doen).
ui-launcher-opt_in-notice_reports = Verder erkent u dat u, als u deze functie gebruikt, geen grafische of betrouwbaarheidsfouten aan de spelontwikkelaar zult melden, tenzij u er werkelijk van overtuigd bent dat het om een platformoverschrijdend probleem gaat dat NIET ALLEEN Linux treft.
ui-launcher-opt_in-continue = Doorgaan
ui-launcher-opt_in-cancel = Annuleren

## ui.launcher.keyring
ui-launcher-keyring-unencrypted = Uw aanmelding wordt onversleuteld opgeslagen in de gegevensmap van de app, omdat er geen Secret Service-aanbieder (zoals GNOME Keyring, KeePassXC of KDE Wallet) beschikbaar is.
ui-launcher-keyring-how = Zo blijft deze versleuteld
ui-launcher-keyring-how_detail = Installeer en start een Secret Service-aanbieder zoals GNOME Keyring, KeePassXC of KDE Wallet en start de toolkit daarna opnieuw. Opgeslagen aanmeldingen worden daarnaartoe verplaatst.
ui-launcher-keyring-unlock_hint = Uw systeemsleutelbos kan vragen om ontgrendeld te worden, zodat de toolkit uw opgeslagen aanmelding kan lezen.

## ui.launcher.play
ui-launcher-play-play = Spelen
ui-launcher-play-play_through_steam = Spelen via Steam
ui-launcher-play-set_up_game = Spel instellen
ui-launcher-play-set_up_runner = Proton instellen
ui-launcher-play-sign_in_again = Opnieuw aanmelden
ui-launcher-play-update = Bijwerken { $size }
ui-launcher-play-updating = Bijwerken { $percent }%
ui-launcher-play-updating_unknown = Bijwerken
ui-launcher-play-paused = Gepauzeerd
ui-launcher-play-starting = Starten...
ui-launcher-play-running = Actief
ui-launcher-play-in_battle = In gevecht
ui-launcher-play-menu = Accounts en spelopties

## ui.launcher.popover
ui-launcher-popover-game = World of Warships
ui-launcher-popover-no_install = Er is geen spelinstallatie ingesteld.
ui-launcher-popover-update_available = Update beschikbaar
ui-launcher-popover-check_failed = Kan niet controleren op updates
ui-launcher-popover-accounts = Accounts
ui-launcher-popover-no_accounts = Nog geen accounts. Voeg er een toe om aangemeld te spelen.
ui-launcher-popover-close_to_switch = Sluit het spel om van account te wisselen.
ui-launcher-popover-default = Standaard
ui-launcher-popover-ready = Gereed
ui-launcher-popover-restoring = Controleren...
ui-launcher-popover-not_saved = Niet opgeslagen
ui-launcher-popover-unencrypted = Onversleuteld opgeslagen
ui-launcher-popover-keyring_locked = Sleutelbos vergrendeld: ontgrendel deze om opgeslagen aanmeldingen te herstellen
ui-launcher-popover-forget = Verwijderen
ui-launcher-popover-sign_out = Afmelden
ui-launcher-popover-expires_in = Verloopt over { $duration }
ui-launcher-popover-expired = Sessie verlopen
ui-launcher-popover-expiry_unknown = Vervaldatum onbekend
ui-launcher-popover-make_default = Als standaard instellen
ui-launcher-popover-account_actions = Accountacties
ui-launcher-popover-play_anyway = Toch spelen
ui-launcher-popover-play_safe_mode = Spelen in veilige modus
ui-launcher-popover-add_account = Account toevoegen...
ui-launcher-popover-game_settings = Spelinstellingen...
ui-launcher-popover-installs = Installaties en instellen...
ui-launcher-popover-open_replay = Replay openen in spel...

## ui.launcher.palette
ui-launcher-palette-play = Spelen
ui-launcher-palette-play_as = Spelen als { $nickname } ({ $realm })
ui-launcher-palette-game_settings = Spelinstellingen
ui-launcher-palette-add_account = Account toevoegen
ui-launcher-palette-update = World of Warships bijwerken
ui-launcher-palette-open_replay = Replay openen in spel

## ui.launcher.installs
ui-launcher-installs-toolkit = Spelmap van de toolkit
ui-launcher-installs-played = Standaardaccount speelt deze
ui-launcher-installs-play_from = Vanaf hier spelen
ui-launcher-installs-update = Bijwerken

## ui.launcher.wgc_login
ui-launcher-wgc_login-label = Wargaming-aanmelding gebruiken
ui-launcher-wgc_login-about = Start deze Steam-installatie via de aanmelding van de toolkit in plaats van via Steam. Instellingen en replays worden apart gehouden van starts via Steam. Steam-portemonnee, prestaties en speeltijd werken niet in deze modus.
ui-launcher-wgc_login-steam_accounts = Accounts die via Steam zijn aangemaakt, hebben een e-mailadres en wachtwoord nodig. Voeg deze eerst toe in Wargaming Accountbeheer.
ui-launcher-wgc_login-steam_accounts_link = Een e-mailadres en wachtwoord toevoegen
ui-launcher-wgc_login-ready = Game Center-bestanden gereed
ui-launcher-wgc_login-fetchable = Inschakelen downloadt de Game Center-bestanden van WG voor deze build ({ $size }).
ui-launcher-wgc_login-downloading = Game Center-bestanden downloaden: { $percent }%
ui-launcher-wgc_login-downloading_unknown = Game Center-bestanden downloaden
ui-launcher-wgc_login-checking = Zoeken naar de Game Center-bestanden van deze build...
ui-launcher-wgc_login-needs_sign_in = Voeg eerst een Wargaming-account toe aan de toolkit.
ui-launcher-wgc_login-unavailable = Niet beschikbaar voor deze build: { $error }

## ui.launcher.replay
ui-launcher-replay-no_install = Geen installatie heeft spelbuild { $build }. Bewaar of installeer een kopie van die build om deze replay te bekijken.

## ui.launcher.setup
ui-launcher-setup-title = World of Warships instellen
ui-launcher-setup-game = Spel
ui-launcher-setup-proton = Proton
ui-launcher-setup-accounts = Accounts
ui-launcher-setup-replays = Replays
ui-launcher-setup-game_question = Welke kopie van het spel moet de toolkit gebruiken?
ui-launcher-setup-none_found = Er is geen installatie gevonden op deze computer.
ui-launcher-setup-choose_folder = Map kiezen...
ui-launcher-setup-install_new = Nieuwe kopie installeren...
ui-launcher-setup-proton_explain = Op Linux draait het spel via Proton: die van Steam als Steam is geïnstalleerd, anders umu-run.
ui-launcher-setup-proton_ready = Proton is gevonden.
ui-launcher-setup-accounts_explain = Meld u aan bij de Wargaming-accounts waarmee u speelt. Spelen meldt het spel aan met het standaardaccount.
ui-launcher-setup-replays_explain = Het spel verwijdert zijn oudste replays zodra het er zoveel bevat.
ui-launcher-setup-skip = Instellen overslaan
ui-launcher-setup-back = Terug
ui-launcher-setup-next = Volgende
ui-launcher-setup-done = Klaar

## ui.launcher.game_settings
ui-launcher-game_settings-title = Spelinstellingen
ui-launcher-game_settings-build = build { $build }
ui-launcher-game_settings-display = Weergave
ui-launcher-game_settings-graphics = Grafisch
ui-launcher-game_settings-sound = Geluid
ui-launcher-game_settings-replays = Replays
ui-launcher-game_settings-locked = Het spel is actief. Het herschrijft deze bestanden bij het afsluiten, dus ze zijn tot dan alleen-lezen.
ui-launcher-game_settings-footer = Wordt geschreven naar de voorkeursbestanden van het spel. Er wordt een back-up bewaard.
ui-launcher-game_settings-cancel = Annuleren
ui-launcher-game_settings-save = Opslaan
ui-launcher-game_settings-saved = Spelinstellingen opgeslagen.
ui-launcher-game_settings-no_install = Er is geen spelinstallatie ingesteld.
ui-launcher-game_settings-window_mode = Venstermodus
ui-launcher-game_settings-windowed = Venster
ui-launcher-game_settings-borderless = Randloos
ui-launcher-game_settings-fullscreen = Volledig scherm
ui-launcher-game_settings-monitor = Monitor
ui-launcher-game_settings-monitor_name = Monitornaam
ui-launcher-game_settings-resolution = Resolutie volledig scherm
ui-launcher-game_settings-width = Breedte
ui-launcher-game_settings-height = Hoogte
ui-launcher-game_settings-refresh = Verversingssnelheid
ui-launcher-game_settings-display_rate = Weergavesnelheid
ui-launcher-game_settings-vsync = Verticale synchronisatie
ui-launcher-game_settings-ui_scale = Interfaceschaal
ui-launcher-game_settings-battle_fps = Framerate-limiet in gevecht
ui-launcher-game_settings-port_fps = Framerate-limiet in haven
ui-launcher-game_settings-unlimited = Onbeperkt
ui-launcher-game_settings-skip_intro = Introvideo overslaan
ui-launcher-game_settings-master = Hoofdvolume
ui-launcher-game_settings-music_port = Muziek in haven
ui-launcher-game_settings-music_battle = Muziek in gevecht
ui-launcher-game_settings-preset = Voorinstelling
ui-launcher-game_settings-antialiasing = Anti-aliasing
ui-launcher-game_settings-textures = Textuurkwaliteit
ui-launcher-game_settings-filtering = Textuurfiltering
ui-launcher-game_settings-shadows = Schaduwkwaliteit
ui-launcher-game_settings-graphics_note = Wijzig de graphics in het spel. Het controleert ze aan de hand van uw hardware.
ui-launcher-game_settings-replays_kept = Te bewaren replays
ui-launcher-game_settings-replays_kept_zero = Bewaar ten minste één replay.
ui-launcher-game_settings-versioned = Replays sorteren op spelversie
ui-launcher-game_settings-replays_note = Wordt geschreven naar de res_mods van deze build. Veilige modus gebruikt dit niet.
ui-launcher-game_settings-absent = Niet ingesteld in de spelbestanden
ui-launcher-game_settings-unrecognized = Onbekende waarde: { $value }
ui-launcher-game_settings-not_a_number = { $field }: voer een getal in.

## ui.settings.launcher
ui-settings-launcher-installs = Spelinstallaties
ui-settings-launcher-installs_description = De installaties van het spel die de launcher start en bijwerkt.
ui-settings-launcher-runner = Proton (Linux)
ui-settings-launcher-runner_description = Hoe het spel op Linux draait. Proton Experimental of de nieuwste Proton van Steam wordt automatisch gevonden, anders umu-run.
ui-settings-launcher-launch_args = Startargumenten
ui-settings-launcher-launch_args_description = Extra opdrachtregelargumenten voor het spel, één per regel.
ui-settings-launcher-environment = Omgeving
ui-settings-launcher-environment_description = Omgevingsvariabelen voor het spel, KEY=VALUE, één per regel.
ui-settings-launcher-safe_mode_args = Argumenten voor veilige modus
ui-settings-launcher-safe_mode_args_description = Argumenten die worden toegevoegd wanneer u het spel in veilige modus speelt, één per regel.
