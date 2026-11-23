# You are encouraged to suggest edits to this file on GitHub by either filing
# an issue, editing the file directly and creating a pull request, or just
# mentioning it in the Discord.
## meta
meta-language_name = Italiano

# -- Dialogo selezione lingua --

## dialog
dialog-continue_in_english = Continua in inglese
dialog-continue_in_language = Continua in italiano
dialog-select_language = Seleziona lingua
dialog-machine_translation_warning = La maggior parte delle traduzioni è stata generata automaticamente e potrebbe essere imprecisa.

# -- Titoli schede --

## ui.tabs
ui-tabs-launcher = Launcher
ui-tabs-unpacker = Estrattore risorse
ui-tabs-settings = Impostazioni
ui-tabs-replay_parser = Ispettore replay
ui-tabs-replay_directory = Cartella replay
ui-tabs-player_tracker = Tracciamento giocatori
ui-tabs-mod_manager = Gestore mod
ui-tabs-armor_viewer = Visualizzatore corazza
ui-tabs-model_viewer = Visualizzatore modelli
ui-tabs-stats = Statistiche
ui-tabs-directory_stats = Statistiche { $dir }
ui-tabs-close_directory_stats = Chiudi statistiche cartella
ui-tabs-search = Cerca
ui-tabs-search_these_replays = Cerca in questi replay
ui-tabs-search_these_replays_unavailable = Questi replay non sono ancora stati indicizzati
ui-tabs-zoom_in_panel = Ingrandisci
ui-tabs-zoom_out_panel = Riduci
ui-tabs-close_panel = Chiudi
ui-tabs-close_tab = Chiudi
ui-tabs-close_other_tabs = Chiudi le altre
ui-tabs-close_tabs_to_the_right = Chiudi a destra

# -- Pulsanti comuni --

## ui.buttons
ui-buttons-yes = Sì
ui-buttons-no = No
ui-buttons-send_replays = Invia replay
ui-buttons-send_build_data = Invia solo dati build
ui-buttons-send_nothing = Non inviare nulla
ui-buttons-switch_to_replays = Passa ai replay
ui-buttons-keep_current = Mantieni impostazione attuale
ui-buttons-stop = Ferma
ui-buttons-copy = Copia
ui-buttons-cancel = Annulla
ui-buttons-continue_ = Continua
ui-buttons-install_update = Installa aggiornamento
ui-buttons-view_release = Visualizza release
ui-buttons-clear_settings = Cancella impostazioni
ui-buttons-more_options = Altre opzioni
ui-buttons-create_issue = Segnala problema
ui-buttons-discord = Discord
ui-buttons-github = GitHub
ui-buttons-view_github = Visualizza progetto su GitHub
ui-buttons-choose = Scegli...
ui-buttons-reset = Reimposta
ui-buttons-select_all = Seleziona tutto
ui-buttons-download = Scarica
ui-buttons-dismiss = Ignora
ui-buttons-retry = Riprova
ui-buttons-check_all = Seleziona tutto
ui-buttons-uncheck_all = Deseleziona tutto
ui-buttons-refresh_persisted_data_confirm = Aggiorna

# -- Voci di menu --

## ui.menu
ui-menu-file = File
ui-menu-check_updates = Controlla aggiornamenti
ui-menu-about = Informazioni
ui-menu-quit = Esci
ui-menu-licenses = Licenze

# -- Titoli finestre --

## ui.windows
ui-windows-build_consent = Consenso raccolta build
ui-windows-replay_migration = Passa alla condivisione dei replay
ui-windows-crash_detected = Crash dell'applicazione rilevato
ui-windows-update_available = Aggiornamento disponibile
ui-windows-confirm = Conferma
ui-windows-network_warning = Avviso di rete
ui-windows-download_game_data = Scarica dati di gioco
ui-windows-error = Errore
ui-windows-software_encode = Nessun encoder video GPU
ui-windows-about = Informazioni
ui-windows-refresh_persisted_data = Aggiorna dati replay salvati
ui-windows-tactics_board = Tavola tattica
ui-windows-minimize = Riduci a icona
ui-windows-maximize = Ingrandisci
ui-windows-restore = Ripristina
ui-windows-close = Chiudi
ui-windows-licenses = Licenze

# -- Testo dialoghi / messaggi --

## ui.dialogs
ui-dialogs-build_consent_message = Desideri inviare le informazioni sulle build dei giocatori dalle battaglie classificate e casuali allo sviluppatore? Questa raccolta dati aiuta la comunità ad analizzare in blocco le build dei giocatori. Puoi disattivare questa opzione in qualsiasi momento nelle impostazioni.
ui-dialogs-replay_migration_message = Toolkit può ora condividere i replay completi, il che consente un'analisi del matchmaking molto più ricca rispetto ai soli dati delle build. Le battaglie in cui giochi con una nave test non vengono condivise. Vuoi passare alla condivisione dei replay grezzi?
ui-dialogs-crash_message = Sembra che WoWs Toolkit sia andato in crash durante l'ultima esecuzione. Se desideri segnalare questo problema, pubblica un issue su GitHub o unisciti al server Discord e fornisci le informazioni seguenti.
ui-dialogs-crash_clear_settings = Se per qualche motivo i dati persistenti dell'applicazione potrebbero essere responsabili, puoi provare a cancellare le impostazioni premendo il pulsante qui sotto. Questo cancellerà tutte le impostazioni. I giocatori tracciati vengono mantenuti. I tuoi replay e i dati di WoWs saranno al sicuro.
ui-dialogs-update_message = La versione { $tag } di WoWs Toolkit è disponibile
ui-dialogs-update_notes = Note di rilascio
ui-dialogs-update_asset_pending = Il download per Windows non è ancora disponibile. Consulta la release per lo stato della build.
ui-dialogs-update_windows_only = Aggiornamento disponibile, ma al momento è supportato solo Windows.
ui-dialogs-p2p_warning = Le sessioni collaborative utilizzano la rete P2P. Altri utenti nella sessione potrebbero essere in grado di vedere il tuo indirizzo IP.
ui-dialogs-software_encode_message = Non è stato trovato alcun encoder video GPU supportato, quindi questa esportazione verrà codificata via software. È molto più lento e una battaglia lunga può richiedere diversi minuti.
ui-dialogs-download_game_data_intro = Alcuni replay richiedono dati di gioco non installati. Scegli le build da scaricare.
ui-dialogs-download_build_row = Versione { $version } (build { $build })
ui-dialogs-download_replays_needing = richiesta da { $count } replay
ui-dialogs-download_availability_resolving = controllo del repository
ui-dialogs-download_availability_exact = pubblicata
ui-dialogs-download_availability_nearest = nessuna corrispondenza esatta, la più vicina è { $version } (build { $build })
ui-dialogs-download_availability_unpublished = mai pubblicata
ui-dialogs-download_availability_unreachable = pubblicata, ma non è stato possibile leggerne l'elenco dei file
ui-dialogs-download_plan_pending = Calcolo di cosa serve scaricare...
ui-dialogs-download_objects_to_fetch = Oggetti da scaricare per le build selezionate: { $count }
ui-dialogs-download_plan_failed = Impossibile determinare cosa serve scaricare
ui-dialogs-download_plan_no_cache_dir = Nessuna cartella di cache dei dati di gioco impostata. Scegline una nelle Impostazioni, poi riprova.
ui-dialogs-refresh_persisted_data_message = Rianalizza ogni replay nella cartella dei replay e riscrive i dati salvati. Le voci registrate con una mappatura dati replay che non corrisponde alla loro build di gioco restano invariate, tranne che Personal Rating e modalità di gioco vengono ricalcolati per ogni replay in base ai valori attesi attuali. L'operazione può richiedere un po' di tempo.
ui-dialogs-file_dialog_failed = Impossibile aprire la finestra di selezione file: { $reason }

# -- Etichette --

## ui.labels
ui-labels-error_occurred = Si è verificato un errore:
ui-labels-made_by = Creato da landaire.
ui-labels-credits = Grazie a Trackpad, TTaro, lkolbly per i loro contributi.
ui-labels-pr_credits = Dati e formula per il calcolo del Personal Rating (PR) forniti da WoWs Numbers.
ui-labels-powered_by = Basato su{" "}
ui-labels-and = {" "}e{" "}
ui-labels-suppress_warning = Non mostrare più
ui-labels-more_info = Maggiori informazioni
ui-labels-loading_replays = Caricamento di { $count } replay...
ui-labels-app_version = Versione { $version }
ui-labels-project_license = WoWs Toolkit è distribuito con la licenza { $license }.
ui-labels-third_party_notices = Avvisi di terze parti
ui-labels-notices_search_hint = Cerca negli avvisi
ui-labels-no_matching_notices = Nessun avviso corrisponde alla ricerca.

# -- Messaggi toast --

## ui.messages
ui-messages-bug_report_info_copied = Informazioni per la segnalazione di bug copiate.
ui-messages-update_downloading = Download dell'aggiornamento: { $read } di { $total }
ui-messages-update_downloading_unknown = Download dell'aggiornamento: { $read } finora
ui-messages-app_up_to_date = Applicazione aggiornata
ui-messages-checking_app_updates = Controllo aggiornamenti dell'app...
ui-messages-update_starting = Download dell'aggiornamento...
ui-messages-update_check_failed = Impossibile verificare gli aggiornamenti dell'app
ui-messages-constants_fetch_failed = Impossibile recuperare la mappatura dati replay aggiornata. Riproverà più tardi.
ui-messages-game_data_loaded = Dati di gioco caricati con successo
ui-messages-no_replays_detected = Nessun replay trovato — la cartella di WoWs è configurata correttamente?
ui-messages-build_loaded = Build { $build } caricata
ui-messages-downloading_game_data = Download dei dati di gioco...
ui-messages-game_data_downloaded = Dati di gioco scaricati per la build { $build }
ui-messages-game_data_builds_downloaded = Scaricati i dati di gioco per { $count } build
ui-messages-game_data_download_failed = Impossibile scaricare i dati di gioco
ui-messages-game_data_build_download_failed = Impossibile scaricare i dati di gioco per la build { $build }
ui-messages-checking_game_data_updates = Controllo aggiornamenti dei dati di gioco...
ui-messages-game_data_up_to_date = I dati di gioco sono aggiornati
ui-messages-game_data_updates_available = { $count } build in cache hanno dati più recenti disponibili
ui-messages-validating_game_data = Verifica della cache dei dati di gioco...
ui-messages-game_data_cache_valid = La cache dei dati di gioco è valida e corrisponde al repository remoto
ui-messages-game_data_cache_invalid = { $count } build in cache sono obsolete o corrotte
ui-messages-replay_loaded = Replay caricato con successo
ui-messages-replay_load_failed = Impossibile caricare il replay
ui-messages-twitch_token_invalid = Il token Twitch non è valido o è scaduto. Aggiornalo nelle Impostazioni.
ui-messages-wows_dir_invalid = La cartella di World of Warships non è valida. Controlla il percorso nelle Impostazioni.
ui-messages-wows_dir_not_set = La cartella di World of Warships non è impostata
ui-messages-twitch_token_expired = Il token Twitch non è valido o è scaduto
ui-messages-versioned_constants_rebuild_failed = Impossibile recuperare le costanti versionali durante la ricostruzione. Riproverà più tardi.
ui-messages-constants_version_mismatch =
    La versione del file di mappatura dati replay non corrisponde alla versione del gioco.
    I risultati post-battaglia potrebbero non essere accurati. Attendi con pazienza mentre i manutentori del progetto aggiornano la mappatura sul server.
ui-messages-constants_updated = File di mappatura dati replay aggiornato con successo
ui-messages-constants_imported = Installata la mappatura dati replay per la build { $build }
ui-messages-constants_imported_other_build = Installata la mappatura dati replay per la build { $build }, che non è la build attualmente caricata ({ $loaded })
ui-messages-constants_import_failed = Impossibile importare la mappatura dati replay: { $error }
ui-messages-session_started = Sessione avviata
ui-messages-user_joined = { $name } si è unito
ui-messages-user_timeout = { $name } è andato in timeout
ui-messages-user_left = { $name } è uscito
ui-messages-session_ended = Sessione terminata
ui-messages-session_error = Errore sessione: { $msg }
ui-messages-connected_to_session = Connesso alla sessione
ui-messages-joined_session = Sessione unita: { $name }
ui-messages-replay_spam_protection = Disconnesso: l'host sta aprendo i replay troppo velocemente
ui-messages-host_opened_replay = L'host ha aperto un replay: { $name }
ui-messages-host_closed_replay = L'host ha chiuso un replay
ui-messages-session_error_generic = Sessione: { $msg }
ui-messages-session_rejected = Sessione rifiutata: { $reason }
ui-messages-indexing_replays = Indicizzazione dei replay...
ui-messages-indexing_replays_progress = Indicizzazione dei replay ({ $done }/{ $total })
ui-messages-replays_indexed = Indicizzati { $indexed } replay su { $total }
ui-messages-replays_already_indexed = Tutti i { $total } replay sono già indicizzati
ui-messages-loading_row_summaries = Caricamento dei dettagli dei replay...
ui-messages-reading_replay_directory = Lettura della cartella replay...
ui-messages-directory_replays_skipped = Saltati { $skipped } replay che non è stato possibile leggere
ui-messages-drop_not_a_replay = Non è un file replay
ui-messages-drop_not_replays = Questi non sono file replay
ui-messages-drop_skipped = Saltati { $count } file rilasciati che non sono replay né cartelle
ui-messages-directory_replays_not_indexed = Elencati { $count } replay senza i dettagli della partita
ui-messages-log_copied = Copiati { $bytes } KB da { $file } negli appunti
ui-messages-log_copy_failed = Impossibile leggere un file di log: { $error }
ui-messages-log_not_found = Nessun file di log trovato in { $dir }

# -- Nomi statistiche --

## stat
stat-damage = Danni
stat-spotting_damage = Danni da avvistamento
stat-frags = Eliminazioni
stat-raw_xp = XP grezzo
stat-base_xp = XP base
stat-win_rate = Percentuale vittorie
stat-win_rate_pct = Percentuale vittorie (%)
stat-personal_rating = Valutazione personale
stat-match_contribution = Contributo alla battaglia
stat-capture_points = Punti di conquista
stat-planes_shot_down = Aerei abbattuti
stat-anti_air_damage = Danni antiaerei
stat-survival_rate = Tasso di sopravvivenza
stat-torpedoes_spotted = Siluri individuati
stat-avg_damage = Danni medi
stat-avg_spotting_damage = Danni da avvistamento medi
stat-avg_frags = Eliminazioni medie
stat-avg_raw_xp = XP grezzo medio
stat-avg_base_xp = XP base medio
stat-avg_pr = PR medio
stat-avg_mc = MC medio
stat-avg_prefix = { $name } medio

## chart
chart-game_number = Partita #
chart-combined = Combinato
chart-combined_suffix = (Combinato)
chart-rolling_average_suffix = (Media mobile)
chart-no_data = Nessun dato disponibile per le navi selezionate
chart-win_rate_unavailable = La percentuale vittorie non è disponibile per i grafici a linee per partita (abilita Media mobile o usa il grafico a barre)

# -- Dialoghi di conferma --

## confirm
confirm-open_in_game = Questo avvierà World of Warships. Continuare?
confirm-clear_all_session_stats = Questo cancellerà tutte le statistiche di sessione. Continuare?
confirm-clear_ship_session_stats = Questo rimuoverà tutte le partite per questa nave dalle statistiche di sessione. Continuare?
confirm-set_as_session_stats = Questo sostituirà le tue statistiche di sessione attuali. Continuare?

# -- Messaggi di errore --

## error
error-file_watcher_creation = Impossibile creare il file watcher: { $error }
error-replay_dir_watch = Impossibile monitorare la cartella replay: { $error }

# -- Impostazioni --

## ui.settings.app
ui-settings-app-heading = Impostazioni applicazione
ui-settings-app-description = Comportamento e aspetto generale dell'applicazione
ui-settings-app-check_for_updates = Controlla aggiornamenti all'avvio
ui-settings-app-enable_logging = Abilita registrazione (effetto al riavvio)
ui-settings-app-enable_logging_tooltip = Scrivi i log di debug in un file nella cartella dati dell'applicazione. Utile per la risoluzione dei problemi.
ui-settings-app-data_sharing_mode = Condivisione dati (battaglie classificate e casuali)
ui-settings-app-data_sharing_off = Non inviare nulla
ui-settings-app-data_sharing_build_data = Invia i dati delle build a ShipBuilds.com
ui-settings-app-data_sharing_replays = Invia i replay a ShipBuilds.com
ui-settings-app-zoom_factor = Fattore di zoom (anche Ctrl + e Ctrl - lo modificano)
ui-settings-app-theme = Tema
ui-settings-app-theme_system = Sistema
ui-settings-app-theme_dark = Scuro
ui-settings-app-theme_light = Chiaro
ui-settings-app-code_integrity = Blocca codice non firmato (effetto al riavvio)
ui-settings-app-code_integrity_tooltip = Rifiuta di caricare codice non firmato da Microsoft, tenendo fuori da questa applicazione overlay, strumenti di cattura e utility audio. Sono una causa comune di crash e blocchi durante il rendering. I driver grafici sono firmati tramite Windows e continuano a funzionare. Non può essere disattivato una volta avviata la sessione, quindi una modifica ha effetto al prossimo avvio. La modalità automatica lo salta quando hai installato un metodo di input di terze parti, che altrimenti verrebbe bloccato. Nota che le altre protezioni restano attive indipendentemente da questa impostazione; esegui con --no-hardening per disattivarle tutte.
ui-settings-app-code_integrity_automatic = Automatico
ui-settings-app-code_integrity_always = Sempre
ui-settings-app-code_integrity_never = Mai
ui-settings-app-language = Lingua
ui-settings-app-open_data_dir = Apri cartella dati applicazione
ui-settings-app-open_data_dir_tooltip = Apri la cartella dati dell'applicazione (impostazioni, log, dati in cache) nel tuo gestore file.
ui-settings-app-proxy_url = Proxy HTTP
ui-settings-app-proxy_url_hint = host:porta, oppure lascia vuoto per usare le impostazioni proxy di sistema (effettivo al riavvio)
ui-settings-app-proxy_url_hint_live = host:porta, oppure lascia vuoto per usare le impostazioni proxy di sistema

## ui.settings.wows
ui-settings-wows-heading = Impostazioni World of Warships
ui-settings-wows-description = Percorso dell'installazione di World of Warships
ui-settings-wows-directory_hint = Cartella di World of Warships
ui-settings-wows-browse = Sfoglia...

## ui.settings.wows.cache
ui-settings-wows-cache-heading = Cache dei dati di gioco
ui-settings-wows-cache-description = Dati di gioco estratti, per poter aprire anche i replay di build precedenti
ui-settings-wows-cache-auto_dump = Memorizza i dati di gioco per la compatibilità con i vecchi replay
ui-settings-wows-cache-directory_label = Directory cache:
ui-settings-wows-cache-browse = Sfoglia
ui-settings-wows-cache-open_folder = Apri cartella
ui-settings-wows-cache-delete_old = Elimina vecchie versioni
ui-settings-wows-cache-stats = Cache dati di gioco: { $size } in { $count } versione/i
ui-settings-wows-cache-check_updates = Controlla aggiornamenti
ui-settings-wows-cache-actions = Azioni cache
ui-settings-wows-cache-actions_tooltip = Controlla aggiornamenti o verifica le build in cache
ui-settings-wows-cache-update_all = Aggiorna tutto
ui-settings-wows-cache-updates_available = { $count } build in cache hanno dati più recenti disponibili
ui-settings-wows-cache-validate = Verifica cache
ui-settings-wows-cache-validate_tooltip = Controlla ogni build in cache confrontandola con il repository remoto e verifica l'integrità dei file
ui-settings-wows-cache-repair = Ripara
ui-settings-wows-cache-repair_needed = { $count } build in cache sono obsolete o corrotte

## ui.task
ui-task-batch_render_progress = Rendering { $current }/{ $total }: { $name }

## ui.settings.replay
ui-settings-replay-heading = Impostazioni replay
ui-settings-replay-description = Configura quali colonne appaiono nella tabella dei risultati replay
ui-settings-replay-show_raw_xp = Mostra XP grezzo
ui-settings-replay-show_entity_id = Mostra ID entità
ui-settings-replay-show_observed_damage = Mostra danni osservati
ui-settings-replay-enable_previews = Mostra anteprima minimappa al passaggio del mouse
ui-settings-replay-enable_previews_tooltip = Passando il mouse su un replay, questo viene letto e riprodotto in background per creare un'anteprima della minimappa in loop. Disattivalo se usi un'archiviazione lenta.
ui-settings-replay-show_fires = Mostra incendi
ui-settings-replay-show_floods = Mostra allagamenti
ui-settings-replay-show_citadels = Mostra cittadelle
ui-settings-replay-show_crits = Mostra colpi critici ai moduli
ui-settings-replay-auto_export_data = Esportazione automatica dati
ui-settings-replay-format_json = JSON
ui-settings-replay-format_csv = CSV
ui-settings-replay-format_cbor = CBOR
ui-settings-replay-export_path_hint = Cartella di esportazione automatica dati
ui-settings-replay-export_path_missing = La cartella non esiste. Non è possibile esportarvi dati.
ui-settings-replay-index_all_replays = Indicizza tutti i replay
ui-settings-replay-reindex_all_replays = Reindicizza tutti i replay
ui-settings-replay-reindex_all_replays_hover = Rianalizza e reindicizza ogni replay, inclusi quelli già indicizzati. Usalo dopo un aggiornamento che aggiunge nuovi dati all'indice (ad es. Personal Rating, stato di disconnessione) per applicarli anche alle voci esistenti.
ui-settings-replay-show_heals = Mostra cure
ui-settings-replay-current_path = Percorso replay attuale

## ui.settings.session
ui-settings-session-heading = Impostazioni sessione
ui-settings-session-description = Preferenze sessione collaborativa dei replay
ui-settings-session-display_name = Nome visualizzato
ui-settings-session-suppress_ip_warning = Non mostrare l'avviso sull'indirizzo IP P2P
ui-settings-session-ip_warning_tooltip = Le sessioni collaborative utilizzano la rete P2P. Altri utenti nella sessione potrebbero essere in grado di vedere il tuo indirizzo IP.
ui-settings-session-disable_auto_open = Disabilita apertura automatica delle finestre di sessione
ui-settings-session-auto_open_tooltip = Quando abilitato, le finestre di sessione (replay, tavole tattiche) non si apriranno automaticamente. Puoi comunque aprirle dal popover della sessione.

## ui.settings.twitch
ui-settings-twitch-heading = Impostazioni Twitch
ui-settings-twitch-description = Connettiti a Twitch per l'integrazione chat e il tracciamento giocatori
ui-settings-twitch-get_token = Ottieni token di accesso
ui-settings-twitch-get_token_tooltip = Utilizziamo la pagina di login di Chatterino perché fornisce un token con le autorizzazioni necessarie (essenzialmente un token moderatore con permessi di chat), eliminando la necessità per lo sviluppatore di WoWs Toolkit di ospitare una propria pagina di login che avrebbe lo stesso risultato.
ui-settings-twitch-paste_token_no_token = Incolla token (nessun token impostato)
ui-settings-twitch-paste_token_tooltip = Legge la credenziale dagli appunti
ui-settings-twitch-paste_token_valid = Incolla token (token valido)
ui-settings-twitch-paste_token_unvalidated = Verifica del token
ui-settings-twitch-paste_token_invalid = Incolla token (token non valido / scaduto)
ui-settings-twitch-monitored_channel = Canale monitorato (predefinito: sé stesso)
ui-settings-twitch-signed_in_as = Accesso effettuato come { $who }

# -- Ispettore replay --

## ui.replay
ui-replay-copy_bug_report_info = Copia informazioni per la segnalazione di bug
ui-replay-collapse_listing = Comprimi elenco replay
ui-replay-expand_listing = Espandi elenco replay
ui-replay-match_age = { $age } fa
ui-replay-listing_caption = Replay
ui-replay-listing_reading = Lettura di { $done } su { $total }...
ui-replay-listing_scanning = Scansione di { $done } su { $total }...
ui-replay-listing_downloading = Download dei dati di gioco, { $done } file su { $total }...
ui-replay-listing_loading_data = Caricamento dati di gioco per { $version } ({ $index } di { $count })...
ui-replay-loading_game_data_version = Caricamento dati di gioco per { $version }...
ui-replay-spectator = Spettatore
ui-replay-loading = Caricamento...
ui-replay-preview_loading = Creazione anteprima...
ui-replay-preview_empty = Nessun movimento da mostrare
ui-replay-preview_no_game_data = Nessun dato di gioco per la versione client { $value }
ui-replay-preview_unreadable = Impossibile leggere questo replay
ui-replay-preview_unreadable_because = Impossibile leggere questo replay: { $value }
ui-replay-preview_unwalkable = Impossibile ricostruire la battaglia di questo replay: { $value }
ui-replay-preview_unknown_version = Questo replay riporta una versione client illeggibile: { $value }
ui-replay-preview_no_map = Nessun dato mappa per { $value }
ui-replay-preview_cancelled = Anteprima sostituita da una più recente
ui-replay-preview_render_failed = Impossibile disegnare l'anteprima: { $value }
ui-replay-no_selection = Clicca su un replay per visualizzarlo o fai doppio clic per aprirlo in una nuova scheda
ui-replay-workspace_closed = Questo spazio di lavoro replay è stato chiuso
ui-replay-incomplete_results = Risultati partita incompleti
ui-replay-incomplete_results_tooltip = Il replay non ha ancora i risultati di fine partita. I dati verranno ricaricati automaticamente quando la partita termina e i risultati vengono aggiunti al replay.
ui-replay-no_chat = Nessun messaggio in chat è stato inviato in questo replay
ui-replay-chat_copy_all = Copia tutto
ui-replay-chat_save_to_file = Salva su file
ui-replay-chat_save_title = Salva chat di gioco
ui-replay-export_results_title = Esporta risultati
ui-replay-results_exported = Risultati esportati
ui-replay-chat_saved = Chat salvata in { $path }
ui-replay-chat_save_failed = Chat non salvata: { $error }
ui-replay-team_damage = Danni alla squadra:{" "}
ui-replay-team_share = { $pct } del totale della squadra ({ $team })
ui-replay-hide_my_stats = Nascondi le statistiche delle mie navi test
ui-replay-open_manually = Apri file replay manualmente...
ui-replay-open_directory = Apri cartella replay...
ui-replay-search_these_replays = Cerca in questi replay
ui-replay-reading_directory = Elenco dei replay in { $dir }
ui-replay-import_constants = Importa file constants.json...
ui-replay-refresh_persisted_data = Aggiorna dati replay salvati...
ui-replay-contributing = Contributo di { $done } replay su { $total }
ui-replay-contributed = Inviati { $sent } replay su { $total }
ui-replay-contribute_nothing_shared = Scegli prima cosa condividere nelle Impostazioni
ui-replay-contribute_none_listed = Nessun replay elencato da inviare
ui-replay-contribute_not_ready = I replay potranno essere inviati dopo il caricamento dei dati di gioco e delle impostazioni
ui-replay-contribute_already_running = Un invio è già in corso
ui-replay-auto_export_failed = Impossibile esportare una battaglia: { $error }
ui-replay-auto_exported = Battaglia esportata in { $file }
ui-replay-opened_new_battle = Aperta la nuova battaglia: { $battle }
ui-replay-new_battle_listed = Nuova battaglia: { $battle }
ui-replay-results_arrived = Sono arrivati i risultati della battaglia { $battle }
ui-replay-constants_written = Scaricate le mappature dei risultati per { $count } build
ui-replay-constants_import_title = Importa un constants.json
ui-replay-constants_imported = Importata la mappatura dei risultati per la build { $build }
ui-replay-constants_import_failed = Mappatura non importata: { $reason }
ui-replay-constants_no_build = Carica prima una build di gioco
ui-replay-constants_latest_written = Scaricata la mappatura dei risultati più recente
ui-replay-constants_check_failed = Impossibile controllare le mappature dei risultati: { $reason }
ui-replay-constants_build_label = build { $build }
ui-replay-constants_generating = Aggiornamento delle costanti di gioco per { $version }...
ui-replay-constants_generated = Costanti di gioco per { $version } generate in { $seconds } s
ui-replay-constants_generation_failed = Impossibile aggiornare le costanti di gioco per { $version }: { $reason }. { $fallback }
ui-replay-constants_fallback_kept = Vengono mantenute le costanti già salvate.
ui-replay-constants_fallback_published = Vengono usate le costanti pubblicate per la build { $build }.
ui-replay-constants_fallback_embedded = Vengono usate le costanti integrate.
ui-replay-copy_latest_log = Copia ultimo log negli appunti
ui-replay-load_alt_perspective = Carica prospettiva dell'altra squadra...
ui-replay-load_alt_perspective_tooltip = Unisce al replay attuale la registrazione della stessa partita fatta da un altro giocatore. Rivela posizioni, build, siluri, artiglieria e consumabili dei nemici dall'altra prospettiva.
ui-replay-load_alt_perspective_disabled_tooltip = Apri o seleziona prima un replay.
ui-replay-load_alt_perspective_failed = Impossibile unire il replay: { $error }
ui-replay-load_alt_perspective_limit = Questa battaglia viene già letta tramite { $limit } altre registrazioni
ui-replay-consumable_charges_remaining = { $remaining } rimanenti su { $total } cariche
ui-replay-consumable_charges_unlimited = { $used } usate (cariche illimitate)
ui-replay-consumable_header_consumable = Consumabile
ui-replay-consumable_header_remaining = Rimanenti
ui-replay-consumable_header_total = Totale
ui-replay-consumable_charges_infinite = ∞
ui-replay-autoload_latest = Carica automaticamente l'ultimo replay
ui-replay-autoload_latest_tooltip = Apri una partita non appena il gioco finisce di scriverla
ui-replay-anonymize_names = Anonimizza i nomi dei giocatori
ui-replay-anonymize_names_tooltip = Mostra nomi generati al posto dei nomi dei giocatori e dei tag di clan. Le esportazioni mantengono i nomi reali.
ui-replay-group_win_rate_all_modes = Conta tutte le modalità di gioco nella % vittorie dei gruppi
ui-replay-group_win_rate_all_modes_tooltip = Per impostazione predefinita, le intestazioni dei gruppi contano solo le battaglie casuali e classificate. Se attivo, vengono contate tutte le modalità di gioco e la % vittorie non viene colorata, perché i colori sono calibrati per le battaglie casuali.
ui-replay-select_replay = Seleziona un replay
ui-replay-drop_hint = Rilascia qui un replay per aprirlo, o una cartella per elencarne i replay
ui-replay-scanning = Scansione dei replay...
ui-replay-scanning_count = Lettura replay { $done } di { $total }
ui-replay-no_replays_in = Nessun replay in { $folder }
ui-replay-drop_folder_hint = Rilascia una cartella sulla finestra per elencarla in una scheda dedicata
ui-replay-new_tag = Nuovo
ui-replay-loading_game_data = Caricamento dati di gioco...
ui-replay-game_data_failed = Impossibile caricare i dati di gioco: { $error }
ui-replay-group_label = Raggruppamento replay
ui-replay-column_filters = Filtri colonne
ui-replay-row_elements = Elementi della riga
ui-replay-row_element_personal_rating = Valutazione personale
ui-replay-row_element_match_contribution = Contributo alla battaglia
ui-replay-row_element_division = Divisione
ui-replay-row_element_twitch = Chat di Twitch
ui-replay-row_element_damage = Danni
ui-replay-row_element_kills = Eliminazioni
ui-replay-row_element_time = Ora della battaglia
ui-replay-listing_options = Opzioni elenco
ui-replay-open = Apri
ui-replay-open_file = Apri file replay...
ui-replay-check_game_data = Controlla dati di gioco
ui-replay-check_game_data_tooltip = Controlla i dati di gioco richiesti dai replay in questa cartella
ui-replay-close_side_panel = Chiudi pannello
ui-replay-nda = NDA
ui-replay-render = Rendering
ui-replay-chat = Chat
ui-replay-game_chat = Chat di gioco
ui-replay-copy_all = Copia tutto
ui-replay-save_to_file = Salva su file
ui-replay-chat_copied = Chat copiata
ui-replay-copy_message = Copia messaggio
ui-replay-actions = Azioni
ui-replay-row_actions_hint = Azioni giocatore (o clic destro sulla riga)
ui-replay-section_match = Partita
ui-replay-section_export = Esporta
ui-replay-section_debug = Debug
ui-replay-timeline = Cronologià partita
ui-replay-row_not_indexed = non indicizzato
ui-replay-hover_survived = Sopravvissuto
ui-replay-hover_sunk = Affondato
ui-replay-hover_not_indexed = Non indicizzato
ui-replay-row_kills = { $count } eliminazioni
ui-replay-row_survived = sopravvissuto
ui-replay-row_sunk = affondato
ui-replay-row_division = Divisione: { $members }
ui-replay-export_chat_save = Chat: salva su file
ui-replay-export_chat_copy = Chat: copia
ui-replay-export_results_json = Risultati: JSON
ui-replay-export_results_cbor = Risultati: CBOR
ui-replay-export_results_csv = Risultati: CSV
ui-replay-timeline_filter = Filtro
ui-replay-timeline_filter_all = Tutti
ui-replay-timeline_filter_none = Nessuno
ui-replay-timeline_search_hint = Cerca nave o giocatore
ui-replay-timeline_no_events = Nessun evento significativo rilevato.
ui-replay-timeline_no_matches = Nessun evento corrisponde al filtro attuale.
ui-replay-timeline_parsing = Analisi degli eventi...
ui-replay-timeline_failed = Impossibile estrarre la cronologià della partita: { $error }
ui-replay-timeline_retry = Riprova
ui-replay-timeline_export_tooltip = Si applica agli eventi attualmente visibili dopo il filtraggio.
ui-replay-timeline_copied = Cronologià copiata
ui-replay-timeline_event_copied = Evento copiato
ui-replay-timeline_saved = Cronologia salvata in { $path }
ui-replay-timeline_save_failed = La cronologia non è stata salvata: { $error }
ui-replay-timeline_kind_health_lost = Danni pesanti
ui-replay-timeline_kind_death = Distrutta
ui-replay-timeline_kind_cap_contested = Punto conteso
ui-replay-timeline_kind_cap_flipped = Punto catturato
ui-replay-timeline_kind_cap_being_captured = Punto in cattura
ui-replay-timeline_kind_radar = Radar usato
ui-replay-timeline_kind_advantage = Cambio di vantaggio
ui-replay-timeline_kind_disconnected = Disconnesso

## ui.replay.context
ui-replay-context-open_in_new_tab = Apri in nuova scheda
ui-replay-context-copy_cell = Copia cella
ui-replay-context-copy_row = Copia riga
ui-replay-context-copy_table = Copia tabella
ui-replay-context-pin_column = Fissa colonna
ui-replay-context-unpin_column = Sblocca colonna
ui-replay-context-copied = Copiato
ui-replay-context-copy_path = Copia percorso
ui-replay-context-copy_replay = Copia replay
ui-replay-context-copy_replays = Copia { $count } replay
ui-replay-context-copy_paths = Copia { $count } percorsi
ui-replay-context-show_in_explorer = Mostra in Esplora file
ui-replay-context-show_replay_controls = Mostra controlli replay
ui-replay-context-open_in_game = Apri nel gioco
ui-replay-context-render_replay = Rendering replay
ui-replay-context-set_session_stats_one = Imposta come statistiche sessione (1 replay)
ui-replay-context-set_session_stats_many = Imposta come statistiche sessione ({ $count } replay)
ui-replay-context-add_session_stats_one = Aggiungi alle statistiche sessione (1 replay)
ui-replay-context-add_session_stats_many = Aggiungi alle statistiche sessione ({ $count } replay)
ui-replay-context-render_to_video = Esporta come video
ui-replay-context-render_to_video_many = Esporta { $count } replay come video
ui-replay-context-render_to_clipboard = Esporta negli appunti
ui-replay-context-render_to_clipboard_many = Esporta { $count } replay negli appunti

## ui.replay.column
ui-replay-column-actions = Azioni
ui-replay-column-player_name = Nome giocatore
ui-replay-column-base_xp = XP base
ui-replay-column-raw_xp = XP grezzo
ui-replay-column-ship_name = Nome nave
ui-replay-column-hits = Colpi
ui-replay-column-heals = Riparazioni
ui-replay-column-heals_tooltip = Numero di attivazioni della Squadra di riparazione osservate per questo giocatore. Puo essere impreciso per le navi non visualizzate a schermo (cioè navi nemiche mai avvistate).
ui-replay-column-heals_no_repair_tooltip = Questa nave non dispone del consumabile Squadra di riparazione.
ui-replay-column-kills = Eliminazioni
ui-replay-column-observed_damage = Danni osservati
ui-replay-column-actual_damage = Danni effettivi
ui-replay-column-spotting_damage = Danni da avvistamento
ui-replay-column-potential_damage = Danni potenziali
ui-replay-column-time_lived = Tempo di vita
ui-replay-column-fires = Incendi
ui-replay-column-floods = Allagamenti
ui-replay-column-citadels = Cittadelle
ui-replay-column-crits = Critici
ui-replay-column-received_damage = Danni ricevuti
ui-replay-column-distance_traveled = Distanza percorsa
ui-replay-column-skills = Abilità
ui-replay-column-personal_rating = PR
ui-replay-column-match_contribution = MC

## ui.rating
ui-rating-personal_rating = Valutazione personale
ui-rating-match_contribution = Contributo alla battaglia
ui-rating-versus_average = rispetto alla media
ui-rating-average = Media
ui-rating-damage = Danni
ui-rating-kills = Eliminazioni
ui-rating-wins = Vittoria
ui-rating-team_damage_share = Quota dei danni della squadra
ui-rating-tanking = Fuoco attirato
ui-rating-spotting = Avvistamento
ui-rating-objectives = Obiettivi
ui-rating-anti_air = Antiaerea
ui-rating-torpedo_warnings = Avvisi siluri
ui-rating-critical_bursts = Raffiche critiche
ui-rating-unrated_early_exit = Nessun MC: la registrazione è terminata prima della battaglia
ui-rating-mc_unrated_battle_type = Nessun MC: il contributo alla battaglia vale solo per battaglie casuali e classificate

## ui.rating.detail
ui-rating-detail-versus_expected = { $actual } contro { $expected } attesi
ui-rating-detail-wins = { $actual } vinte contro { $expected } attese
ui-rating-detail-weighted_damage = { $actual } contro { $expected } attesi, danni a cacciatorpediniere e sottomarini x1.2
ui-rating-detail-team_damage_share = { $share }% dei danni della squadra ai nemici, { $even }% è una quota equa
ui-rating-detail-tanking = { $potential } danni potenziali, { $class }x la media della classe, { $team }x una quota equa di quelli della squadra
ui-rating-detail-spotting = { $damage } danni da avvistamento (media della classe { $baseline }), { $ships } PV di navi avvistate (media { $ships_baseline })
ui-rating-detail-objectives = Punti: { $capture } conquistando, { $block } bloccando, { $reset } azzerando, { $battle_end } mantenuti alla fine, { $hold } mantenendo (contati a un quarto)
ui-rating-detail-anti_air_planes = { $strike } aerei d'attacco, { $support } caccia e ricognitori
ui-rating-detail-anti_air_damage = { $damage } danni AA
ui-rating-detail-torpedo_warnings = { $threatening } siluri pericolosi avvistati
ui-rating-detail-torpedo_warnings_one = 1 siluro pericoloso avvistato
ui-rating-detail-critical_bursts = { $count } raffiche critiche: { $bursts }. Visibili solo per le navi vicine al giocatore che registra
ui-rating-detail-critical_bursts_one = 1 raffica critica: { $bursts }. Visibile solo per le navi vicine al giocatore che registra
ui-rating-detail-critical_burst = { $share }% di { $ship } in { $seconds } s
ui-rating-detail-critical_burst_at_once = { $share }% di { $ship } in un colpo

## ui.personal_rating
ui-personal_rating-unrated_hover = La valutazione personale si applica solo alle battaglie casuali e classificate.

## ui.rating.basis
ui-rating-basis-static = Statico
ui-rating-basis-dynamic = Dinamico
ui-rating-basis-static_explained = Statico: misurato rispetto a una costante fissa o a un riferimento di classe
ui-rating-basis-dynamic_explained = Dinamico: misurato rispetto ai valori attesi di wows-numbers o agli altri giocatori di questa battaglia

## ui.replay.filter
ui-replay-filter-raw_xp = XP grezzo
ui-replay-filter-entity_id = ID entità
ui-replay-filter-observed_damage = Danni osservati
ui-replay-filter-heals = Riparazioni
ui-replay-filter-fires = Incendi
ui-replay-filter-floods = Allagamenti
ui-replay-filter-citadels = Cittadelle
ui-replay-filter-crits = Colpi critici ai moduli

## ui.replay.group
ui-replay-group-date = Data
ui-replay-group-ship = Nave
ui-replay-group-none = Nessuno
ui-replay-group-prefix = Raggruppamento: { $label }

## ui.replay.player
ui-replay-player-hidden_profile = Il giocatore ha un profilo nascosto
ui-replay-player-never_connected = Il giocatore non si è mai connesso alla partita

## ui.replay.build
ui-replay-build-open_shiptool = Apri build in ShipTool
ui-replay-build-open_in_ship_builder = Apri build in WoWs SB
ui-replay-build-copy_link = Copia link build
ui-replay-build-copy_short_link = Copia link build breve
ui-replay-build-link_copied = Link build copiato
ui-replay-build-open_wows_numbers = Apri pagina WoWs Numbers
ui-replay-build-not_spotted = Questa nave non è mai stata avvistata. Informazioni build non disponibili.

## ui.replay.sections
ui-replay-sections-achievements = Traguardi
ui-replay-sections-ribbons = Nastri
ui-replay-sections-modules = Moduli:
ui-replay-sections-modules_none = Nessun modulo
ui-replay-sections-signals = Segnali:
ui-replay-sections-loadout = Configurazione:
ui-replay-sections-loadout_none = Nessuna configurazione
ui-replay-sections-abilities = Consumabili:
ui-replay-sections-abilities_none = Nessun consumabile
ui-replay-sections-captain_skills = Abilità capitano:
ui-replay-sections-captain_skills_none = Nessuna abilità capitano
ui-replay-sections-consumables = Consumabili:
ui-replay-sections-fire_chance = Probabilità d'incendio effettiva
ui-replay-sections-fire_chance_expected = prevista
ui-replay-sections-fire_chance_fires = { $fires } incendi
ui-replay-sections-fire_chance_fires_one = 1 incendio
ui-replay-sections-fire_chance_eligible_hits = { $hits } colpi che avrebbero potuto appiccarne uno
ui-replay-sections-fire_chance_eligible_hits_one = 1 colpo che avrebbe potuto appiccarne uno
ui-replay-sections-fire_chance_ships = su { $ships } navi bersaglio
ui-replay-sections-fire_chance_ships_one = su 1 nave bersaglio
ui-replay-sections-fire_chance_expected_fires = previsti { $fires } incendi
ui-replay-sections-fire_chance_per_ship = Per nave bersaglio
ui-replay-sections-fire_chance_no_eligible_hits = nessun colpo che avrebbe potuto appiccare un incendio
ui-replay-sections-fire_chance_formula = Formula della probabilità d'incendio dell'attaccante
ui-replay-sections-fire_chance_formula_base = burnProb di base
ui-replay-sections-fire_chance_formula_clamped = limitata
ui-replay-sections-fire_chance_breakdown = Dettaglio
ui-replay-sections-fire_chance_shells_fired = proiettili HE sparati
ui-replay-sections-fire_chance_shells_fired_one = proiettile HE sparato
ui-replay-sections-fire_chance_he_hits = colpi HE su una nave
ui-replay-sections-fire_chance_he_hits_one = colpo HE su una nave
ui-replay-sections-fire_chance_no_target_ship = colpi HE non attribuibili a una nave bersaglio
ui-replay-sections-fire_chance_no_target_ship_one = colpo HE non attribuibile a una nave bersaglio
ui-replay-sections-fire_chance_eligible = avrebbero potuto appiccare un incendio
ui-replay-sections-fire_chance_ribbons = nastri SetFire
ui-replay-sections-fire_chance_ribbons_one = nastro SetFire
ui-replay-sections-fire_chance_ribbons_credited = attribuiti a un proiettile
ui-replay-sections-fire_chance_ribbons_uncredited = non attribuiti
ui-replay-sections-fire_chance_unattributed_no_burn_state = nessun cambio di stato d'incendio a cui abbinarlo
ui-replay-sections-fire_chance_unattributed_already_credited = un incendio precedente ha già usato il colpo vicino
ui-replay-sections-fire_chance_unattributed_secondary_contest = anche uno dei nostri secondari avrebbe potuto appiccarlo
ui-replay-sections-fire_chance_unattributed_hit_contest = ogni nostro colpo vicino era ambiguo con un altro
ui-replay-sections-fire_chance_unattributed_all_excluded = ogni nostro colpo vicino era già stato escluso
ui-replay-sections-fire_chance_unattributed_none_could_burn = nessun nostro colpo vicino avrebbe potuto appiccare un incendio
ui-replay-sections-fire_chance_unattributed_no_hit = nessun nostro colpo è andato a segno nell'intervallo
ui-replay-sections-fire_chance_not_applicable = vittima già distrutta, non applicabile
ui-replay-sections-fire_chance_copied = Dettaglio della probabilità d'incendio copiato
ui-replay-sections-fire_chance_click_to_copy = Clicca per copiare questo dettaglio
ui-replay-sections-fire_chance_exclusion_already_burning = sezione già in fiamme
ui-replay-sections-fire_chance_exclusion_victim_build_unknown = build della vittima sconosciuta, le zone d'incendio potrebbero essere unite
ui-replay-sections-fire_chance_exclusion_damage_control_active = Squadra di controllo danni attiva
ui-replay-sections-fire_chance_exclusion_damage_control_unknown = stato del controllo danni sconosciuto
ui-replay-sections-fire_chance_exclusion_observation_gap = lacuna nell'osservazione
ui-replay-sections-fire_chance_exclusion_consumable_unreliable = modello di attivazione dei consumabili inaffidabile
ui-replay-sections-fire_chance_exclusion_victim_fate_unknown = sorte della vittima sconosciuta
ui-replay-sections-fire_chance_exclusion_hit_type_does_not_roll = questo tipo di colpo non tira per l'incendio
ui-replay-sections-fire_chance_exclusion_no_geometry = nessuna geometria delle sezioni d'incendio
ui-replay-sections-fire_chance_exclusion_impact_unplaceable = impossibile collocare l'impatto sulla nave a cui è stato abbinato
ui-replay-sections-fire_chance_exclusion_victim_pose_unknown = posizione della vittima non nota al momento dell'impatto
ui-replay-sections-fire_chance_exclusion_ambiguous_hit = ambiguo con un altro nostro colpo
ui-replay-sections-damage_events = Eventi danno

## ui.replay.debug
ui-replay-debug-view_raw_metadata = Visualizza metadati grezzi giocatore
ui-replay-debug-raw_metadata = Metadati grezzi
ui-replay-debug-raw_results = Risultati grezzi
ui-replay-debug-mapped_results = Risultati mappati
ui-replay-debug-player_metadata = Metadati giocatore
ui-replay-debug-no_results_packet = Questo replay non contiene il pacchetto dei risultati della battaglia
ui-replay-debug-raw_json_tooltip = I risultati di battaglia grezzi come serializzati da WG.
ui-replay-debug-mapped_json_tooltip = Risultati di battaglia con array posizionali risolti in campi denominati.
ui-replay-debug-results_raw_json = Risultati battaglia: JSON grezzo
ui-replay-debug-results_mapped_json = Risultati battaglia: JSON mappato
ui-replay-debug-search = Cerca
ui-replay-debug-previous_match = Risultato precedente (Maiusc+Invio)
ui-replay-debug-next_match = Risultato successivo (Invio)

## ui.replay.results
ui-replay-results-victory = Vittoria
ui-replay-results-defeat = Sconfitta
ui-replay-results-draw = Pareggio

## ui.replay.controls
ui-replay-controls-window_title = Controlli replay
ui-replay-controls-unavailable = Controlli non disponibili (commands.scheme.xml non trovato nei file di gioco).

# -- Collaborazione / Sessione --

## ui.collab
ui-collab-session = Sessione
ui-collab-starting = Avvio sessione…
ui-collab-session_active = Sessione attiva
ui-collab-connected_to_session = Connesso alla sessione
ui-collab-session_token = Token sessione:
ui-collab-toggle_visibility = Mostra/nascondi token
ui-collab-copy_token = Copia token
ui-collab-token_copied = Token copiato negli appunti
ui-collab-copy_web_link = Copia link web
ui-collab-copy_web_link_tooltip = Copia link per il client web
ui-collab-web_link_copied = Link web copiato negli appunti
ui-collab-copy_localhost_link = Copia link localhost
ui-collab-copy_localhost_tooltip = Copia link localhost (dev)
ui-collab-localhost_copied = Link localhost copiato negli appunti
ui-collab-connected_count = { $count } connessi
ui-collab-promote_cohost = Promuovi a co-host
ui-collab-promoted_cohost = { $name } promosso a co-host
ui-collab-permissions = Permessi
ui-collab-lock_annotations = Blocca annotazioni
ui-collab-lock_settings = Blocca impostazioni
ui-collab-reset_overrides = Reimposta sovrascritture client
ui-collab-reset_overrides_tooltip = Reimposta tutte le modifiche alle impostazioni di visualizzazione del client
ui-collab-leave = Esci
ui-collab-role_host = Host
ui-collab-role_cohost = Co-host
ui-collab-you = (tu)
ui-collab-display_name = Nome visualizzato:
ui-collab-display_name_hint = Il tuo nome...
ui-collab-display_name_error = Inserisci un nome visualizzato
ui-collab-enter_display_name = Inserisci prima un nome visualizzato
ui-collab-host_session = Ospita una sessione
ui-collab-start_session = Avvia sessione
ui-collab-join_session = Unisciti a una sessione
ui-collab-paste_and_join = Incolla token e unisciti
ui-collab-clipboard_empty = Gli appunti sono vuoti
ui-collab-invalid_token = Token non valido: { $error }
ui-collab-no_runtime = Impossibile avviare la sessione: nessun runtime
ui-collab-shared_windows = Finestre condivise
ui-collab-watching = In visione
ui-collab-waiting_for_frames = In attesa dell'arrivo della battaglia
ui-collab-open = Apri
ui-collab-open_for_everyone = Apri per tutti
ui-collab-tactics_board = Tavola tattica
ui-collab-max_boards = Massimo { $count } tavole tattiche aperte
ui-collab-waiting_for_data = In attesa del caricamento dei dati di gioco…

# -- Testo risultato battaglia --

## battle
battle-victory = VITTORIA
battle-defeat = SCONFITTA
battle-draw = PAREGGIO
battle-battle_starts_in = LA BATTAGLIA INIZIA TRA

# -- Descrizioni tipo di fine --

## finish_type
finish_type-unknown = Battaglia terminata
finish_type-extermination = Tutte le navi nemiche distrutte
finish_type-base_captured = Base catturata
finish_type-timeout = Tempo scaduto
finish_type-failure = Missione fallita
finish_type-technical = Fine tecnica
finish_type-score = Limite punteggio raggiunto
finish_type-score_on_timeout = Vantaggio punteggio allo scadere del tempo
finish_type-pve_success = Missione compiuta
finish_type-pve_failed = Missione fallita
finish_type-score_zero = Punteggio esaurito
finish_type-score_excess = Limite punteggio superato

# -- Scheda statistiche --

## ui.stats
ui-stats-overview = Panoramica
ui-stats-sample_notice = Dati di esempio, non partite registrate
ui-stats-limit = Limite
ui-stats-division = Div:
ui-stats-div_all = Tutti
ui-stats-div_solo = Solo
ui-stats-div_div = Div
ui-stats-filters = Filtri
ui-stats-own_filters = Filtra solo questo grafico
ui-stats-own_filters_tooltip = Restringe questo grafico senza cambiare ciò che mostrano gli altri riquadri
ui-stats-mode_label = Modalità:
ui-stats-clear = Cancella
ui-stats-pr = PR:
ui-stats-mc = MC:
ui-stats-frags_count = { $count } eliminazioni
ui-stats-best_frags = Migliore: { $ship } ({ $count } eliminazioni)
ui-stats-max_damage = Danni max: { $ship } ({ $damage })
ui-stats-copy_markdown = Copia come Markdown
ui-stats-copy_image = Copia come immagine
ui-stats-copy_image_done = Grafico copiato negli appunti
ui-stats-copy_image_failed = Impossibile copiare il grafico
ui-stats-copy_csv = Copia come CSV
ui-stats-remove_games_hint = Rimuovi tutte le partite di { $ship } (Ctrl+Click per saltare la conferma)
ui-stats-no_stats = Nessuna statistica di sessione disponibile. Gioca qualche partita prima!
ui-stats-settings = Impostazioni
ui-stats-chart_type = Tipo grafico
ui-stats-stat_label = Statistica:
ui-stats-chart_line = Linea
ui-stats-chart_bar = Barre
ui-stats-options = Opzioni
ui-stats-combined = Combinato
ui-stats-rolling_avg = Media mobile
ui-stats-labels = Etichette
ui-stats-ships = Navi
ui-stats-filter_ships = Filtra navi del grafico
ui-stats-all_ships = Tutte
ui-stats-no_ships = Nessuna
ui-stats-search_ships = Cerca navi...
ui-stats-no_ship_matches = Nessuna nave corrisponde alla ricerca
ui-stats-search_statistics = Cerca statistiche
ui-stats-no_statistic_matches = Nessuna statistica corrisponde alla ricerca
ui-stats-ship_selection_count = { $selected } di { $total } selezionate
ui-stats-all_ships_tooltip = Seleziona tutte le navi corrispondenti
ui-stats-no_ships_tooltip = Deseleziona le navi corrispondenti
ui-stats-copy_as_image = Copia come immagine
ui-stats-limit_to_recent = Limita alle partite recenti
ui-stats-limit_to_recent_tooltip = Il limite si applica per nave in Navi e Grafici, e su tutte le partite in Panoramica.
ui-stats-division_label = Divisione:
ui-stats-add_chart = Aggiungi grafico
ui-stats-clear_confirm = Cancellare tutte le partite?
ui-stats-clear_tooltip = Dimentica tutte le partite registrate
ui-stats-clear_failed = Non cancellato: { $reason }
ui-stats-clear_ship_confirm = Premi di nuovo per dimenticare le partite di questa nave
ui-stats-copy_table = Copia questa tabella
ui-stats-copied = Copiato negli appunti
ui-stats-no_games = Nessuna partita registrata per i filtri attuali
ui-stats-nothing_to_plot = Nulla da rappresentare per i filtri attuali
ui-stats-column_ship = Nave
ui-stats-column_battles = Battaglie
ui-stats-column_win_rate = % vittorie
ui-stats-column_avg_damage = Danni medi
ui-stats-column_avg_frags = Eliminazioni medie
ui-stats-frags = { $count } eliminazioni
ui-stats-statistic = Statistica
ui-stats-open_directory_stats = Statistiche cartella
ui-stats-open_directory_stats_tooltip = Apri le statistiche di tutti i replay in questa cartella e nelle sue sottocartelle
ui-stats-directory_stats_unavailable = Dati di gioco ancora in caricamento
ui-stats-directory_reading_task = Lettura dei replay per le statistiche cartella...
ui-stats-directory_walking = Ricerca dei replay...
ui-stats-directory_scanning = Controllo di { $done } replay su { $total }...
ui-stats-directory_reading = Lettura di { $done } replay su { $total }...
ui-stats-directory_reload = Ricarica
ui-stats-directory_reload_tooltip = Rileggi i replay di questa cartella
ui-stats-directory_skipped = { $count } replay saltati
ui-stats-directory_skipped_detail =
    Illeggibili: { $unreadable }
    Nessun dato di gioco per la loro build: { $missing_build }
    Nessun risultato di battaglia: { $no_results }
ui-stats-directory_no_games = Nessun replay in questa cartella ha risultati di battaglia
ui-stats-chart = Grafico
ui-stats-combine_ships = Combina navi
ui-stats-running_average = Media progressiva
ui-stats-value_labels = Etichette valori
ui-stats-reset_view = Reimposta vista
ui-stats-keep_current_layout = Mantieni layout attuale
ui-stats-session_title = Statistiche sessione
ui-stats-session_manual = Manuale
ui-stats-session_since_opened = Dall'avvio dell'app
ui-stats-session_manual_tooltip = La sessione contiene le battaglie che giochi e i replay che imposti o aggiungi dall'elenco dei replay. Viene mantenuta tra un avvio e l'altro.
ui-stats-session_since_opened_tooltip = La sessione contiene ogni battaglia iniziata alle { $time } o dopo, quando l'app è stata avviata. La tua sessione manuale viene mantenuta e ritorna quando passi a Manuale.
ui-stats-session_battles_one = 1 battaglia
ui-stats-session_battles_many = { $count } battaglie
ui-stats-session_chip_manual = Sessione: { $battles }
ui-stats-session_chip_since = Sessione dalle { $time }: { $battles }
ui-stats-session_empty_title = Ancora nessuna battaglia in questa sessione
ui-stats-session_empty_manual = Le battaglie che giochi si aggiungono alla sessione al termine. Puoi anche impostare o aggiungere replay dal menu dell'elenco dei replay.
ui-stats-session_empty_since = Le battaglie che iniziano dopo le { $time } si aggiungono alla sessione al termine.
ui-stats-session_set_add_unavailable = La sessione contiene le battaglie dalle { $time }. Passa a Manuale per impostare o aggiungere replay.
ui-stats-clear_unavailable_since = Una sessione dall'avvio dell'app non viene salvata. Passa a Manuale per cancellare la sessione salvata.
ui-stats-charts_save_failed = Impostazioni dei grafici non salvate: { $reason }
ui-stats-layout_save_failed = Layout dei grafici non salvato: { $reason }
ui-stats-axis_game = Partita
ui-stats-chart_hover_game = Partita { $number }, { $time }
ui-stats-series_average = { $stat } (media)
ui-stats-series_running = { $stat } (progressiva)
ui-stats-series_more = +{ $count } altre
ui-stats-mode_random = Casuali
ui-stats-mode_ranked = Classificate
ui-stats-mode_other = Altre
ui-stats-win_rate_rated_only = La % vittorie conta solo le battaglie casuali e classificate
ui-stats-win_rate_breakdown_line = { $mode }: { $rate } (battaglie: { $battles })
ui-stats-matches = Partite
ui-stats-column_time = Ora
ui-stats-column_mode = Modalità
ui-stats-column_result = Risultato
ui-stats-keep_game = Conta questa partita nelle statistiche
ui-stats-leave_out_game = Escludi questa partita dalle statistiche
ui-stats-leave_out_ship = Escludi tutte le partite con { $ship }
ui-stats-leave_out_ship_hint = Escludi { $ship } dalle statistiche
ui-stats-keep_all_games = Conta tutte le partite
ui-stats-keep_all_games_tooltip = Conta di nuovo tutte le partite escluse manualmente
ui-stats-left_out_only = Solo escluse
ui-stats-match_count = { $shown } di { $total } partite, { $left_out } escluse
ui-stats-no_matches = Nessuna partita corrisponde ai filtri attuali
ui-stats-search_matches = Cerca per nave...
ui-stats-ship_filter_tooltip = Scegli quali navi contano nelle statistiche
ui-stats-keep_ships_tooltip = Conta tutte le navi corrispondenti
ui-stats-leave_out_ships_tooltip = Escludi tutte le navi corrispondenti dalle statistiche
ui-stats-dates = Date
ui-stats-all_dates = Tutte le date
ui-stats-clear_dates = Mostra tutte le date
ui-stats-games_left_out = { $count } partite escluse
ui-stats-reset_filters = Reimposta filtri
ui-stats-reset_filters_tooltip = Mostra tutte le partite: nessun limite, intervallo di date, divisione, modalità o navi e partite escluse

## ui.stats.table
ui-stats-table-min = Min
ui-stats-table-max = Max
ui-stats-table-total = Totale
ui-stats-table-average = Media

# -- Tracciamento giocatori --

## ui.player_tracker
ui-player_tracker-clear_stats = Cancella statistiche
ui-player_tracker-player_filter = Filtro giocatori
ui-player_tracker-populate_from_replays = Popola dati dai replay
ui-player_tracker-find_matches = Trova partite
ui-player_tracker-open_wows_numbers = Apri su wows-numbers
ui-player_tracker-open_shipbuilds = Apri le build su shipbuilds
ui-player_tracker-subtab_historical = Storico
ui-player_tracker-subtab_players = Giocatori
ui-player_tracker-filter_hint = Filtra per giocatore o clan...
ui-player_tracker-time_period = Periodo
ui-player_tracker-allies = Alleati
ui-player_tracker-enemies = Nemici
ui-player_tracker-live_no_directory = Imposta la cartella di World of Warships per monitorare le battaglie
ui-player_tracker-live_checking = Ricerca di una battaglia in corso...
ui-player_tracker-live_none = Nessuna battaglia in corso
ui-player_tracker-live_loading_build = Caricamento dei dati navi di questa build; nomi e classi compariranno al termine.
ui-player_tracker-waiting_for_index = In attesa dell'indice dei replay
ui-player_tracker-loading_clans = Caricamento clan...
ui-player_tracker-loading_players = Caricamento giocatori...
ui-player_tracker-index_failed = Impossibile leggere l'indice: { $reason }
ui-player_tracker-no_players = Nessun giocatore indicizzato per questo periodo
ui-player_tracker-notes_title = Note: { $name }
ui-player_tracker-note_not_saved = Non salvata: { $reason }
ui-player_tracker-subtab_current_match = Partita attuale
ui-player_tracker-your_team = La tua squadra ({ $count })
ui-player_tracker-enemy_team = Squadra nemica ({ $count })
ui-player_tracker-no_live_match = Nessuna partita in corso. L'elenco compare quando entri in partita.
ui-player_tracker-encounters_hover = Visto { $total } volte in totale, { $range } nel periodo selezionato. Ultimo avvistamento { $last }.
ui-player_tracker-never_encountered = Mai incontrato prima
ui-player_tracker-copy_wg_id = Clicca per copiare l'ID WG
ui-player_tracker-copied_wg_id = ID WG { $id } copiato
ui-player_tracker-aliases_hover = Conosciuto anche come: { $names }
ui-player_tracker-notes_hint = Note su questo giocatore
ui-player_tracker-arena_count = { $count } partite registrate
ui-player_tracker-last_encountered_exact = Ultimo incontro { $timestamp }
ui-player_tracker-subtab_clans = Clan
ui-player_tracker-clan_no_data = Nessun incontro con clan registrato finora.
ui-player_tracker-find_clan_matches = Trova le partite che menzionano questo tag. L'indice non ha un campo dedicato ai soli clan, quindi corrispondono anche i nomi giocatore che contengono il tag.
ui-player_tracker-clan_sightings_hover = { $range } di questi rientrano nel periodo selezionato.
ui-player_tracker-clan_member_matches = visto in { $count } partite
ui-player_tracker-show_division_mates = Mostra compagni di divisione
ui-player_tracker-show_division_mates_hover = Conta le partite giocate in divisione. Disattivato per impostazione predefinita, perché hai incontrato quei giocatori lì avendoli invitati tu. Le altre partite contro gli stessi giocatori sono sempre contate.
ui-player_tracker-seen_in_twitch_chat = Visto nella chat di Twitch
ui-player_tracker-seen_in_twitch_chat_hover = Mostra solo i giocatori il cui nome corrispondeva a un utente della chat di Twitch durante una partita con te.
ui-player_tracker-twitch_seen_hover = Visto nella chat di Twitch come { $logins } in { $count } partite con te
ui-player_tracker-win_rate_overall = Generale
ui-player_tracker-win_rate_ship = Nave
ui-player_tracker-view_compact = Compatta
ui-player_tracker-view_detailed = Dettagliata
ui-player_tracker-ship_stats = Statistiche nave
ui-player_tracker-win_rate_mode_hover = Mostra le statistiche dell'account o quelle di questa nave. Il colore della riga, il PR, il numero di battaglie e i danni medi seguono questa scelta.
ui-player_tracker-stats_resolving = Lettura dell'elenco giocatori...
ui-player_tracker-stats_fetching = Recupero delle statistiche dei giocatori...
ui-player_tracker-stats_rate_limited = Limite di richieste raggiunto, riprova tra { $wait }
ui-player_tracker-stats_unavailable = Statistiche non disponibili: { $reason }
ui-player_tracker-roster_unavailable = Impossibile leggere l'elenco giocatori dal replay
ui-player_tracker-stats_hidden = Il profilo di questo giocatore è privato
ui-player_tracker-stats_no_data = Nessuna statistica per questo giocatore
ui-player_tracker-stats_unplayed_ship = Nessuna battaglia registrata con questa nave
ui-player_tracker-pr_overall_hover = PR dell'account su tutte le navi
ui-player_tracker-pr_ship_hover = PR con questa nave
ui-player_tracker-avg_damage = Danni medi
ui-player_tracker-win_rate_hover_line = { $label }: { $rate } su { $battles } battaglie
ui-player_tracker-win_rate_hover_line_damage = { $label }: { $rate } su { $battles } battaglie, { $damage } danni medi
ui-player_tracker-team_average_win_rate = WR media: { $rate }
ui-player_tracker-team_players = { $count } giocatori
ui-player_tracker-row_win_rate = WR { $rate }
ui-player_tracker-row_personal_rating = PR { $value }
ui-player_tracker-row_battles = { $count } battaglie
ui-player_tracker-row_seen = Visto x{ $total }
ui-player_tracker-debug_replay_picker = Debug: tratta un replay come la partita attuale
ui-player_tracker-debug_replay_picker_hover = Carica in questa scheda l'elenco giocatori di un replay concluso, così da poter provare il percorso delle statistiche fuori da una battaglia.
ui-player_tracker-debug_replay_none = Scegli un replay

## ui.player_tracker.column
ui-player_tracker-column-win_rate = WR
ui-player_tracker-column-personal_rating = PR
ui-player_tracker-column-battles = Battaglie
ui-player_tracker-column-avg_damage = Danni
ui-player_tracker-column-encounters = Visto
ui-player_tracker-column-clan = Clan
ui-player_tracker-column-player_name = Nome giocatore
ui-player_tracker-column-total_encounters = Incontri totali
ui-player_tracker-column-encounters_in_range = Incontri nel periodo
ui-player_tracker-column-last_encountered = Ultimo incontro
ui-player_tracker-column-members = Membri
ui-player_tracker-column-matches = Partite
ui-player_tracker-column-sightings = Avvistamenti

## ui.player_tracker.period
ui-player_tracker-period-past_hour = Ultima ora
ui-player_tracker-period-past_six_hours = Ultime 6 ore
ui-player_tracker-period-past_day = Ultime 24 ore
ui-player_tracker-period-past_week = Ultima settimana
ui-player_tracker-period-past_month = Ultimo mese
ui-player_tracker-period-all_time = Tutto il periodo

# -- Gestore mod --


## ui.twitch
ui-twitch-possible_name = Possibile nome stream: { $name }
ui-twitch-seen_minutes = Visto: { $minutes } minuti dopo l'inizio della partita
ui-twitch-click_to_copy = Clicca per copiare
ui-twitch-copied = Copiato { $name }
## ui.mod_manager
ui-mod_manager-category_filters = Filtri categoria
ui-mod_manager-enabled = Abilitato
ui-mod_manager-mod_home = Pagina mod
ui-mod_manager-discord_thread = Thread di approvazione Discord
ui-mod_manager-save_config = Salva configurazione mod
ui-mod_manager-load_config = Carica configurazione mod
ui-mod_manager-filter = Filtro

## ui.mod_manager.column
ui-mod_manager-column-enabled = Abilitato
ui-mod_manager-column-name = Nome
ui-mod_manager-column-author = Autore
ui-mod_manager-column-category = Categoria

# -- Renderer replay --

## ui.renderer
ui-renderer-loading = Caricamento dati replay...
ui-renderer-preparing_export = Preparazione esportazione...
ui-renderer-encoding = Codifica
ui-renderer-muxing = Muxing

## ui.renderer.advantage
ui-renderer-advantage-breakdown = Analisi vantaggio
ui-renderer-advantage-team_eliminated = Una squadra è stata eliminata
ui-renderer-advantage-score_projection = Proiezione punteggio
ui-renderer-advantage-fleet_power = Potenza flotta
ui-renderer-advantage-strategic_threat = Minaccia strategica
ui-renderer-advantage-total = Totale
ui-renderer-advantage-hp_incomplete = Dati HP/nave incompleti — mostrati solo i fattori di punteggio

## ui.renderer.context
ui-renderer-context-show_trail = Mostra scia
ui-renderer-context-disable_other_trails = Disabilita tutte le altre scie
ui-renderer-context-ranges = Portate
ui-renderer-context-detection = Rilevamento
ui-renderer-context-main_battery = Batteria principale
ui-renderer-context-secondary = Secondari
ui-renderer-context-torpedo = Siluro
ui-renderer-context-radar = Radar
ui-renderer-context-hydro = Idrofono
ui-renderer-context-enable_all = Abilita tutto
ui-renderer-context-disable_all = Disabilita tutto
ui-renderer-context-disable_other_ranges = Disabilita tutte le altre portate
ui-renderer-context-enable_all_ranges = Abilita portate di tutte le navi
ui-renderer-context-show_armor = Mostra corazza in tempo reale
ui-renderer-context-realtime_armor = Mostra corazza in tempo reale

## ui.renderer.controls
ui-renderer-controls-jump_to_start = Vai all'inizio
ui-renderer-controls-previous_event = Evento precedente (Shift+Left)
ui-renderer-controls-back_10s = Indietro 10s (Left)
ui-renderer-controls-pause = Pausa (Space)
ui-renderer-controls-play = Riproduci (Space)
ui-renderer-controls-forward_10s = Avanti 10s (Right)
ui-renderer-controls-next_event = Evento successivo (Shift+Right)
ui-renderer-controls-jump_to_end = Vai alla fine
ui-renderer-controls-save_as_video = Salva come video
ui-renderer-controls-copy_to_clipboard = Renderizza video negli appunti
ui-renderer-controls-speed = Velocità di riproduzione

## ui.renderer.settings
ui-renderer-settings-title = Impostazioni di visualizzazione
ui-renderer-settings-hp_bars = Barre PS
ui-renderer-settings-ship_settings = Impostazioni nave
ui-renderer-settings-armament = Armamento
ui-renderer-settings-dead_ships = Navi distrutte
ui-renderer-settings-dead_ship_names = Nomi navi distrutte
ui-renderer-settings-player_names = Nomi giocatori
ui-renderer-settings-ship_ranges = Portate navi
ui-renderer-settings-self_ship_ranges = Portate propria nave
ui-renderer-settings-ship_names = Nomi navi
ui-renderer-settings-camera_direction = Direzione torretta
ui-renderer-settings-trail_settings = Impostazioni scia
ui-renderer-settings-heat_trail = Scia termica
ui-renderer-settings-speed_trails = Scie velocità
ui-renderer-settings-dead_ship_trails = Scie navi distrutte
ui-renderer-settings-map_settings = Impostazioni mappa
ui-renderer-settings-buildings = Edifici
ui-renderer-settings-capture_points = Punti di cattura
ui-renderer-settings-consumables = Consumabili
ui-renderer-settings-planes = Aerei
ui-renderer-settings-smoke = Fumo
ui-renderer-settings-torpedoes = Siluri
ui-renderer-settings-tracers = Traccianti
ui-renderer-settings-hud_settings = Impostazioni HUD
ui-renderer-settings-battle_result = Risultato battaglia
ui-renderer-settings-buff_counters = Contatori buff
ui-renderer-settings-chat_label = Chat
ui-renderer-settings-kill_feed = Registro eliminazioni
ui-renderer-settings-score_label = Punteggio
ui-renderer-settings-score_timers = Timer punteggio
ui-renderer-settings-team_advantage = Vantaggio squadra
ui-renderer-settings-team_rosters = Formazioni squadre
ui-renderer-settings-stats_panel = Pannello statistiche
ui-renderer-settings-timer = Timer
ui-renderer-settings-export_settings = Impostazioni esportazione
ui-renderer-settings-prefer_cpu = Preferisci codificatore CPU
ui-renderer-settings-prefer_cpu_tooltip = Usa codificatore software (CPU) anziché GPU per l'esportazione video
ui-renderer-settings-include_pre_battle = Includi fase pre-battaglia
ui-renderer-settings-include_pre_battle_tooltip = Includi nel video esportato la comparsa delle navi e il conto alla rovescia prima dell'inizio della battaglia
ui-renderer-settings-export_timing = Tempi del video
ui-renderer-settings-export_timing_length = Adatta alla durata
ui-renderer-settings-export_timing_speed = Velocità
ui-renderer-settings-export_timing_result_speed = Riproduce a circa { $speed }x
ui-renderer-settings-export_timing_result_length = Il video dura { $length }
ui-renderer-settings-codec = Codec
ui-renderer-settings-codec_auto = Automatico
ui-renderer-settings-save_defaults = Salva predefiniti
ui-renderer-settings-save_defaults_done = Salvate come impostazioni iniziali del renderer
ui-renderer-settings-save_defaults_failed = Impossibile salvare le impostazioni predefinite
ui-renderer-settings-event_timeline = Timeline eventi

# -- Estrattore file --

## ui.unpacker
ui-unpacker-assets_bin = Assets.bin
ui-unpacker-loading_assets = Caricamento assets.bin...
ui-unpacker-load_failed = Impossibile caricare: { $error }
ui-unpacker-no_game_data = Nessun dato di gioco caricato. Imposta la cartella di World of Warships nelle Impostazioni.
ui-unpacker-filter_files = Filtra file...
ui-unpacker-search_in_files = Cerca nei file
ui-unpacker-search_hint = Cerca nei file...
ui-unpacker-path_filter_hint = Filtro percorso (es. *.xml)
ui-unpacker-queue_all = Accoda tutto
ui-unpacker-queue_all_files = Accoda tutti i file
ui-unpacker-empty_directory = Cartella vuota
ui-unpacker-stop_search_tooltip = Ferma ricerca
ui-unpacker-no_matches = Nessuna corrispondenza trovata.
ui-unpacker-view_as_json = Visualizza come JSON
ui-unpacker-extract_as_json = Estrai come JSON
ui-unpacker-view_contents = Visualizza contenuto
ui-unpacker-version = Versione:
ui-unpacker-game_params = GameParams
ui-unpacker-base_as_json = Base come JSON
ui-unpacker-as_json = Come JSON
ui-unpacker-as_cbor = Come CBOR
ui-unpacker-as_json_minimal = Come JSON (Minimale / Trasformato)
ui-unpacker-as_cbor_minimal = Come CBOR (Minimale / Trasformato)
ui-unpacker-decode_prototypes = Decodifica prototipi come JSON
ui-unpacker-decode_prototypes_tooltip = Decodifica prototipi Assets.bin supportati (.visual, .model, .mfm) in JSON durante l'estrazione
ui-unpacker-extraction_queue = Coda di estrazione
ui-unpacker-collapse_folders = Comprimi albero cartelle
ui-unpacker-expand_folders = Espandi albero cartelle
ui-unpacker-clear_all = Cancella tutto
ui-unpacker-browse = Sfoglia
ui-unpacker-output_dir_hint = Cartella di output...
ui-unpacker-go_to_directory = Vai alla cartella
ui-unpacker-remove_from_queue = Rimuovi dalla coda
ui-unpacker-queue_folder = Accoda intera cartella
ui-unpacker-queue_file = Accoda questo file
ui-unpacker-extraction_output = Estrai in
ui-unpacker-queue_empty = Spunta un file o una cartella nell'elenco per accodarlo
ui-unpacker-show_queue = Coda di estrazione
ui-unpacker-double_click_open = Doppio clic per aprire
ui-unpacker-packages = Pacchetti
ui-unpacker-item_count = { $count } elementi
ui-unpacker-search_title = Ricerca: { $query }
ui-unpacker-search_in = in { $source }
ui-unpacker-image_decode_failed = Impossibile decodificare questa immagine: { $reason }
ui-unpacker-find = Trova
ui-unpacker-find_previous = Risultato precedente
ui-unpacker-find_next = Risultato successivo
ui-unpacker-find_close = Chiudi ricerca
ui-unpacker-find_count = { $current } di { $total }
ui-unpacker-find_none = Nessun risultato
ui-unpacker-find_match_case = Maiuscole/minuscole
ui-unpacker-queue = Coda
ui-unpacker-reveal_in_tree = Mostra nell'albero dei pacchetti
ui-unpacker-hit_offset = a { $offset }
ui-unpacker-queued_count = { $count } in coda
ui-unpacker-counting_files = Conteggio dei file...
ui-unpacker-extracting = Estrazione { $done } di { $total }
ui-unpacker-extract_failed = Estrazione non riuscita: { $reason }
ui-unpacker-extracted = Estratti { $count } file
ui-unpacker-cancelled = Annullato dopo { $count } file
ui-unpacker-extract = Estrai
ui-unpacker-extract_one = Estrai 1 elemento
ui-unpacker-extract_many = Estrai { $count } elementi
ui-unpacker-choose_directory = Scegli prima una cartella di destinazione
ui-unpacker-dump_parameters = Esporta GameParams
ui-unpacker-dump_parameters_tooltip = Esporta il GameParams.data di questa build
ui-unpacker-load_a_build = Carica prima una build
ui-unpacker-parameters_written = Parametri scritti
ui-unpacker-save_parameters_title = Salva parametri di gioco
ui-unpacker-writing = Scrittura in corso...
ui-unpacker-prototype_written = Prototipo scritto
ui-unpacker-whole_tree = Albero completo
ui-unpacker-base_only = Solo parametri base

## ui.unpacker.column
ui-unpacker-column-name = Nome
ui-unpacker-column-size = Dimensione
ui-unpacker-column-type_col = Tipo
ui-unpacker-column-path = Percorso
ui-unpacker-column-file = File
ui-unpacker-column-offset = Offset
ui-unpacker-column-context = Contesto

# -- Visualizzatore corazza --

## ui.armor
ui-armor-load_settings = Carica i dati di gioco nelle Impostazioni per usare il Visualizzatore corazza.
ui-armor-gpu_unavailable = Stato di rendering GPU non disponibile.
ui-armor-loading_ship = Caricamento dati nave...
ui-armor-load_failed = Impossibile caricare i dati della nave: { $error }
ui-armor-ship_load_failed = Impossibile caricare { $ship }: { $reason }
ui-armor-empty = Vuoto
ui-armor-mirror_cameras = Specchia telecamere
ui-armor-sync_options = Sincronizza opzioni
ui-armor-show_armor_by_default = Mostra la corazza per impostazione predefinita
ui-armor-sync_tooltip = Mantieni sincronizzati tra tutti i pannelli la visibilità corazza/scafo e gli overlay di visualizzazione (centro nave, linea di galleggiamento, anelli telecamera)
ui-armor-common_settings = Impostazioni comuni
ui-armor-tier_filter = Livelli da { $lowest } a { $highest }
ui-armor-fortifications = Fortificazioni
ui-armor-show_hidden_ships = Mostra navi nascoste
ui-armor-show_hidden_ships_tooltip = Elenca anche le navi che l'elenco omette, come quelle a noleggio del clan
ui-armor-compare_split = Confronta in nuovo pannello
ui-armor-open_in_new_tab = Apri in nuova scheda
ui-armor-export_model = Esporta modello nave
ui-armor-armor_thickness = Spessore corazza
ui-armor-export_disclaimer = Questi modelli 3D e texture sono proprietà intellettuale di Wargaming e qualsiasi utilizzo di questi modelli deve essere conforme alle leggi locali.
ui-armor-export_button = Esporta
ui-armor-armor_toggle = Corazza
ui-armor-armor_tooltip = Mostra/nascondi zone corazza
ui-armor-hull_toggle = Scafo
ui-armor-hull_tooltip = Mostra/nascondi parti del modello scafo
ui-armor-splash_toggle = Splash
ui-armor-splash_tooltip = Mostra/nascondi box splash
ui-armor-show_hidden = Mostra nascosti
ui-armor-show_hidden_tooltip = Mostra/nascondi pannelli (possibilmente) nascosti
ui-armor-gaps = Fessure
ui-armor-gaps_tooltip = Evidenzia i bordi limite dove i pannelli corazza non si connettono (possibili fessure)
ui-armor-display = Visualizzazione
ui-armor-display_tooltip = Impostazioni visualizzazione (Ctrl+S)
ui-armor-marker_opacity = Opacità indicatori
ui-armor-save_defaults = Salva come predefiniti
ui-armor-export_tab = Esporta
ui-armor-export_tooltip = Esporta modello nave in file OBJ
ui-armor-pen_check = Controllo penetrazione
ui-armor-pen_check_tooltip = Configura proiettili per il confronto penetrazione
ui-armor-trajectory = Traiettoria
ui-armor-trajectory_on = Traiettoria [ON]
ui-armor-trajectory_range = Distanza
ui-armor-trajectory_clear = Cancella tutto
ui-armor-arc_heading = Traiettoria { $number }
ui-armor-arc_summary = { $hits } colpi, { $armor }mm a { $range } km
ui-armor-arc_delete = Elimina questa traiettoria
ui-armor-arc_isolate_plates = Isola piastre
ui-armor-arc_isolate_plates_tooltip = Mostra solo le piastre corazzate attraversate da questa traiettoria
ui-armor-arc_isolate_zones = Isola zone
ui-armor-arc_isolate_zones_tooltip = Mostra tutte le parti delle zone corazzate in cui è entrata questa traiettoria
ui-armor-arc_simulation_caveat = Questa simulazione si basa su dati ottenuti tramite reverse engineering e potrebbe non corrispondere al calcolo balistico del gioco.
ui-armor-arc_no_shell = Nessun proiettile sparato; le piastre attraversate sono mostrate senza
ui-armor-arc_detonated_in = Esplode all'interno di { $zone }
ui-armor-arc_detonated_past_armor = Esplode oltre tutta la corazza
ui-armor-arc_ricochet = Rimbalzo su { $plate }
ui-armor-arc_shatter = Frantumazione su { $plate }
ui-armor-arc_stopped = Fermato da { $plate }
ui-armor-arc_overpen = Perforazione eccessiva
ui-armor-arc_overpen_unarmed = Perforazione eccessiva (spoletta non armata)
ui-armor-arc_plate_unknown = una piastra non registrata
ui-armor-angle_good = sotto 30 gradi
ui-armor-angle_mid = da 30 a 45 gradi
ui-armor-angle_bad = oltre 45 gradi
ui-armor-trajectory_tooltip = Clicca sulla corazza per simulare traiettorie proiettili (Ctrl+T)
ui-armor-trajectory_ship = Nave:
ui-armor-trajectory_select_ship = Seleziona nave
ui-armor-trajectory_ship_tooltip = Scegli la nave che fornisce il proiettile della traiettoria
ui-armor-trajectory_add_ship = Aggiungi nave che spara
ui-armor-trajectory_add_ship_tooltip = Apri Controllo penetrazione per cercare e aggiungere una nave
ui-armor-trajectory_shell = Proiettile:
ui-armor-trajectory_select_shell = Seleziona proiettile
ui-armor-trajectory_shell_tooltip = Scegli il proiettile usato per le traiettorie
ui-armor-continue_ricochet = Continua dopo il rimbalzo
ui-armor-splash_mode = Splash
ui-armor-splash_on = Splash [ON]
ui-armor-splash_no_data = Nessun dato splash per questa nave
ui-armor-splash_no_shells = Aggiungi una nave con proiettili HE/SAP alla lista di confronto
ui-armor-splash_tooltip_mode = Clicca sulla corazza per visualizzare il volume di danno splash HE
ui-armor-loading_armor = Caricamento nave...
ui-armor-select_ship = Seleziona una nave dalla lista
ui-armor-pen_check_label = Controllo penetrazione
ui-armor-all_btn = Tutti
ui-armor-none_btn = Nessuno
ui-armor-opaque = Opaco
ui-armor-upgrade = Potenziamento:
ui-armor-camo = Mimetica:
ui-armor-camo_none = Base (nessuna)
ui-armor-camo_group_universal = Mimetiche universali
ui-armor-camo_group_expendable = Mimetiche consumabili
ui-armor-camo_group_other = Altre mimetiche
ui-armor-show_armor_thickness = Mostra legenda spessore corazza
ui-armor-plate_edges = Bordi piastra
ui-armor-waterline = Linea di galleggiamento
ui-armor-camera_rings = Anelli telecamera
ui-armor-realtime_readout = { $health } PS  -  { $hits } colpi
ui-armor-show_camera_rings = Mostra anelli telecamera
ui-armor-camera_fov = FOV telecamera
ui-armor-camera_height = Altezza telecamera
ui-armor-show_camera_zoom_path = Mostra percorso zoom
ui-armor-zoom_path_regular_fov = FOV normale
ui-armor-zoom_path_max_fov = FOV massimo
ui-armor-camera_perspective = Prospettiva telecamera
ui-armor-perspective_fov = FOV vista
ui-armor-perspective_projection = Proiezione
ui-armor-perspective_proj_game = Gioco
ui-armor-perspective_proj_center = Attraverso il centro
ui-armor-ship_center = Centro nave
ui-armor-grid = Griglia
ui-armor-length_marks = Indicatori di lunghezza
ui-armor-length_readout = Lunghezza { $length }, larghezza { $beam }
ui-armor-length_metres = { $value } m
ui-armor-length_centimetres = { $value } cm
ui-armor-filter_options = Filtra opzioni...
ui-armor-filter_zones = Filtra zone...
ui-armor-filter_parts = Filtra parti...
ui-armor-tree_matches = Corrispondenze: { $count }
ui-armor-tree_no_matches = Nessuna corrispondenza
ui-armor-no_matching_options = Nessuna opzione corrisponde al filtro
ui-armor-opacity = Opacità
ui-armor-ship_hull = Scafo nave
ui-armor-opaque_hull = Scafo opaco
ui-armor-lighting = Illuminazione scafo
ui-armor-lighting_enabled = Abilita illuminazione
ui-armor-lighting_preset_ingame = In gioco
ui-armor-lighting_preset_flat = Piatto
ui-armor-lighting_preset_studio = Studio
ui-armor-lighting_flat = Piatto (ambientale)
ui-armor-lighting_intensity = Intensità luce
ui-armor-lighting_azimuth = Azimut luce
ui-armor-lighting_elevation = Elevazione luce
ui-armor-lighting_rim = Contorno
ui-armor-lighting_specular = Speculare
ui-armor-lighting_shininess = Lucentezza
ui-armor-lighting_key_color = Colore principale
ui-armor-lighting_ambient_color = Colore ambientale
ui-armor-pop_out = Separa
ui-armor-zero_mm_plates = Piastre 0 mm
ui-armor-armor_opacity = Opacità corazza
ui-armor-ballistic_disclaimer = I risultati balistici si basano su reverse engineering/stime del funzionamento della simulazione
ui-armor-roll = Rollio
ui-armor-ctrl_click_solo = Ctrl+clic per isolare
ui-armor-reset_plates = Reimposta piastre
ui-armor-visibility = Visibilità
ui-armor-armor_options = Corazza
ui-armor-no_ship_loaded = Nessuna nave caricata
ui-armor-lod = LOD:
ui-armor-lod_highest = 0 (massimo)
ui-armor-camouflage = Mimetizzazione
ui-armor-search_ships = Cerca navi...
ui-armor-ships = Navi
ui-armor-compare = Confronta
ui-armor-compare_tooltip = Apri un nuovo riquadro di confronto, o clic destro su una nave per confrontarla
ui-armor-mirror_cameras_tooltip = Muovi insieme le telecamere di tutti i riquadri
ui-armor-show_armor_by_default_tooltip = Apri i modelli mostrando la corazza invece dello scafo
ui-armor-xray_highlight = Evidenziazione a raggi X al passaggio nella vista
ui-armor-xray_always = Sempre
ui-armor-xray_while_held = Tenendo premuto { $key }
ui-armor-xray_off = Disattivata
ui-armor-stack_panes = Impila riquadri
ui-armor-stack_panes_tooltip = Disponi i riquadri di confronto uno sopra l'altro invece che affiancati
ui-armor-viewports = Viste
ui-armor-options_panel = Pannello opzioni
ui-armor-options_pane = Riquadro { $number } di { $count }
ui-armor-splash_boxes = Volumi splash
ui-armor-cast_here_hint = Tieni premuto { $key } e fai clic per sparare un proiettile qui
ui-armor-trajectory_next_cast = Prossimo colpo
ui-armor-arc_pick_hint = Fai clic su un indicatore per modificarne la traiettoria.
ui-armor-arc_marker_tooltip = Traiettoria { $number }. Fai clic per modificarla. Clic destro per rimuoverla.
ui-armor-zone_toggle_tooltip = Mostra o nasconde tutte le piastre di questa zona. Tieni premuto { $key } per mostrare solo questa zona.
ui-armor-mode_solo_zone = solo zona
ui-armor-mode_cast_trajectory = spara traiettoria
ui-armor-loading_catalog = Caricamento catalogo navi...
ui-armor-catalog_failed = Impossibile caricare il catalogo navi: { $reason }
ui-armor-legend_collapse = Comprimi
ui-armor-legend_hide = Nascondi legenda
ui-armor-armor_thickness_title = Spessore corazza
ui-armor-initializing = Inizializzazione vista 3D...
ui-armor-init_failed = Impossibile inizializzare la vista corazza: { $reason }
ui-armor-loading_named_ship = Caricamento { $ship }...
ui-armor-rendering = Rendering in corso...
ui-armor-options = Opzioni

## ui.armor.compass
ui-armor-compass-north = N
ui-armor-compass-east = E
ui-armor-compass-south = S
ui-armor-compass-west = O

## ui.armor.realtime
ui-armor-realtime-loading = Caricamento corazza nave...
ui-armor-realtime-waiting = In attesa dei dati nave...
ui-armor-realtime-incoming_fire = Fuoco in arrivo
ui-armor-realtime-attacker_filter = Filtro attaccante
ui-armor-realtime-all_enemies = Tutti i nemici
ui-armor-realtime-show_secondaries = Mostra armamento secondario
ui-armor-realtime-auto_scroll = Scorrimento automatico
ui-armor-realtime-seek_to_salvo = Vai alla salva
ui-armor-realtime-deselect = Deseleziona
ui-armor-realtime-select_all = Seleziona tutto
ui-armor-realtime-no_data = Dati proiettile non più disponibili
ui-armor-realtime-sim_agrees = Simulazione concorde
ui-armor-realtime-sim_unchecked = Impossibile simulare { $count } proiettili
ui-armor-realtime-sim_rng_zone = Zona RNG ({ $angle } gradi in { $from }-{ $to })
ui-armor-realtime-sim_disagrees = Sim: { $sim } / Server: { $server }
ui-armor-realtime-no_armor_hit = Nessuna piastra corazza colpita
ui-armor-realtime-tracked = { $salvos } salve, { $shells } proiettili tracciati
ui-armor-realtime-nothing_landed = Nessun colpo ha ancora raggiunto questa nave
ui-armor-realtime-shells = { $count }x
ui-armor-realtime-salvo_shell = { $shell } da { $caliber } mm
ui-armor-realtime-salvo_range = { $km } km
ui-armor-realtime-unknown_attacker = Nave { $id }
ui-armor-realtime-server_outcome = Server: { $outcome }
ui-armor-realtime-not_cast = Impossibile seguire questo proiettile nello scafo: il suo volo o la posizione della nave non sono stati registrati
ui-armor-realtime-pick_hint = Fai clic su una salva o un proiettile per seguirlo nello scafo

## ui.armor.plates
ui-armor-plates-heading = Piastre colpite ({ $count })
ui-armor-plates-arrival = { $velocity } m/s all'impatto, { $time } s di volo, in caduta a { $fall } deg
ui-armor-plates-overmatch = Overmatch: { $penetration } mm di penetrazione a { $velocity } m/s
ui-armor-plates-penetrated = Penetra: { $penetration } mm contro { $effective } mm effettivi a { $velocity } m/s
ui-armor-plates-ricochet = Rimbalzo a { $angle } deg
ui-armor-plates-shatter = Si frantuma: { $penetration } mm contro { $effective } mm effettivi
ui-armor-plates-fuse_armed = Spoletta armata
ui-armor-plates-detonation = Esplode in { $zone }, { $distance } m dopo la piastra #{ $number }
ui-armor-plates-detonation_past_armor = Esplode oltre ogni corazza, { $distance } m dopo la piastra #{ $number }
ui-armor-plates-flat_penetrates = { $ammo } penetra: { $penetration } mm contro { $thickness } mm
ui-armor-plates-flat_shatters = { $ammo } si frantuma: { $penetration } mm contro { $thickness } mm
ui-armor-plates-server_angle = server { $angle } gradi

# -- Etichette livello vantaggio --

## advantage
advantage-absolute = Assoluto
advantage-strong = Forte
advantage-moderate = Moderato
advantage-weak = Debole

## ui.search
ui-search-undo = Annulla l'ultima modifica alla query
ui-search-edit_value = Modifica valore
ui-search-add_filter = Aggiungi filtro
ui-search-redo = Ripeti la modifica annullata
ui-search-pill_select = Seleziona
ui-search-pill_deselect = Deseleziona
ui-search-pill_group_all = Raggruppa con and
ui-search-pill_group_any = Raggruppa con or
ui-search-pill_ungroup = Separa gruppo
ui-search-pill_negate = Nega
ui-search-pill_flip_connector = Inverti and/or
ui-search-pill_delete = Elimina
ui-search-match_count_truncated = { $count }+ partite
ui-search-outcome_win = Vittoria
ui-search-outcome_loss = Sconfitta
ui-search-outcome_draw = Pareggio
ui-search-outcome_unknown = Sconosciuto
ui-search-bool_true = vero
ui-search-bool_false = falso
ui-search-match_count = { $count } partite
ui-search-row_actions_hint = Azioni replay (o clic destro sulla riga)
ui-search-looking_up_values = Ricerca dei valori...
ui-search-type_a_query = Digita una query e premi Invio
ui-search-parse_failed = Query non valida: { $reason }
ui-search-searching = Ricerca in corso...
ui-search-failed = Ricerca non riuscita: { $reason }
ui-search-no_matches = Nessun risultato
ui-search-game_mode_gap_hint_one = 1 partita indicizzata non ha la modalità di gioco registrata. Usa Reindicizza tutti i replay nelle Impostazioni per completarla.
ui-search-game_mode_gap_hint = { $count } partite indicizzate non hanno la modalità di gioco registrata. Usa Reindicizza tutti i replay nelle Impostazioni per completarle.
ui-search-open = Apri
ui-search-open_missing = Il file del replay non esiste più
ui-search-opened_directory = Aperta { $dir }, che contiene questo replay
ui-search-open_no_directory = Impossibile stabilire a quale cartella appartiene questo replay
ui-search-path_copied = Percorso del replay copiato negli appunti
ui-search-preview_map = Anteprima mappa
ui-search-preview_and_render = Passa il mouse per l'anteprima, clicca per il rendering
ui-search-free_text = contiene "{ $text }"
ui-search-quant_any = qualsiasi
ui-search-quant_none = nessun
ui-search-quant_ge = { $n } o più
ui-search-quant_eq = esattamente { $n }
ui-search-quant_le = al massimo { $n }
ui-search-quant_gt = più di { $n }
ui-search-quant_lt = meno di { $n }
ui-search-quant_ne = non { $n }
ui-search-scope_self = Mio
ui-search-scope_ally = Alleato
ui-search-scope_enemy = Nemico
ui-search-scope_div = Div

## ui.search.suggest
ui-search-suggest-preset_divmate_test_ship = Qualcuno nella mia divisione ha usato una nave test
ui-search-suggest-preset_no_enemy_cv = Nessuna portaerei nemica
ui-search-suggest-preset_all_enemies_died = Tutti i nemici sono affondati
ui-search-suggest-preset_high_damage_enemies = 3 o più nemici sopra i 100k
ui-search-suggest-preset_stream_sniper = Contiene uno stream sniper
ui-search-suggest-preset_i_survived = Sono sopravvissuto
ui-search-suggest-preset_i_disconnected = Mi sono disconnesso
ui-search-suggest-my_damage = I miei danni
ui-search-suggest-enemy_ship = Nave nemica
ui-search-suggest-allied_ship = Nave alleata
ui-search-suggest-player_in_match = Il giocatore era in partita
ui-search-suggest-someone_in_division = Qualcuno nella mia divisione
ui-search-suggest-scope_self = Mio
ui-search-suggest-scope_ally = Alleato
ui-search-suggest-scope_enemy = Nemico
ui-search-suggest-scope_division = Divisione
ui-search-suggest-scope_anyone = Qualsiasi giocatore

## ui.search.bar
ui-search-bar-hint = Filtra i replay
ui-search-bar-and_word = e
ui-search-bar-or_word = o
ui-search-bar-not_word = non
ui-search-bar-group_and = Raggruppa con E
ui-search-bar-group_or = Raggruppa con O
ui-search-bar-negate = Nega
ui-search-bar-delete = Elimina
ui-search-bar-clear_selection = Cancella selezione
ui-search-bar-ungroup = Clicca per sciogliere questo gruppo
ui-search-bar-flip_connector = Clicca per scambiare E e O
ui-search-bar-context_preset = Preimpostazione
ui-search-bar-context_match = Partita
ui-search-bar-context_roster = Giocatori
ui-search-bar-no_suggestions = Nessun risultato

## ui.search.field
ui-search-field-outcome = Risultato
ui-search-field-map = Mappa
ui-search-field-class = Classe
ui-search-field-tier = Livello
ui-search-field-date = Data
ui-search-field-player_present = Giocatore
ui-search-field-group = Gruppo replay
ui-search-field-stat_damage = Danni
ui-search-field-stat_kills = Eliminazioni
ui-search-field-stat_spotting = Avvistamento
ui-search-field-stat_potential = Danni potenziali
ui-search-field-stat_received = Danni ricevuti
ui-search-field-stat_pr = PR
ui-search-field-stat_mc = MC
ui-search-field-rating = Valutazione
ui-search-field-stat_survived = Sopravvissuto
ui-search-field-stat_disconnected = Disconnesso
ui-search-field-game_type = Tipo di partita
ui-search-field-game_mode = Modalità di gioco
ui-search-field-match_group = Gruppo partita
ui-search-field-build = Build
ui-search-field-results_available = Risultati disponibili
ui-search-field-relation = Relazione
ui-search-field-division = Divisione
ui-search-field-name = Nome
ui-search-field-clan = Clan
ui-search-field-realm = Regione
ui-search-field-ship = Nave
ui-search-field-ship_index = Indice nave
ui-search-field-nation = Nazione
ui-search-field-test_ship = Nave test
ui-search-field-stream_sniper = Stream sniper
ui-search-field-sniper_login = Login Twitch dello sniper

## ui.search.op
ui-search-op-contains = contiene
ui-search-op-equals = uguale a
ui-search-op-not_equals = diverso da
ui-search-op-eq = =
ui-search-op-ne = !=
ui-search-op-gt = >
ui-search-op-ge = >=
ui-search-op-lt = <
ui-search-op-le = <=
ui-search-op-is = è
ui-search-op-is_not = non è
ui-search-op-is_set = è impostato
ui-search-op-is_not_set = non è impostato

## ui.search.value
ui-search-value-relation_self = Io
ui-search-value-relation_ally = Alleato
ui-search-value-relation_enemy = Nemico
ui-search-value-division_mine = La mia
ui-search-value-division_any = Qualsiasi
ui-search-value-division_none = Nessuna
ui-search-value-class_air_carrier = Portaerei
ui-search-value-class_battleship = Corazzata
ui-search-value-class_cruiser = Incrociatore
ui-search-value-class_destroyer = Cacciatorpediniere
ui-search-value-class_submarine = Sottomarino
ui-search-value-class_auxiliary = Ausiliaria
ui-search-value-rating_bad = Scarso
ui-search-value-rating_below_average = Sotto la media
ui-search-value-rating_average = Nella media
ui-search-value-rating_good = Buono
ui-search-value-rating_very_good = Molto buono
ui-search-value-rating_great = Ottimo
ui-search-value-rating_unicum = Unicum
ui-search-value-rating_super_unicum = Super Unicum
ui-search-value-game_mode_test = Test
ui-search-value-game_mode_standard = Standard
ui-search-value-game_mode_singlebase = Base singola
ui-search-value-game_mode_domination = Dominio
ui-search-value-game_mode_tutorial = Tutorial
ui-search-value-game_mode_megabase = Megabase
ui-search-value-game_mode_forts = Forti
ui-search-value-game_mode_standard_domination = Dominio standard
ui-search-value-game_mode_epicenter = Epicentro
ui-search-value-game_mode_assault_defense = Difesa d'assalto
ui-search-value-game_mode_pve = PVE
ui-search-value-game_mode_arms_race = Corsa agli armamenti
ui-search-value-game_mode_epicenter_ring = Anello epicentro
ui-search-value-game_mode_anti_standard = Anti-standard
ui-search-value-game_mode_attack_defense = Attacco e difesa
ui-search-value-game_mode_torpedo_beat = Ritmo siluri
ui-search-value-game_mode_team_battle_royale = Battle royale a squadre
ui-search-value-game_mode_escape_to_portal = Fuga verso il portale
ui-search-value-game_mode_domination_asymm = Dominio asimmetrico
ui-search-value-game_mode_key_battle = Battaglia chiave
ui-search-value-game_mode_portal_2021 = Portale 2021
ui-search-value-game_mode_team_battle_royale_2021 = Battle royale a squadre 2021
ui-search-value-game_mode_convoy_event = Evento convoglio
ui-search-value-game_mode_convoy_airship = Convoglio dirigibile
ui-search-value-game_mode_two_teams_battle_royale = Battle royale a due squadre
ui-search-value-game_mode_pinata_event = Evento pignatta
ui-search-value-game_mode_respawns = Respawn
ui-search-value-game_mode_respawns_sectors = Respawn a settori


## ui.search.column
ui-search-column-date = Data
ui-search-column-map = Mappa
ui-search-column-mode = Modalità
ui-search-column-ship = La tua nave
ui-search-column-result = Risultato
ui-search-column-damage = Danni
ui-search-column-kills = Eliminazioni
ui-search-column-pr = PR
ui-search-column-mc = MC

## ui.settings.index
ui-settings-index-heading = Indice dei replay
ui-settings-index-description = L'archivio ricercabile di ogni replay nella tua cartella
ui-settings-index-build = Crea indice
ui-settings-index-rebuild = Reindicizza tutto
ui-settings-index-rebuild_hint = Rilegge ogni replay, per correggere le righe analizzate male in precedenza. Crea indice legge solo ciò che manca.
ui-settings-index-progress = { $done } di { $total } letti, { $failed } non leggibili
ui-settings-index-progress_skipped = { $done } di { $total } letti, { $skipped } già presenti, { $failed } non leggibili
ui-settings-index-built = Indicizzati { $indexed } replay, { $failed } non leggibili
ui-settings-index-built_with_skipped = Indicizzati { $indexed } replay, { $skipped } già presenti, { $failed } non leggibili

## ui.settings
ui-settings-loading = Caricamento impostazioni...
ui-settings-load_failed = Impossibile caricare le impostazioni
ui-settings-not_set = (non impostato)

## ui.settings.panel
ui-settings-panel-mcp = Server MCP
ui-settings-panel-open = Apri impostazioni
ui-settings-panel-search_placeholder = Cerca nelle impostazioni
ui-settings-panel-count_one = 1 impostazione
ui-settings-panel-count = { $count } impostazioni
ui-settings-panel-no_matches = Nessuna impostazione corrisponde alla ricerca
ui-settings-panel-close = Chiudi impostazioni
ui-settings-panel-in_error = { $name }: un'impostazione richiede attenzione
ui-settings-panel-general = Generale
ui-settings-panel-appearance = Aspetto
ui-settings-panel-game = World of Warships
ui-settings-panel-replays = Replay
ui-settings-panel-collaboration = Collaborazione
ui-settings-panel-twitch = Twitch
ui-settings-panel-language_description = Lingua dell'interfaccia
ui-settings-panel-data_sharing_description = Cosa viene inviato a ShipBuilds.com dopo le battaglie classificate e casuali
ui-settings-panel-code_integrity_description = Se il codice non firmato da Microsoft può essere caricato in questa applicazione
ui-settings-panel-check_for_updates_description = Cerca una versione più recente a ogni avvio dell'applicazione
ui-settings-panel-data_folder = Cartella dati dell'app
ui-settings-panel-theme_description = Colori dell'interfaccia. Passa il puntatore su una scelta per vederne l'anteprima.
ui-settings-panel-zoom_description = Dimensione di tutto il testo e dei controlli
ui-settings-panel-zoom = Zoom
ui-settings-panel-zoom_shortcuts = { $zoom_in } ingrandisce, { $zoom_out } riduce, { $reset } torna a 1x
ui-settings-panel-zoom_changed = Zoom: { $factor }x
ui-settings-panel-cache_directory = Directory della cache
ui-settings-panel-cache_directory_description = Dove sono memorizzate le build nella cache
ui-settings-panel-cached_builds = Build nella cache
ui-settings-panel-current_path_description = Il file replay aperto più di recente
ui-settings-panel-show_raw_xp_description = Aggiungi una colonna XP grezza alla tabella dei risultati
ui-settings-panel-show_observed_damage_description = Aggiungi una colonna danni osservati alla tabella dei risultati
ui-settings-panel-show_entity_id_description = Aggiungi una colonna ID entità alla tabella dei risultati
ui-settings-panel-show_heals_description = Aggiungi una colonna cure alla tabella dei risultati
ui-settings-panel-auto_export_description = Scrivi i dati di ogni battaglia in un file nel formato e nella directory scelti
ui-settings-panel-display_name_description = Il nome che gli altri vedono in una sessione
ui-settings-panel-get_token_description = Apri una pagina di accesso che rilascia un token per la chat
ui-settings-panel-token = Token di accesso
ui-settings-panel-monitored_channel_description = Canale da seguire. Lascia vuoto per seguire il tuo.
ui-settings-panel-armor_defaults = Predefiniti salvati
ui-settings-panel-armor_saved_description = Salvati dalle opzioni del Visualizzatore modelli

## ui.settings.licenses
ui-settings-licenses-title = Informazioni e licenze
ui-settings-licenses-back = Torna alle impostazioni
ui-settings-licenses-third_party_intro = Questa applicazione contiene i componenti di terze parti elencati di seguito. Seleziona una voce per mostrare i pacchetti che la usano e il testo completo della licenza.

## ui.settings.armor
ui-settings-armor-no_defaults = (nessun predefinito salvato)

## ui.app
ui-app-status_cache_checking = Controllo della cache dei dati di gioco
ui-app-status_cache_validating = Verifica della cache dei dati di gioco
ui-app-status_cache_downloading = Download dei dati di gioco
ui-app-status_cache_planning = Richiesta dei dati di gioco pubblicati
ui-app-drop_to_load = Rilascia per caricare { $file }
ui-app-drop_to_list = Rilascia per elencare i replay in { $folder }
ui-app-drop_to_list_replays = Rilascia per elencare { $count } replay
ui-app-drop_to_list_folders = Rilascia per elencare i replay in { $count } cartelle
ui-app-drop_to_open_and_list = Rilascia per aprire { $file } ed elencare i replay in { $folders }
ui-app-drop_to_list_replays_and_folders = Rilascia per elencare { $count } replay e i replay in { $folders }
ui-app-drop_folder_count = { $count } cartelle
ui-app-reading_replay = Lettura di { $replay }
ui-app-reading_replays = Lettura di { $count } replay
ui-app-status_constants_generating = Aggiornamento delle costanti di gioco per { $version }
ui-app-worker_constants = Il processo delle costanti di gioco
ui-app-worker_live_match = Il monitoraggio della battaglia
ui-app-worker_replay_loader = Il caricatore dei replay
ui-app-worker_restarting = { $worker } si è arrestato e si sta riavviando
ui-app-worker_stalled = { $worker } non risponde
ui-app-worker_stopped = { $worker } si è arrestato
ui-app-status_indexing = Indicizzazione dei replay
ui-app-status_of = { $done } di { $total }
ui-app-status_tasks = Attività in corso: { $count }
ui-app-status_show_panel = Mostra pannello: { $panel }
ui-app-status_hide_panel = Nascondi pannello: { $panel }
ui-app-panel_left = Sinistra
ui-app-panel_right = Destra
ui-app-panel_bottom = In basso
ui-app-debug_build = Build di debug

## ui.palette
ui-palette-title = Comandi
ui-palette-placeholder = Digita un comando o cerca...
ui-palette-go_to = Vai a: { $tab }
ui-palette-advanced_search = Ricerca avanzata...
ui-palette-set_theme = Tema: { $theme }
ui-palette-games_i_died_in = Partite in cui sono affondato
ui-palette-games_i_won = Partite vinte
ui-palette-send_all_replays = Invia tutti i replay a ShipBuilds
ui-palette-send_all_replays_again = Invia tutti i replay a ShipBuilds (ignora il registro)
ui-palette-search_players = Cerca giocatori...
ui-palette-my_matches_in_ship = Le mie partite con la nave...
ui-palette-open_ship_in_armor_viewer = Apri nave nel Visualizzatore modelli...
ui-palette-open_ship_in_shiptool = Apri nave in ShipTool...
ui-palette-players_title = Cerca giocatori
ui-palette-my_ships_title = Le mie partite con la nave
ui-palette-armor_ships_title = Apri nave nel Visualizzatore modelli
ui-palette-shiptool_ships_title = Apri nave in ShipTool
ui-palette-players_placeholder = Cerca giocatori per nome...
ui-palette-my_ships_placeholder = Cerca le navi che hai giocato...
ui-palette-armor_ships_placeholder = Cerca qualsiasi nave per nome...
ui-palette-armor_ship_row = { $name } -- { $class } di livello { $tier }, { $nation }
ui-palette-back_hint = Esc: indietro
ui-palette-ships_loading = Caricamento elenco navi...
ui-palette-ships_unavailable = Nessun elenco navi: { $reason }
ui-palette-matches_found = { $count } risultati
ui-palette-nothing_indexed = Ancora nulla di indicizzato
ui-palette-sample_stats = Debug: mostra dati di esempio in Statistiche
ui-palette-restore_stats = Debug: mostra dati registrati in Statistiche

## ui.replay.hover
ui-replay-hover-ship = Nave
ui-replay-hover-map = Mappa
ui-replay-hover-mode = Modalità
ui-replay-hover-blank_mode = Vuota
ui-replay-hover-played = Giocata
ui-replay-hover-damage = Danni
ui-replay-hover-kills = Eliminazioni
ui-replay-hover-result = Risultato
ui-replay-hover-division = Divisione
ui-replay-hover-twitch = Chat di Twitch

## ui.replay.renderer
ui-replay-renderer-export_video = Esporta in video
ui-replay-renderer-export_clipboard = Esporta negli appunti
ui-replay-renderer-export_saved = Video salvato in { $path }
ui-replay-renderer-export_copied = Video copiato negli appunti
ui-replay-renderer-export_not_written = Impossibile esportare il video: { $reason }
ui-replay-renderer-pop_out = Apri in una finestra separata
ui-replay-renderer-export_path_unusable = Impossibile scrivere in quel percorso
ui-replay-renderer-batch_started = Rendering di { $count } replay in { $dir }
ui-replay-renderer-batch_finished = Renderizzati { $written } replay; { $failed } non renderizzabili
ui-replay-renderer-batch_all_written = Renderizzati { $written } replay
ui-replay-renderer-batch_copied = Renderizzati { $written } replay e copiati negli appunti
ui-replay-renderer-batch_not_copied = Renderizzati in { $dir }, ma gli appunti li hanno rifiutati: { $reason }
ui-replay-renderer-batch_progress = Rendering { $done } di { $total }: { $replay }
ui-replay-renderer-baking = Lettura del replay e ricostruzione della battaglia...
ui-replay-renderer-failed = Impossibile riprodurre questo replay
ui-replay-renderer-play = Riproduci
ui-replay-renderer-pause = Pausa

## ui.renderer.annotations
ui-renderer-annotations-title = Strumenti di disegno
ui-renderer-annotations-undo = Annulla
ui-renderer-annotations-redo = Ripeti
ui-renderer-annotations-ship_hint = Trova una nave
ui-renderer-annotations-no_ship = Nessuna nave scelta
ui-renderer-annotations-nib = Spessore tratto
ui-renderer-annotations-thinner = Tratto più sottile
ui-renderer-annotations-thicker = Tratto più spesso
ui-renderer-annotations-shortcuts_title = Scorciatoie da tastiera
ui-renderer-annotations-shortcuts_tools = Strumenti
ui-renderer-annotations-shortcuts_actions = Azioni
ui-renderer-annotations-tool_select = Seleziona
ui-renderer-annotations-tool_arrow = Freccia
ui-renderer-annotations-tool_freehand = Mano libera
ui-renderer-annotations-tool_line = Linea
ui-renderer-annotations-tool_circle = Cerchio
ui-renderer-annotations-tool_rectangle = Rettangolo
ui-renderer-annotations-tool_triangle = Triangolo
ui-renderer-annotations-tool_measurement = Misura
ui-renderer-annotations-tool_eraser = Gomma
ui-renderer-annotations-action_undo = Annulla
ui-renderer-annotations-action_multi_select = Selezione multipla
ui-renderer-annotations-action_stroke_width = Spessore tratto
ui-renderer-annotations-action_delete = Elimina
ui-renderer-annotations-action_cancel = Annulla / Deseleziona
ui-renderer-annotations-toggle_fill = Fai di nuovo clic per attivare o disattivare il riempimento
ui-renderer-annotations-clear_all = Cancella tutto
ui-renderer-annotations-edit = Modifica disegno
ui-renderer-annotations-size = Dimensione
ui-renderer-annotations-color = Colore
ui-renderer-annotations-custom_ink = Colore personalizzato
ui-renderer-annotations-filled = Riempito
ui-renderer-annotations-locked = Annotazioni bloccate dall'host

## ui.armor.cast
ui-armor-cast-prompt = { $key }+clic sullo scafo per aggiungere un punto di impatto
ui-armor-cast-empty = Ancora nessun punto di impatto
ui-armor-cast-miss_nothing_under_pointer = Nessuna piastra corazzata sotto il puntatore
ui-armor-cast-miss_thickness_unknown = Le piastre di questo modello non hanno uno spessore noto contro cui sparare
ui-armor-cast-miss_no_plate_on_path = La traiettoria del proiettile da questo angolo non attraversa alcuna piastra corazzata

## ui.armor.pen
ui-armor-pen-search_hint = Cerca navi da confrontare...
ui-armor-pen-add_a_ship = Cerca e aggiungi navi qui sopra per confrontare la penetrazione
ui-armor-pen-hover_a_plate = Passa il mouse su una piastra della nave per controllarla
ui-armor-pen-awaiting_plate = passa su una piastra
ui-armor-pen-clear_all = Cancella tutto
ui-armor-pen-remove = Rimuovi dal confronto
ui-armor-pen-title = Penetrazione
ui-armor-pen-title_tooltip = Controlla l'effetto dei proiettili di una nave su una piastra
ui-armor-pen-ship = Nave che spara
ui-armor-pen-firing_ship_tooltip = Usa i proiettili di questa nave per le traiettorie
ui-armor-pen-use_for_trajectory = Usa per la traiettoria
ui-armor-pen-pick_ship = Scegli la nave che spara contro questa
ui-armor-pen-enter_thickness = Inserisci lo spessore della piastra
ui-armor-pen-no_shells = { $ship } non ha proiettili della batteria principale
ui-armor-pen-plate = Piastra
ui-armor-pen-mm = mm
ui-armor-pen-ifhe = IFHE
ui-armor-pen-ifhe_tooltip = Spoletta inerziale per proiettili HE aumenta di un quarto la penetrazione HE
ui-armor-pen-last_hovered = Ultima indicata: { $plate }
ui-armor-pen-penetrates = penetra
ui-armor-pen-shatters = si frantuma
ui-armor-pen-angle_dependent = dipende dall'angolo
ui-armor-pen-no_data = nessun dato di penetrazione

## ui.armor.splash
ui-armor-splash-hint = Clicca sulla corazza in modalità splash per vedere quali zone raggiunge un'esplosione
ui-armor-splash-half_extent = Semiampiezza dell'esplosione: { $value } unità del modello
ui-armor-splash-triangles = Triangoli di corazza nell'esplosione: { $total }, penetrati: { $penetrated }
ui-armor-splash-no_zones = Nessun volume splash si sovrappone in questo punto
ui-armor-splash-zones = Zone nell'esplosione
ui-armor-splash-direct_hit = Colpo diretto
ui-armor-splash-hp = { $value } PS
ui-armor-splash-section = Zone splash

## ui.armor.export
ui-armor-export-hull = Scafo
ui-armor-export-contents = Contenuto
ui-armor-export-contents_mesh = Mesh
ui-armor-export-contents_armor = Corazza
ui-armor-export-contents_both = Mesh e corazza
ui-armor-export-lod = Dettaglio mesh
ui-armor-export-lod_tooltip = 0 è il dettaglio massimo. La corazza non ha livelli di dettaglio e non viene influenzata.
ui-armor-export-res = Risoluzione texture
ui-armor-export-res_full = Piena
ui-armor-export-res_2048 = 2048 px
ui-armor-export-res_1024 = 1024 px
ui-armor-export-res_512 = 512 px
ui-armor-export-camos = Mimetizzazioni
ui-armor-export-camo_select_none = Deseleziona tutto
ui-armor-export-camo_reset = Ripristina predefinite
ui-armor-export-loading_details = Caricamento...
ui-armor-export-size_estimate = Dimensione stimata del file: { $size }
ui-armor-export-size_breakdown = Geometria { $geometry } (esatta), texture { $textures } (stimate)
ui-armor-export-size_incomplete = Impossibile stimare { $count } mimetizzazioni; la dimensione potrebbe essere incompleta
ui-armor-export-exporting = Esportazione di { $ship }...
ui-armor-export-close_button = Chiudi
ui-armor-export-already_running = Un'esportazione è già in corso
ui-armor-export-exported = Esportata { $ship }
ui-armor-export-exported_with_size = Esportata { $ship } ({ $size })
ui-armor-export-vehicle_not_found = Nave non trovata
ui-armor-export-create_file_failed = Impossibile creare il file: { $error }
ui-armor-export-export_failed = Esportazione non riuscita: { $error }

## ui.tactics
ui-tactics-a_peers_board = Una tavola condivisa da un altro partecipante della sessione
ui-tactics-map = Mappa
ui-tactics-map_hint = Trova una mappa
ui-tactics-mode = Modalità
ui-tactics-pick_a_map = Scegli una mappa su cui disegnare
ui-tactics-drawing = Disegno della mappa...
ui-tactics-reset_view = Reimposta vista
ui-tactics-add_cap = Aggiungi punto
ui-tactics-add_cap_tooltip = Posiziona un punto di conquista con il prossimo clic sulla mappa
ui-tactics-selected_cap = Punto { $letter }
ui-tactics-cap_radius = { $km } km
ui-tactics-cap_team = Squadra
ui-tactics-cap_team_tooltip = Cambia la squadra del punto di conquista selezionato
ui-tactics-team_neutral = Neutrale
ui-tactics-team_one = Squadra 1 (verde)
ui-tactics-team_two = Squadra 2 (rossa)
ui-tactics-delete_cap = Elimina
ui-tactics-delete_cap_tooltip = Rimuovi il punto di conquista selezionato
ui-tactics-cap_replay_delete_tooltip = Un punto di conquista caricato da un replay non può essere eliminato
ui-tactics-cap_narrow_tooltip = Riduci il raggio del punto di conquista selezionato
ui-tactics-cap_widen_tooltip = Aumenta il raggio del punto di conquista selezionato
ui-tactics-undo_tooltip = Annulla l'ultima modifica alla tavola ({ $shortcut })
ui-tactics-redo_tooltip = Ripeti l'ultima modifica annullata ({ $shortcut } o { $alternate })
ui-tactics-clear_caps = Rimuovi punti aggiunti
ui-tactics-clear_added_caps_tooltip = Rimuovi i punti di conquista aggiunti dall'utente
ui-tactics-preset_name = Nome tavola
ui-tactics-preset_save = Salva
ui-tactics-preset_delete = Elimina
ui-tactics-reset_board = Reimposta tavola
ui-tactics-reset_board_tooltip = Ripristina i punti della modalità selezionata e cancella i disegni
ui-tactics-preset_needs_a_name = Dai prima un nome alla tavola
ui-tactics-preset_needs_a_map = Imposta prima una mappa per la tavola
ui-tactics-preset_saved = Salvata { $name }
ui-tactics-preset_save_failed = Impossibile salvare la tavola: { $error }
ui-tactics-preset_load_failed = Impossibile aprire la tavola: { $error }
ui-tactics-preset_delete_failed = Impossibile eliminare la tavola: { $error }
ui-tactics-presets = Tavole salvate
ui-tactics-preset_open = Apri
ui-tactics-clear_drawing = Cancella disegno
ui-tactics-scan_replays = Ricava punti dai replay
ui-tactics-scan_replays_tooltip = Legge i replay per ricavare la disposizione dei punti di conquista delle modalità giocate
ui-tactics-scan_needs_game_data = Carica prima una build di gioco
ui-tactics-scan_running = Lettura dei replay...
ui-tactics-scan_progress = Lettura dei replay... { $read } / { $total }
ui-tactics-scan_done = Letti { $total } replay, trovate { $added } nuove disposizioni
ui-tactics-session_boards_full = La sessione ha già { $limit } lavagne aperte, quindi questa lavagna resta sulla sua mappa
ui-tactics-placing = Posizionamento di { $ship }
ui-tactics-side_friendly = Alleato
ui-tactics-side_enemy = Nemico
ui-tactics-ink_white = Inchiostro bianco
ui-tactics-ink_gray = Inchiostro grigio
ui-tactics-ink_red = Inchiostro rosso
ui-tactics-ink_orange = Inchiostro arancione
ui-tactics-ink_yellow = Inchiostro giallo
ui-tactics-ink_green = Inchiostro verde
ui-tactics-ink_blue = Inchiostro blu
ui-tactics-ink_purple = Inchiostro viola
ui-tactics-ink_pink = Inchiostro rosa
ui-tactics-no_map_art = Questa build non include la grafica di quella mappa
ui-tactics-ship_config = Configurazione nave
ui-tactics-hull = Scafo
ui-tactics-hull_default = Predefinito
ui-tactics-modifiers = Modificatori
ui-tactics-concealment_expert = Esperto di occultamento
ui-tactics-gun_range_mod = Mod. gittata cannoni
ui-tactics-secondary_range_mod = Mod. gittata secondaria
ui-tactics-ship_ranges = Cerchi di gittata
ui-tactics-shape = Forma
ui-tactics-shape_size = Dimensione
ui-tactics-shape_size_km = Dimensione (km)
ui-tactics-shape_filled = Piena
ui-tactics-shape_smaller = Riduci la forma
ui-tactics-shape_larger = Ingrandisci la forma

## ui.armor.outcome
ui-armor-outcome-penetration = Penetrazione
ui-armor-outcome-citadel = Cittadella
ui-armor-outcome-ricochet = Rimbalzo
ui-armor-outcome-shatter = Frantumazione
ui-armor-outcome-overpenetration = Perforazione eccessiva
ui-armor-outcome-underwater = Subacqueo
ui-armor-outcome-unknown = Non riconosciuto ({ $raw })
ui-armor-outcome-stopped = Fermato nella corazza

## ui.armor.sim
ui-armor-sim-overmatch_rules_out_ricochet = Overmatch, quindi nessun rimbalzo
ui-armor-sim-always_ricochet_band = Rimbalzo, nella fascia di rimbalzo garantito
ui-armor-sim-ricochet_angle_too_low = Rimbalzo impossibile, angolo troppo basso

## ui.launcher
ui-launcher-rescan = Ripeti scansione
ui-launcher-add_install = Aggiungi installazione...
ui-launcher-add_install_title = Cartella di installazione di World of Warships
ui-launcher-no_installs = Nessuna installazione trovata. Aggiungine una o installa il gioco qui sotto.
ui-launcher-managed_by_steam = gestita da Steam
ui-launcher-checking_updates = controllo aggiornamenti
ui-launcher-up_to_date = Aggiornato
ui-launcher-update_available = Aggiornamento disponibile: { $parts }
ui-launcher-not_installed = non installato: { $parts }
ui-launcher-check_failed = controllo aggiornamenti non riuscito: { $error }
ui-launcher-recheck = Ricontrolla
ui-launcher-download = Scarica / Aggiorna...
ui-launcher-download_title = Dove installare la build
ui-launcher-download_hint = Una nuova installazione occupa decine di GB; gli aggiornamenti delta non sono ancora supportati, quindi una parte modificata viene riscaricata per intero.
ui-launcher-pause = Pausa
ui-launcher-cancel = Annulla
ui-launcher-resume = Riprendi
ui-launcher-paused = Download in pausa; i file già scaricati vengono conservati.
ui-launcher-installed = Installato
ui-launcher-installed_skipped = Installato ({ $count } parti delta saltate: non ancora supportate, scaricate ma non applicate)
ui-launcher-download_failed = Download non riuscito: { $error }
ui-launcher-update_in_place = Un aggiornamento va nella cartella dell'installazione stessa. Scegli { $path } per aggiornarla.
ui-launcher-cancelled = Download annullato.
ui-launcher-launch_failed = Avvio non riuscito: { $error }
ui-launcher-use_for_toolkit = Usa per il toolkit
ui-launcher-not_set = non impostato
ui-launcher-browse = Sfoglia...
ui-launcher-install_heading = Installa un gioco
ui-launcher-install_hint = Una nuova installazione occupa decine di GB.
ui-launcher-install_title = Dove installare il gioco
ui-launcher-install = Installa...
ui-launcher-checking_version = controllo versione disponibile
ui-launcher-version = versione { $version }
ui-launcher-version_unreported = versione client non indicata
ui-launcher-unavailable = non disponibile: { $error }
ui-launcher-retry = Riprova

## ui.launcher.account
ui-launcher-account-sign_in = Accedi
ui-launcher-account-add_title = Aggiungi account
ui-launcher-account-again_title = Accedi di nuovo
ui-launcher-account-memory_only = Accesso effettuato solo per questa sessione. Non viene salvato, quindi dovrai accedere di nuovo dopo un riavvio.
ui-launcher-account-password_note = La tua password viene inviata solo a Wargaming e non viene mai memorizzata.
ui-launcher-account-realm_fixed = Regione: { $realm }
ui-launcher-account-email = Email
ui-launcher-account-password = Password
ui-launcher-account-code = Codice di autenticazione a due fattori
ui-launcher-account-code_required = Codice di autenticazione a due fattori richiesto.
ui-launcher-account-use_backup_code = Usa codice di backup
ui-launcher-account-submit_code = Invia codice
ui-launcher-account-captcha = Questo accesso richiede un CAPTCHA, che questo launcher non può ancora mostrare. Accedi una volta tramite il Wargaming Game Center ufficiale o wargaming.net in un browser, poi riprova.

## ui.launcher.sign_in_error
ui-launcher-sign_in_error-wrong_credentials = E-mail o password errata.
ui-launcher-sign_in_error-two_factor_required = Questo account richiede l'approvazione a due fattori. Approva l'accesso in Wargaming Auth o con il codice ricevuto via e-mail, poi accedi di nuovo.
ui-launcher-sign_in_error-two_factor_invalid = Il codice a due fattori è errato o scaduto. Inserisci un nuovo codice.
ui-launcher-sign_in_error-captcha_required = Wargaming richiede un CAPTCHA. Accedi una volta dal Game Center ufficiale o su wargaming.net, poi riprova.
ui-launcher-sign_in_error-rate_limited = Troppi tentativi di accesso. Attendi qualche minuto, poi riprova.
ui-launcher-sign_in_error-account_banned = Questo account è bannato.
ui-launcher-sign_in_error-network_unreachable = Impossibile raggiungere Wargaming. Controlla la connessione a Internet.
ui-launcher-sign_in_error-service_unavailable = Il servizio di accesso di Wargaming non è disponibile. Riprova più tardi.
ui-launcher-sign_in_error-session_expired = L'accesso salvato è scaduto. Accedi di nuovo.
ui-launcher-sign_in_error-keyring_locked = Il portachiavi di sistema è bloccato. Sbloccalo, poi accedi di nuovo.
ui-launcher-sign_in_error-not_saved = Accesso effettuato, ma non è stato possibile salvarlo. Dovrai accedere di nuovo dopo un riavvio.
ui-launcher-sign_in_error-unexpected = Accesso non riuscito. I dettagli sono nel log.

## ui.launcher.realm
ui-launcher-realm-eu = Europa
ui-launcher-realm-na = Nord America
ui-launcher-realm-asia = Asia

## ui.launcher.runner
ui-launcher-runner-looking = Ricerca di Proton.
ui-launcher-runner-steam_proton = { $name } di Steam
ui-launcher-runner-umu = umu-run con { $name }
ui-launcher-runner-umu_default = umu-run con UMU-Proton
ui-launcher-runner-native = Il gioco viene eseguito in modo nativo.
ui-launcher-runner-first_run_prefix = Il primo avvio crea un prefisso Wine, operazione che richiede alcuni secondi.
ui-launcher-runner-first_run_download = Il primo avvio scarica UMU-Proton e un runtime Steam, circa 700 MB.
ui-launcher-runner-choose = Scegli una cartella di Proton
ui-launcher-runner-automatic = Trova automaticamente
ui-launcher-runner-detect = Cerca di nuovo
ui-launcher-runner-needed = Serve Proton per avviare su Linux. Installa Steam con Proton, oppure umu-launcher.

## ui.replay.damage_share
ui-replay-damage_share-mode = Danni come quota del totale dell'altra nave
ui-replay-damage_share-dealt = Il { $pct } dei danni inflitti da { $ship } è andato a { $other }.
ui-replay-damage_share-dealt_inverse = Il { $pct } dei danni subiti da { $other } è arrivato da { $ship }.
ui-replay-damage_share-received = Il { $pct } dei danni subiti da { $ship } è arrivato da { $other }.
ui-replay-damage_share-received_inverse = Il { $pct } dei danni inflitti da { $other } è andato a { $ship }.
ui-replay-damage_share-hold = Tieni premuto { $key } per invertire.
ui-replay-damage_share-release = Rilascia { $key } per tornare indietro.

## ui.reindex
ui-reindex-context_one = Forza reindicizzazione
ui-reindex-context_many = Forza reindicizzazione di { $count } replay
ui-reindex-palette_by_version = Forza reindicizzazione replay: per versione
ui-reindex-palette_all = Forza reindicizzazione replay: tutti
ui-reindex-versions_title = Forza reindicizzazione replay: per versione
ui-reindex-versions_placeholder = Filtra versioni del gioco...
ui-reindex-version_row = { $version } -- { $count } replay
ui-reindex-no_versions = Nessun replay elencato indica una versione del gioco
ui-reindex-confirm_title = Forza reindicizzazione replay
ui-reindex-confirm_ok = Reindicizza
ui-reindex-confirm_count = { $count } replay verranno riletti e le loro voci di indice riscritte.
ui-reindex-confirm_range = Giocati dal { $oldest } al { $newest }.
ui-reindex-confirm_undated = { $count } replay da leggere non hanno una data di battaglia leggibile e non rientrano in questo intervallo.
ui-reindex-confirm_downloads = Prima vengono scaricati i dati di gioco per queste versioni:
ui-reindex-confirm_unobtainable = Non è possibile ottenere i dati di gioco per queste versioni, quindi i loro { $count } replay vengono saltati:
ui-reindex-confirm_no_cache_dir = Non è impostata alcuna cartella di cache dei dati di gioco, quindi i dati di gioco per queste versioni non possono essere scaricati e i loro { $count } replay vengono saltati:
ui-reindex-confirm_no_build = { $count } replay non indicano alcuna build del gioco e vengono saltati.
ui-reindex-confirm_no_version = { $count } replay non indicano una versione del gioco leggibile e vengono saltati.
ui-reindex-build_row = { $version } (build { $build }): { $count } replay
ui-reindex-nothing_listed = L'elenco non contiene replay da reindicizzare.
ui-reindex-nothing_readable = Nessuno di questi replay può essere reindicizzato: non sono disponibili dati di gioco ({ $count } saltati).
ui-reindex-no_game_data = I replay non possono essere letti finché non è impostata la cartella del gioco.
ui-reindex-busy_index = È già in corso un'indicizzazione.
ui-reindex-busy_cache = È già in corso un'operazione sui dati di gioco. Riprova al termine.
ui-reindex-planning = Verifica dei dati di gioco disponibili...
ui-reindex-no_index = L'indice dei replay non è aperto, quindi non è stato reindicizzato nulla.
ui-reindex-done = { $indexed } replay reindicizzati; { $skipped } saltati, { $failed } non leggibili
ui-reindex-stopped = Reindicizzazione interrotta: { $indexed } reindicizzati, { $skipped } saltati, { $failed } non leggibili
ui-reindex-stopped_before_reading = Reindicizzazione interrotta prima di leggere qualsiasi replay.

## ui.index_folder
ui-index_folder-tooltip = Indicizza tutti i replay di questo elenco non ancora presenti nell'indice dei replay
ui-index_folder-confirm_title = Indicizza replay
ui-index_folder-confirm_ok = Indicizza
ui-index_folder-confirm_count = { $count } replay di questo elenco non sono ancora nell'indice dei replay e verranno letti e aggiunti.
ui-index_folder-note_live = I replay indicizzati vengono aggiunti all'indice dei replay letto da Cerca e da Tracciamento giocatori.
ui-index_folder-note_folder = I replay indicizzati vengono aggiunti all'indice dei replay sotto la fonte propria di questa cartella ({ $dir }). Cerca e l'elenco giocatori di Tracciamento giocatori li includono. L'elenco clan di Tracciamento giocatori legge solo i replay del gioco.
ui-index_folder-nothing_listed = Questo elenco non contiene replay da indicizzare.
ui-index_folder-nothing_missing = Tutti i replay di questo elenco sono già indicizzati.
ui-index_folder-unreadable_index = Impossibile leggere l'indice dei replay.
ui-index_folder-done = { $indexed } replay indicizzati; { $skipped } saltati, { $failed } non leggibili
ui-index_folder-stopped = Indicizzazione interrotta: { $indexed } indicizzati, { $skipped } saltati, { $failed } non leggibili
ui-index_folder-stopped_before_reading = Indicizzazione interrotta prima della lettura di qualsiasi replay.

## ui.replay_folders
ui-replay_folders-install = Gioco
ui-replay_folders-watched = Monitorata: i replay nuovi e modificati appaiono qui
ui-replay_folders-unwatched = Non monitorata: usa Aggiorna per rileggerla
ui-replay_folders-unseen = { $count } replay nuovi o modificati dall'ultima visualizzazione
ui-replay_folders-missing = Questa cartella non esiste più: { $dir }
ui-replay_folders-missing_hint = Verrà aperta qui a ogni avvio finché non rimuovi il fissaggio.
ui-replay_folders-pin = Fissa
ui-replay_folders-unpin = Rimuovi fissaggio
ui-replay_folders-refresh = Aggiorna
ui-replay_folders-close = Chiudi
ui-replay_folders-pins_unreadable = Impossibile leggere le cartelle replay fissate.
ui-replay_folders-game_data_available = I dati di gioco sono disponibili per i replay elencati.
ui-replay_folders-browse = Sfoglia...
ui-replay_folders-recent_title = Apri cartella replay
ui-replay_folders-recent_placeholder = Filtra cartelle recenti...
ui-replay_folders-recent_missing = Questa cartella non esiste più ed è stata rimossa dall'elenco recenti: { $dir }
ui-replay_folders-checking = Ricerca di { $dir }...
ui-replay_folders-indexing = Indicizzazione di { $dir }
ui-replay_folders-indexing_in_background = Questa cartella è ancora in fase di indicizzazione
ui-replay_folders-search_while_indexing = Questa cartella è ancora in fase di indicizzazione, quindi i risultati non sono ancora completi. Ripeti la ricerca al termine.
ui-replay_folders-source_failed = Impossibile indicizzare i replay di questa cartella: { $error }
ui-replay_folders-restart_watch = Riavvia monitoraggio
ui-replay_folders-watch_stopped = Il monitoraggio di { $folder } si è interrotto: { $cause }.
ui-replay_folders-watch_restarting = Riavvio tra { $seconds } s (tentativo { $attempt } di { $attempts }).
ui-replay_folders-watch_gave_up = Il riavvio non è riuscito { $attempts } volte e non verrà ritentato.
ui-replay_folders-watch_restart_hint = Fai clic sul punto rosso o scegli Riavvia monitoraggio dal menu della scheda per riavviarlo ora.
ui-replay_folders-watch_resumed = Il monitoraggio di { $folder } è ripreso.
ui-replay_folders-watch_cause_backend = il file system ha segnalato un errore: { $error }
ui-replay_folders-watch_cause_gone = la cartella è stata spostata o rimossa
ui-replay_folders-watch_cause_panicked = errore interno: { $error }
ui-replay_folders-watch_cause_exited = il monitoraggio è terminato in modo imprevisto
ui-replay_folders-dropped = Rilasciati
ui-replay_folders-dropped_numbered = Rilasciati { $number }
ui-replay_folders-dropped_count = { $count } replay rilasciati
ui-replay_folders-dropped_no_source = I replay rilasciati vengono indicizzati con le loro cartelle. Apri una cartella per cercarla o indicizzarla.
ui-replay_folders-unreadable_replay = Impossibile leggere: { $error }

## ui.models
ui-models-heading = Modelli
ui-models-loading = Caricamento modelli...
ui-models-unavailable = Modelli non disponibili
ui-models-search = Cerca navi e modelli...
ui-models-open = Apri
ui-models-export = Esporta modello
ui-models-copy_path = Copia percorso del modello
ui-models-path_copied = Copiato { $path }
ui-models-skin_replaces = Sostituisce { $model } quando è equipaggiato { $exterior }
ui-models-geometry_absent = Non presente nei file di gioco
ui-models-geometry_none = Questo modello non ha geometria
ui-models-hardpoints_tooltip = Mostra punti di aggancio
ui-models-common = Modelli comuni

## ui.models.group
ui-models-group-gameplay = Gioco
ui-models-group-building = Edifici
ui-models-group-character = Personaggi
ui-models-group-location = Ambientazioni
ui-models-group-port = Porti
ui-models-group-style = Stili
ui-models-group-other = Altri modelli

## ui.models.building
ui-models-building-weapon = Armi degli edifici
ui-models-building-unassigned = Edifici non assegnati
ui-models-building-unspecified = Edifici senza tipo

## ui.models.misc
ui-models-misc-crane = Gru
ui-models-misc-anchor = Ancore
ui-models-misc-deck_fitting = Attrezzature di coperta
ui-models-misc-instrument = Strumenti
ui-models-misc-propeller = Eliche
ui-models-misc-searchlight = Proiettori
ui-models-misc-boat = Imbarcazioni
ui-models-misc-flag = Bandiere
ui-models-misc-insignia = Insegne
ui-models-misc-railing = Parapetti
ui-models-misc-hatch = Boccaporti
ui-models-misc-ventilation = Ventilazione
ui-models-misc-antenna = Antenne
ui-models-misc-weapon = Armi
ui-models-misc-stowage = Stivaggio
ui-models-misc-other = Altro

## ui.models.variant
ui-models-variant-dead = Relitto
ui-models-variant-skin = Aspetto

## ui.models.kind
ui-models-kind-ship = Navi
ui-models-kind-gun = Cannoni
ui-models-kind-director = Direttori di tiro
ui-models-kind-finder = Telemetri
ui-models-kind-radar = Radar
ui-models-kind-catapult = Catapulte
ui-models-kind-aircraft = Aerei
ui-models-kind-projectile = Proiettili
ui-models-kind-misc = Varie
ui-models-kind-cap = Punti di cattura
ui-models-kind-other = Altro

## ui.models.sub_kind
ui-models-sub_kind-battleship = Corazzate
ui-models-sub_kind-cruiser = Incrociatori
ui-models-sub_kind-destroyer = Cacciatorpediniere
ui-models-sub_kind-aircraft_carrier = Portaerei
ui-models-sub_kind-submarine = Sottomarini
ui-models-sub_kind-auxiliary = Navi ausiliarie
ui-models-sub_kind-main_battery = Batteria principale
ui-models-sub_kind-secondary_battery = Batteria secondaria
ui-models-sub_kind-anti_aircraft = Contraerea
ui-models-sub_kind-torpedo_tubes = Tubi lanciasiluri
ui-models-sub_kind-depth_charge_launcher = Lanciabombe di profondità
ui-models-sub_kind-missile_launcher = Lanciamissili
ui-models-sub_kind-search_radar = Radar di ricerca
ui-models-sub_kind-fire_control_radar = Radar di direzione del tiro
ui-models-sub_kind-fighter = Caccia
ui-models-sub_kind-bomber = Bombardieri
ui-models-sub_kind-dive_bomber = Bombardieri in picchiata
ui-models-sub_kind-scout = Ricognitori
ui-models-sub_kind-torpedo = Siluri
ui-models-sub_kind-bomb = Bombe
ui-models-sub_kind-rocket = Razzi
ui-models-sub_kind-depth_charge = Bombe di profondità
ui-models-sub_kind-mine = Mine
ui-models-sub_kind-missile = Missili
ui-models-sub_kind-jato = Razzi di decollo
ui-models-sub_kind-shell = Proiettili d'artiglieria

## ui.models.part
ui-models-part-hull = Scafo
ui-models-part-main_battery = Batteria principale
ui-models-part-secondaries = Secondarie
ui-models-part-anti_aircraft = AA
ui-models-part-torpedoes = Siluri
ui-models-part-directors = Direttori di tiro
ui-models-part-finders = Telemetri
ui-models-part-radars = Radar
ui-models-part-depth_charges = Bombe di profondità
ui-models-part-aircraft = Aerei
ui-models-part-other = Altro
ui-models-part-misc = Varie

## ui.model_view
ui-model_view-thickness_from_building = Spessore da { $building }
ui-model_view-thickness_from = Spessore da { $ship } ({ $hp })
ui-model_view-thickness_unknown = Spessore sconosciuto: nessuna nave monta questo modello
ui-model_view-thickness_other_ships = Altre navi
ui-model_view-export_title = Esporta modello
ui-model_view-export_tooltip = Esporta il modello in un file GLB
ui-model_view-part_model = Modello: { $model }
ui-model_view-part_hardpoint = Punto di aggancio: { $hardpoint }

## ui.model_view.part_group
ui-model_view-part_group-hull = Scafo
ui-model_view-part_group-main_battery = Batteria principale
ui-model_view-part_group-secondary_battery = Secondarie
ui-model_view-part_group-aa_guns = AA
ui-model_view-part_group-torpedoes = Siluri
ui-model_view-part_group-depth_charges = Bombe di profondità
ui-model_view-part_group-fire_control = Controllo del tiro
ui-model_view-part_group-radar = Radar
ui-model_view-part_group-missiles = Lanciamissili
ui-model_view-part_group-decorations = Decorazioni
ui-model_view-part_group-misc = Varie
ui-model_view-part_group-armor = Corazza
ui-model_view-part_group-other = Altro

## ui.scene_problem
ui-scene_problem-cannot_show = Impossibile mostrare { $name }
ui-scene_problem-geometry_absent = La geometria di questo modello non è nei file di gioco.
ui-scene_problem-model_unreadable = Impossibile leggere i dati di questo modello. I dettagli sono nel registro.
ui-scene_problem-ship_unreadable = Impossibile leggere i dati di questa nave. I dettagli sono nel registro.
ui-scene_problem-no_meshes = Questo modello non ha mesh.
ui-scene_problem-no_meshes_at_lod = Questo modello non ha mesh al livello di dettaglio scelto.
ui-scene_problem-no_visible_surface = Questo modello non ha alcuna superficie visibile da disegnare.
ui-scene_problem-section_stand_in = Questa sezione dello scafo non ha una geometria propria. La nave disegna questa parte dello scafo in un'altra sezione.

## ui.armor.option_tabs
ui-armor-option_tabs-label = Schede opzioni
ui-armor-option_tabs-ship = Nave
ui-armor-option_tabs-layers = Livelli
ui-armor-option_tabs-analysis = Analisi
ui-armor-option_tabs-configuration = Configurazione
ui-armor-option_tabs-hull_parts = Parti dello scafo
ui-armor-option_tabs-tab_matches = { $tab }, { $count } corrispondenze
ui-armor-option_tabs-empty = Nulla in questa scheda si applica al modello mostrato.
ui-armor-option_tabs-no_choices = Questo modello non ha scafo, modulo, mimetica o livello di dettaglio da scegliere.

## ui.armor.hidden_ship
ui-armor-hidden_ship-clan_rental = Noleggio del clan
ui-armor-hidden_ship-other_class = Non è una classe di nave

## ui.armor.textures
ui-armor-textures-title = Texture
ui-armor-textures-hint = Il gioco legge un file nella cartella res_mods della build al posto del file impacchettato con lo stesso percorso. Clic destro su una texture per copiare quel percorso.
ui-armor-textures-under_pointer = Sotto il puntatore: { $material }
ui-armor-textures-reading = Lettura delle texture...
ui-armor-textures-summary = { $materials } materiali, { $textures } texture
ui-armor-textures-no_material = Nessun materiale
ui-armor-textures-unresolved = Il percorso non è in assets.bin
ui-armor-textures-absent = Non presente nei pacchetti installati
ui-armor-textures-unreadable = Impossibile leggere l'intestazione DDS
ui-armor-textures-unknown_format = Formato non riconosciuto
ui-armor-textures-details_one = { $format }, { $size }, 1 file, { $bytes }
ui-armor-textures-details = { $format }, { $size }, { $count } file, { $bytes }
ui-armor-textures-role_albedo = Albedo
ui-armor-textures-role_normal = Normal
ui-armor-textures-role_metal_gloss = Metallo-lucentezza
ui-armor-textures-role_ambient_occlusion = Occlusione ambientale
ui-armor-textures-role_detail = Dettaglio
ui-armor-textures-role_name_matched = Albedo, trovata dal nome file
ui-armor-textures-copy_path = Copia percorso
ui-armor-textures-copy_override = Copia percorso res_mods
ui-armor-textures-copy_overrides = Copia { $count } percorsi res_mods
ui-armor-textures-paths_copied = Copiati { $count } percorsi
ui-armor-textures-no_build = I dati di gioco non sono caricati, quindi la cartella mod della build non è nota.
ui-armor-textures-show_in_unpacker = Mostra nell'estrattore risorse
ui-armor-textures-show_textures = Mostra texture
ui-armor-textures-extract = Estrai file...
ui-armor-textures-extract_title = Estrai i file texture in
ui-armor-textures-extracting = Estrazione dei file texture...
ui-armor-textures-extracted = Estratti { $count } file in { $folder }
ui-armor-textures-extract_failed = Impossibile estrarre i file texture: { $reason }
ui-armor-textures-replace_title = Sostituire i file esistenti?
ui-armor-textures-replace_description = { $count } di questi file sono già in { $folder }. L'estrazione li sostituisce.
ui-armor-textures-replace = Sostituisci

## ui.settings.mcp
ui-settings-mcp-enabled = Attiva server MCP
ui-settings-mcp-enabled_description = Fornisce replay e dati di gioco agli assistenti IA tramite MCP. Mostra un controllo nella barra di stato.
ui-settings-mcp-start_on_launch = Avvia con il programma
ui-settings-mcp-start_on_launch_description = Avvia il server all'apertura di WoWs Toolkit.
ui-settings-mcp-bind_address = Indirizzo di ascolto
ui-settings-mcp-bind_address_description = Indirizzo IP di loopback su cui il server è in ascolto, come 127.0.0.1 o ::1. Solo questo computer può raggiungere il server.
ui-settings-mcp-port = Porta
ui-settings-mcp-port_description = Porta TCP su cui ascoltare. Scelta a caso alla prima attivazione e mantenuta.
ui-settings-mcp-port_unset = Scelta alla prima attivazione
ui-settings-mcp-address_not_loopback = { $input } non è un indirizzo di loopback. Il server è in ascolto solo su questo computer, ad esempio su 127.0.0.1.
ui-settings-mcp-address_invalid = { $input } non è un indirizzo IP
ui-settings-mcp-port_invalid = { $input } non è una porta da 1 a 65535
ui-settings-mcp-port_in_use = Un altro programma usa questa porta. Scegli un'altra porta.

## ui.mcp
ui-mcp-started = Server MCP attivo su { $url }
ui-mcp-copy_url = Copia URL
ui-mcp-start_failed = Impossibile avviare il server MCP: { $reason }
ui-mcp-port_in_use = la porta { $port } su { $address } è usata da un altro programma
ui-mcp-no_watchdog = il supervisore dei processi in background non è attivo
ui-mcp-status_running = Server MCP attivo su { $url }. Fai clic per fermarlo.
ui-mcp-status_starting = Avvio del server MCP su { $address }
ui-mcp-status_failed = Il server MCP non si è avviato. Fai clic per riprovare.
ui-mcp-status_stopped = Server MCP fermo. Fai clic per avviarlo.

## ui.launcher.opt_in
ui-launcher-opt_in-setting = Abilita il launcher del gioco
ui-launcher-opt_in-description = Gioca a World of Warships dal toolkit su Linux, con l'accesso effettuato tramite il tuo account Wargaming.
ui-launcher-opt_in-title = Abilitare il launcher del gioco?
ui-launcher-opt_in-notice_account = Utilizzando la funzione launcher riconosci che questa funzione non è uno scenario supportato dallo sviluppatore del gioco. Stai inserendo la tua password direttamente in questa applicazione che, se non ti fidi di essa, potrebbe essere usata per rubare il tuo account (sebbene lo sviluppatore dell'applicazione non intenda farlo).
ui-launcher-opt_in-notice_reports = Inoltre, riconosci che, se usi questa funzione, non segnalerai allo sviluppatore del gioco bug grafici o di affidabilità, a meno che tu non sia davvero convinto che si tratti di un problema multipiattaforma che NON riguarda SOLO Linux.
ui-launcher-opt_in-continue = Continua
ui-launcher-opt_in-cancel = Annulla

## ui.launcher.keyring
ui-launcher-keyring-unencrypted = Il tuo accesso è salvato senza cifratura nella cartella dati dell'app perché non è disponibile alcun provider Secret Service (come GNOME Keyring, KeePassXC o KDE Wallet).
ui-launcher-keyring-how = Come mantenerlo cifrato
ui-launcher-keyring-how_detail = Installa e avvia un provider Secret Service come GNOME Keyring, KeePassXC o KDE Wallet, poi riavvia il toolkit. Gli accessi salvati vengono spostati lì.
ui-launcher-keyring-unlock_hint = Il portachiavi di sistema potrebbe chiedere di essere sbloccato perché il toolkit possa leggere il tuo accesso salvato.

## ui.launcher.play
ui-launcher-play-play = Gioca
ui-launcher-play-play_through_steam = Gioca tramite Steam
ui-launcher-play-set_up_game = Configura il gioco
ui-launcher-play-set_up_runner = Configura Proton
ui-launcher-play-sign_in_again = Accedi di nuovo
ui-launcher-play-update = Aggiorna { $size }
ui-launcher-play-updating = Aggiornamento { $percent }%
ui-launcher-play-updating_unknown = Aggiornamento
ui-launcher-play-paused = In pausa
ui-launcher-play-starting = Avvio...
ui-launcher-play-running = In esecuzione
ui-launcher-play-in_battle = In battaglia
ui-launcher-play-menu = Account e opzioni di gioco

## ui.launcher.popover
ui-launcher-popover-game = World of Warships
ui-launcher-popover-no_install = Nessuna installazione del gioco impostata.
ui-launcher-popover-update_available = Aggiornamento disponibile
ui-launcher-popover-check_failed = Impossibile verificare gli aggiornamenti
ui-launcher-popover-accounts = Account
ui-launcher-popover-no_accounts = Ancora nessun account. Aggiungine uno per giocare con l'accesso effettuato.
ui-launcher-popover-close_to_switch = Chiudi il gioco per cambiare account.
ui-launcher-popover-default = Predefinito
ui-launcher-popover-ready = Pronto
ui-launcher-popover-restoring = Verifica...
ui-launcher-popover-not_saved = Non salvato
ui-launcher-popover-unencrypted = Salvato senza cifratura
ui-launcher-popover-keyring_locked = Portachiavi bloccato: sbloccalo per ripristinare gli accessi salvati
ui-launcher-popover-forget = Rimuovi
ui-launcher-popover-sign_out = Esci
ui-launcher-popover-expires_in = Scade tra { $duration }
ui-launcher-popover-expired = Sessione scaduta
ui-launcher-popover-expiry_unknown = Scadenza sconosciuta
ui-launcher-popover-make_default = Imposta come predefinito
ui-launcher-popover-account_actions = Azioni account
ui-launcher-popover-play_anyway = Gioca comunque
ui-launcher-popover-play_safe_mode = Gioca in modalità sicura
ui-launcher-popover-add_account = Aggiungi account...
ui-launcher-popover-game_settings = Impostazioni di gioco...
ui-launcher-popover-installs = Installazioni e configurazione...
ui-launcher-popover-open_replay = Apri replay nel gioco...

## ui.launcher.palette
ui-launcher-palette-play = Gioca
ui-launcher-palette-play_as = Gioca come { $nickname } ({ $realm })
ui-launcher-palette-game_settings = Impostazioni di gioco
ui-launcher-palette-add_account = Aggiungi account
ui-launcher-palette-update = Aggiorna World of Warships
ui-launcher-palette-open_replay = Apri replay nel gioco

## ui.launcher.installs
ui-launcher-installs-toolkit = Cartella del gioco del toolkit
ui-launcher-installs-played = Usata dall'account predefinito
ui-launcher-installs-play_from = Gioca da qui
ui-launcher-installs-update = Aggiorna

## ui.launcher.wgc_login
ui-launcher-wgc_login-label = Usa l'accesso Wargaming
ui-launcher-wgc_login-about = Avvia questa installazione Steam tramite l'accesso del toolkit invece che tramite Steam. Impostazioni e replay vengono tenuti separati dagli avvii tramite Steam. Portafoglio di Steam, obiettivi e tempo di gioco non funzionano in questa modalità.
ui-launcher-wgc_login-steam_accounts = Gli account creati tramite Steam richiedono un'email e una password. Aggiungile prima nella Gestione account di Wargaming.
ui-launcher-wgc_login-steam_accounts_link = Come aggiungere email e password
ui-launcher-wgc_login-ready = File del Game Center pronti
ui-launcher-wgc_login-fetchable = Attivando questa opzione vengono scaricati i file del Game Center di WG per questa build ({ $size }).
ui-launcher-wgc_login-downloading = Download dei file del Game Center: { $percent }%
ui-launcher-wgc_login-downloading_unknown = Download dei file del Game Center
ui-launcher-wgc_login-checking = Ricerca dei file del Game Center per questa build...
ui-launcher-wgc_login-needs_sign_in = Aggiungi prima un account Wargaming al toolkit.
ui-launcher-wgc_login-unavailable = Non disponibile per questa build: { $error }

## ui.launcher.replay
ui-launcher-replay-no_install = Nessuna installazione ha la build { $build } del gioco. Conserva o installa una copia di quella build per guardare questo replay.

## ui.launcher.setup
ui-launcher-setup-title = Configura World of Warships
ui-launcher-setup-game = Gioco
ui-launcher-setup-proton = Proton
ui-launcher-setup-accounts = Account
ui-launcher-setup-replays = Replay
ui-launcher-setup-game_question = Quale copia del gioco deve usare il toolkit?
ui-launcher-setup-none_found = Nessuna installazione trovata su questo computer.
ui-launcher-setup-choose_folder = Scegli una cartella...
ui-launcher-setup-install_new = Installa una nuova copia...
ui-launcher-setup-proton_explain = Su Linux il gioco viene eseguito tramite Proton: quello di Steam se Steam è installato, altrimenti umu-run.
ui-launcher-setup-proton_ready = Proton è stato trovato.
ui-launcher-setup-accounts_explain = Accedi agli account Wargaming con cui giochi. Gioca avvia il gioco con l'account predefinito.
ui-launcher-setup-replays_explain = Il gioco elimina i replay più vecchi quando ne conserva questo numero.
ui-launcher-setup-skip = Salta configurazione
ui-launcher-setup-back = Indietro
ui-launcher-setup-next = Avanti
ui-launcher-setup-done = Fine

## ui.launcher.game_settings
ui-launcher-game_settings-title = Impostazioni di gioco
ui-launcher-game_settings-build = build { $build }
ui-launcher-game_settings-display = Schermo
ui-launcher-game_settings-graphics = Grafica
ui-launcher-game_settings-sound = Audio
ui-launcher-game_settings-replays = Replay
ui-launcher-game_settings-locked = Il gioco è in esecuzione. Riscrive questi file alla chiusura, quindi fino ad allora sono di sola lettura.
ui-launcher-game_settings-footer = Scritto nei file delle preferenze del gioco. Viene conservato un backup.
ui-launcher-game_settings-cancel = Annulla
ui-launcher-game_settings-save = Salva
ui-launcher-game_settings-saved = Impostazioni di gioco salvate.
ui-launcher-game_settings-no_install = Nessuna installazione del gioco impostata.
ui-launcher-game_settings-window_mode = Modalità finestra
ui-launcher-game_settings-windowed = In finestra
ui-launcher-game_settings-borderless = Senza bordi
ui-launcher-game_settings-fullscreen = Schermo intero
ui-launcher-game_settings-monitor = Monitor
ui-launcher-game_settings-monitor_name = Nome del monitor
ui-launcher-game_settings-resolution = Risoluzione a schermo intero
ui-launcher-game_settings-width = Larghezza
ui-launcher-game_settings-height = Altezza
ui-launcher-game_settings-refresh = Frequenza di aggiornamento
ui-launcher-game_settings-display_rate = Frequenza dello schermo
ui-launcher-game_settings-vsync = Sincronizzazione verticale
ui-launcher-game_settings-ui_scale = Scala dell'interfaccia
ui-launcher-game_settings-battle_fps = Limite di frame rate in battaglia
ui-launcher-game_settings-port_fps = Limite di frame rate in porto
ui-launcher-game_settings-unlimited = Illimitato
ui-launcher-game_settings-skip_intro = Salta il video introduttivo
ui-launcher-game_settings-master = Volume generale
ui-launcher-game_settings-music_port = Musica in porto
ui-launcher-game_settings-music_battle = Musica in battaglia
ui-launcher-game_settings-preset = Preimpostazione
ui-launcher-game_settings-antialiasing = Anti-aliasing
ui-launcher-game_settings-textures = Qualità delle texture
ui-launcher-game_settings-filtering = Filtro delle texture
ui-launcher-game_settings-shadows = Qualità delle ombre
ui-launcher-game_settings-graphics_note = Modifica la grafica nel gioco, che la verifica in base al tuo hardware.
ui-launcher-game_settings-replays_kept = Replay da conservare
ui-launcher-game_settings-replays_kept_zero = Conserva almeno un replay.
ui-launcher-game_settings-versioned = Ordina i replay per versione del gioco
ui-launcher-game_settings-replays_note = Scritto nella cartella res_mods di questa build. La modalità sicura non la usa.
ui-launcher-game_settings-absent = Non impostato nei file del gioco
ui-launcher-game_settings-unrecognized = Valore sconosciuto: { $value }
ui-launcher-game_settings-not_a_number = { $field }: inserisci un numero.

## ui.settings.launcher
ui-settings-launcher-installs = Installazioni del gioco
ui-settings-launcher-installs_description = Le installazioni del gioco che il launcher avvia e aggiorna.
ui-settings-launcher-runner = Proton (Linux)
ui-settings-launcher-runner_description = Come viene eseguito il gioco su Linux. Proton Experimental o il Proton più recente di Steam viene trovato automaticamente, altrimenti umu-run.
ui-settings-launcher-launch_args = Argomenti di avvio
ui-settings-launcher-launch_args_description = Argomenti aggiuntivi della riga di comando per il gioco, uno per riga.
ui-settings-launcher-environment = Ambiente
ui-settings-launcher-environment_description = Variabili d'ambiente per il gioco, KEY=VALUE, una per riga.
ui-settings-launcher-safe_mode_args = Argomenti della modalità sicura
ui-settings-launcher-safe_mode_args_description = Argomenti aggiunti quando giochi in modalità sicura, uno per riga.
