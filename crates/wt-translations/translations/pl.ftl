# You are encouraged to suggest edits to this file on GitHub by either filing
# an issue, editing the file directly and creating a pull request, or just
# mentioning it in the Discord.
## meta
meta-language_name = Polski

## dialog
dialog-continue_in_english = Kontynuuj po angielsku
dialog-continue_in_language = Kontynuuj po polsku
dialog-select_language = Wybierz język
dialog-machine_translation_warning = Większość tłumaczeń została wygenerowana maszynowo i może być niedokładna.

## ui.tabs
ui-tabs-unpacker = Rozpakowywacz zasobów
ui-tabs-settings = Ustawienia
ui-tabs-replay_parser = Inspektor powtórek
ui-tabs-player_tracker = Śledzenie graczy
ui-tabs-armor_viewer = Przeglądarka pancerza
ui-tabs-model_viewer = Przeglądarka modeli
ui-tabs-stats = Statystyki
ui-tabs-directory_stats = Statystyki: { $dir }
ui-tabs-close_directory_stats = Zamknij statystyki katalogu
ui-tabs-search = Wyszukiwanie
ui-tabs-search_these_replays_unavailable = Te powtórki nie zostały jeszcze zindeksowane
ui-tabs-zoom_in_panel = Powiększ
ui-tabs-zoom_out_panel = Pomniejsz
ui-tabs-close_panel = Zamknij
ui-tabs-close_tab = Zamknij
ui-tabs-close_other_tabs = Zamknij pozostałe
ui-tabs-close_tabs_to_the_right = Zamknij karty po prawej

## ui.buttons
ui-buttons-yes = Tak
ui-buttons-no = Nie
ui-buttons-send_replays = Wyślij powtórki
ui-buttons-send_build_data = Wyślij tylko dane buildu
ui-buttons-send_nothing = Nie wysyłaj niczego
ui-buttons-switch_to_replays = Przełącz na powtórki
ui-buttons-keep_current = Zachowaj obecne ustawienie
ui-buttons-stop = Zatrzymaj
ui-buttons-copy = Kopiuj
ui-buttons-cancel = Anuluj
ui-buttons-continue_ = Kontynuuj
ui-buttons-install_update = Zainstaluj aktualizację
ui-buttons-view_release = Zobacz wydanie
ui-buttons-clear_settings = Wyczyść ustawienia
ui-buttons-more_options = Więcej opcji
ui-buttons-create_issue = Utwórz zgłoszenie
ui-buttons-discord = Discord
ui-buttons-github = GitHub
ui-buttons-view_github = Zobacz projekt na GitHub
ui-buttons-choose = Wybierz...
ui-buttons-reset = Resetuj
ui-buttons-clear = Wyczyść
ui-buttons-select_all = Zaznacz wszystko
ui-buttons-download = Pobierz
ui-buttons-retry = Ponów
ui-buttons-refresh_persisted_data_confirm = Odśwież

## ui.menu
ui-menu-file = Plik
ui-menu-check_updates = Sprawdź aktualizacje
ui-menu-about = O programie
ui-menu-quit = Zakończ

## ui.windows
ui-windows-build_consent = Zgoda na zbieranie buildów
ui-windows-replay_migration = Przejdź na udostępnianie powtórek
ui-windows-crash_detected = Wykryto awarię aplikacji
ui-windows-update_available = Dostępna aktualizacja
ui-windows-network_warning = Ostrzeżenie sieciowe
ui-windows-download_game_data = Pobierz dane gry
ui-windows-error = Błąd
ui-windows-software_encode = Brak kodera wideo GPU
ui-windows-about = O programie
ui-windows-refresh_persisted_data = Odśwież zapisane dane powtórek
ui-windows-tactics_board = Tablica taktyczna
ui-windows-minimize = Minimalizuj
ui-windows-maximize = Maksymalizuj
ui-windows-restore = Przywróć
ui-windows-close = Zamknij

## ui.dialogs
ui-dialogs-build_consent_message = Czy chcesz wysyłać informacje o buildach graczy z bitew rankingowych i losowych do dewelopera? Zbieranie tych danych pomaga społeczności w masowej analizie buildów graczy. Możesz zrezygnować w każdej chwili w ustawieniach.
ui-dialogs-replay_migration_message = Toolkit może teraz udostępniać pełne powtórki, co pozwala na znacznie bogatszą analizę dobierania graczy niż same dane buildów. Bitwy, w których grasz okrętem testowym, nie są udostępniane. Czy chcesz przejść na udostępnianie surowych powtórek?
ui-dialogs-crash_message = Wygląda na to, że WoWs Toolkit uległ awarii podczas ostatniego uruchomienia. Jeśli chcesz zgłosić ten problem, utwórz zgłoszenie na GitHub lub dołącz do serwera Discord i podaj poniższe informacje.
ui-dialogs-crash_clear_settings = Jeśli z jakiegoś powodu dane przechowywane przez aplikację mogą być odpowiedzialne za problem, możesz spróbować wyczyścić ustawienia przyciskiem poniżej. Spowoduje to wyczyszczenie wszystkich ustawień. Śledzeni gracze zostaną zachowani. Twoje powtórki i dane WoWs będą bezpieczne.
ui-dialogs-update_message = Dostępna jest wersja { $tag } WoWs Toolkit
ui-dialogs-update_notes = Informacje o wydaniu
ui-dialogs-update_asset_pending = Plik do pobrania dla Windows nie jest jeszcze dostępny. Sprawdź stan kompilacji na stronie wydania.
ui-dialogs-update_windows_only = Aktualizacja dostępna, ale obecnie obsługiwany jest tylko Windows.
ui-dialogs-p2p_warning = Sesje współpracy używają sieci P2P. Inni użytkownicy w sesji mogą widzieć Twój adres IP.
ui-dialogs-software_encode_message = Nie znaleziono obsługiwanego kodera wideo GPU, więc ten eksport zostanie zakodowany programowo. Trwa to znacznie dłużej, a długa bitwa może zająć kilka minut.
ui-dialogs-download_game_data_intro = Niektóre powtórki wymagają danych gry, które nie są zainstalowane. Wybierz buildy do pobrania.
ui-dialogs-download_build_row = Wersja { $version } (build { $build })
ui-dialogs-download_replays_needing = wymagane przez { $count ->
    [one] { $count } powtórkę
    [few] { $count } powtórki
    [many] { $count } powtórek
   *[other] { $count } powtórki
}
ui-dialogs-download_availability_exact = opublikowany
ui-dialogs-download_availability_nearest = brak dokładnego dopasowania, najbliższy to { $version } (build { $build })
ui-dialogs-download_availability_unpublished = nigdy nie opublikowany
ui-dialogs-download_availability_unreachable = opublikowany, ale nie udało się odczytać jego listy plików
ui-dialogs-download_plan_pending = Ustalanie, co należy pobrać...
ui-dialogs-download_objects_to_fetch = Obiekty do pobrania dla zaznaczonych buildów: { $count }
ui-dialogs-download_plan_failed = Nie udało się ustalić, co należy pobrać
ui-dialogs-download_plan_no_cache_dir = Nie ustawiono katalogu pamięci podręcznej danych gry. Wybierz go w Ustawieniach i spróbuj ponownie.
ui-dialogs-refresh_persisted_data_message = Ponownie przetwórz każdą powtórkę w katalogu powtórek i zapisz od nowa jej dane. Wpisy zapisane z mapowaniem danych powtórek niezgodnym z ich buildem gry pozostają bez zmian, z wyjątkiem osobistego ratingu i trybu gry, które są przeliczane dla każdej powtórki według bieżących wartości oczekiwanych. Może to potrwać.
ui-dialogs-file_dialog_failed = Nie można otworzyć okna wyboru pliku: { $reason }

## ui.labels
ui-labels-error_occurred = Wystąpił błąd:
ui-labels-made_by = Stworzono przez landaire.
ui-labels-credits = Podziękowania dla Trackpad, TTaro, lkolbly za ich wkład.
ui-labels-pr_credits = Dane i formuła obliczania osobistego ratingu (PR) dostarczone przez WoWs Numbers.
ui-labels-suppress_warning = Nie pokazuj ponownie
ui-labels-more_info = Więcej informacji
ui-labels-app_version = Wersja { $version }
ui-labels-project_license = WoWs Toolkit jest rozpowszechniany na licencji { $license }.
ui-labels-third_party_notices = Informacje o komponentach innych firm
ui-labels-notices_search_hint = Szukaj w informacjach
ui-labels-no_matching_notices = Żadna informacja nie pasuje do wyszukiwania.

## ui.messages
ui-messages-bug_report_info_copied = Skopiowano informacje do zgłoszenia błędu.
ui-messages-update_downloading = Pobieranie aktualizacji: { $read } z { $total }
ui-messages-update_downloading_unknown = Pobieranie aktualizacji: dotychczas { $read }
ui-messages-app_up_to_date = Aplikacja jest aktualna
ui-messages-checking_app_updates = Sprawdzanie aktualizacji aplikacji...
ui-messages-update_starting = Pobieranie aktualizacji...
ui-messages-update_check_failed = Nie udało się sprawdzić aktualizacji
ui-messages-game_data_loaded = Pomyślnie wczytano dane gry
ui-messages-no_replays_detected = Nie wykryto powtórek. Czy katalog WoWs jest poprawnie skonfigurowany?
ui-messages-game_data_builds_downloaded = Pobrano dane gry dla { $count ->
    [one] { $count } buildu
    [few] { $count } buildów
    [many] { $count } buildów
   *[other] { $count } buildu
}
ui-messages-game_data_download_failed = Nie udało się pobrać danych gry
ui-messages-game_data_up_to_date = Dane gry są aktualne
ui-messages-game_data_updates_available = { $count ->
    [one] { $count } build w pamięci podręcznej ma dostępne nowsze dane
    [few] { $count } buildy w pamięci podręcznej mają dostępne nowsze dane
    [many] { $count } buildów w pamięci podręcznej ma dostępne nowsze dane
   *[other] { $count } buildu w pamięci podręcznej ma dostępne nowsze dane
}
ui-messages-game_data_cache_valid = Pamięć podręczna danych gry jest poprawna i zgodna ze zdalnym repozytorium
ui-messages-game_data_cache_invalid = { $count ->
    [one] { $count } build w pamięci podręcznej jest nieaktualny lub uszkodzony
    [few] { $count } buildy w pamięci podręcznej są nieaktualne lub uszkodzone
    [many] { $count } buildów w pamięci podręcznej jest nieaktualnych lub uszkodzonych
   *[other] { $count } buildu w pamięci podręcznej jest nieaktualne lub uszkodzone
}
ui-messages-replay_load_failed = Nie udało się wczytać powtórki
ui-messages-twitch_token_invalid = Token Twitch jest nieprawidłowy lub wygasł. Zaktualizuj go w Ustawieniach.
ui-messages-wows_dir_invalid = Katalog World of Warships jest nieprawidłowy. Sprawdź ścieżkę w Ustawieniach.
ui-messages-wows_dir_not_set = Nie ustawiono katalogu World of Warships
ui-messages-constants_version_mismatch =
    Wersja pliku mapowania danych powtórek nie odpowiada wersji gry.
    Wyniki po bitwie mogą być niedokładne. Proszę czekać, aż opiekunowie projektu zaktualizują mapowanie na serwerze.
ui-messages-session_started = Sesja rozpoczęta
ui-messages-user_joined = { $name } dołączył(a)
ui-messages-user_timeout = { $name } — przekroczono limit czasu
ui-messages-user_left = { $name } opuścił(a) sesję
ui-messages-session_ended = Sesja zakończona
ui-messages-session_error = Błąd sesji: { $msg }
ui-messages-connected_to_session = Połączono z sesją
ui-messages-host_opened_replay = Host otworzył powtórkę: { $name }
ui-messages-host_closed_replay = Host zamknął powtórkę
ui-messages-session_rejected = Sesja odrzucona: { $reason }
ui-messages-drop_not_a_replay = To nie jest plik powtórki
ui-messages-drop_not_replays = To nie są pliki powtórek
ui-messages-drop_skipped = Pominięto { $count ->
    [one] { $count } upuszczony plik, który nie jest powtórką ani folderem
    [few] { $count } upuszczone pliki, które nie są powtórkami ani folderami
    [many] { $count } upuszczonych plików, które nie są powtórkami ani folderami
   *[other] { $count } upuszczonego pliku, który nie jest powtórką ani folderem
}
ui-messages-log_copied = Skopiowano { $bytes } KB z { $file } do schowka
ui-messages-log_copy_failed = Nie udało się odczytać pliku logu: { $error }
ui-messages-log_not_found = Nie znaleziono pliku logu w { $dir }

## stat
stat-damage = Obrażenia
stat-spotting_damage = Obrażenia po wykryciu
stat-frags = Fragi
stat-raw_xp = Surowe XP
stat-base_xp = Bazowe XP
stat-win_rate = Wskaźnik wygranych
stat-personal_rating = Osobisty rating
stat-match_contribution = Wkład w bitwę
stat-capture_points = Punkty za przejmowanie
stat-planes_shot_down = Zestrzelone samoloty
stat-anti_air_damage = Obrażenia przeciwlotnicze
stat-survival_rate = Wskaźnik przetrwania
stat-torpedoes_spotted = Wykryte torpedy
stat-avg_pr = Śr. PR
stat-avg_prefix = Śr. { $name }

## chart
chart-combined = Łącznie
chart-combined_suffix = (Łącznie)
chart-rolling_average_suffix = (Średnia krocząca)

## confirm
confirm-open_in_game = Spowoduje to uruchomienie World of Warships. Kontynuować?
confirm-clear_all_session_stats = Spowoduje to wyczyszczenie wszystkich statystyk sesji. Kontynuować?
confirm-set_as_session_stats = Spowoduje to zastąpienie bieżących statystyk sesji. Kontynuować?

## error
error-file_watcher_creation = Nie udało się utworzyć obserwatora plików: { $error }
error-replay_dir_watch = Nie udało się obserwować katalogu powtórek: { $error }

## ui.settings.app
ui-settings-app-check_for_updates = Sprawdzaj aktualizacje przy uruchomieniu
ui-settings-app-enable_logging = Włącz logowanie (wymaga ponownego uruchomienia)
ui-settings-app-enable_logging_tooltip = Zapisuj logi debugowania do pliku w katalogu danych aplikacji. Przydatne przy rozwiązywaniu problemów.
ui-settings-app-data_sharing_mode = Udostępnianie danych (bitwy rankingowe i losowe)
ui-settings-app-theme = Motyw
ui-settings-app-theme_system = Systemowy
ui-settings-app-theme_dark = Ciemny
ui-settings-app-theme_light = Jasny
ui-settings-app-code_integrity = Blokuj niepodpisany kod (wymaga ponownego uruchomienia)
ui-settings-app-code_integrity_tooltip = Odmawia wczytania kodu, którego nie podpisał Microsoft, dzięki czemu nakładki, narzędzia do przechwytywania obrazu i programy audio nie trafiają do tej aplikacji. Są one częstą przyczyną awarii i zawieszeń podczas renderowania. Sterowniki graficzne są podpisane przez Windows i działają dalej. Tej opcji nie można wyłączyć po rozpoczęciu sesji, więc zmiana zadziała przy następnym uruchomieniu. Tryb automatyczny pomija ją, jeśli masz zainstalowaną metodę wprowadzania innej firmy, którą w przeciwnym razie by zablokowała. Pamiętaj, że pozostałe zabezpieczenia działają niezależnie od tego ustawienia; uruchom z --no-hardening, aby wyłączyć je wszystkie.
ui-settings-app-code_integrity_automatic = Automatycznie
ui-settings-app-code_integrity_always = Zawsze
ui-settings-app-code_integrity_never = Nigdy
ui-settings-app-language = Język
ui-settings-app-open_data_dir_tooltip = Otwórz folder danych aplikacji (ustawienia, logi, dane w pamięci podręcznej) w menedżerze plików.
ui-settings-app-proxy_url = Serwer proxy HTTP
ui-settings-app-proxy_url_hint_live = host:port lub pozostaw puste, aby użyć systemowych ustawień proxy

## ui.settings.wows
ui-settings-wows-description = Ścieżka do instalacji World of Warships
ui-settings-wows-directory_hint = Katalog World of Warships
ui-settings-wows-browse = Przeglądaj...

## ui.settings.wows.cache
ui-settings-wows-cache-heading = Pamięć podręczna danych gry
ui-settings-wows-cache-description = Zapisane dane gry, dzięki którym powtórki ze starszych buildów nadal się otwierają
ui-settings-wows-cache-auto_dump = Zapisuj dane gry dla kompatybilności ze starszymi powtórkami
ui-settings-wows-cache-browse = Przeglądaj
ui-settings-wows-cache-open_folder = Otwórz folder
ui-settings-wows-cache-delete_old = Usuń stare wersje
ui-settings-wows-cache-stats = Pamięć podręczna danych gry: { $size } w { $count ->
    [one] { $count } wersji
    [few] { $count } wersjach
    [many] { $count } wersjach
   *[other] { $count } wersji
}
ui-settings-wows-cache-check_updates = Sprawdź aktualizacje
ui-settings-wows-cache-actions = Działania na pamięci podręcznej
ui-settings-wows-cache-actions_tooltip = Sprawdź aktualizacje lub zweryfikuj buildy w pamięci podręcznej
ui-settings-wows-cache-update_all = Zaktualizuj wszystkie
ui-settings-wows-cache-updates_available = Nowsze dane są dostępne dla buildów w pamięci podręcznej: { $count }
ui-settings-wows-cache-validate = Sprawdź pamięć podręczną
ui-settings-wows-cache-repair = Napraw
ui-settings-wows-cache-repair_needed = Nieaktualne lub uszkodzone buildy w pamięci podręcznej: { $count }

## ui.settings.replay
ui-settings-replay-show_raw_xp = Pokaż surowe XP
ui-settings-replay-show_entity_id = Pokaż ID jednostki
ui-settings-replay-show_observed_damage = Pokaż zaobserwowane obrażenia
ui-settings-replay-enable_previews = Pokaż podgląd minimapy po najechaniu
ui-settings-replay-enable_previews_tooltip = Najechanie na powtórkę powoduje jej odczytanie i odtworzenie w tle, aby zbudować zapętlony podgląd minimapy. Wyłącz przy wolnym nośniku.
ui-settings-replay-auto_export_data = Automatyczny eksport danych
ui-settings-replay-export_path_hint = Katalog automatycznego eksportu danych
ui-settings-replay-export_path_missing = Katalog nie istnieje. Nie można do niego eksportować danych.
ui-settings-replay-index_all_replays = Zindeksuj wszystkie powtórki
ui-settings-replay-show_heals = Pokaż naprawy
ui-settings-replay-current_path = Bieżąca ścieżka powtórek

## ui.settings.session
ui-settings-session-display_name = Nazwa wyświetlana
ui-settings-session-suppress_ip_warning = Nie pokazuj ostrzeżenia o adresie IP P2P
ui-settings-session-ip_warning_tooltip = Sesje współpracy używają sieci P2P. Inni użytkownicy w sesji mogą widzieć Twój adres IP.
ui-settings-session-disable_auto_open = Wyłącz automatyczne otwieranie okien sesji
ui-settings-session-auto_open_tooltip = Po włączeniu okna sesji (powtórki, tablice taktyczne) nie będą otwierane automatycznie. Nadal możesz je otworzyć z menu sesji.

## ui.settings.twitch
ui-settings-twitch-get_token = Pobierz token logowania
ui-settings-twitch-get_token_tooltip = Używamy strony logowania Chatterino, ponieważ zapewnia token z niezbędnymi uprawnieniami (zasadniczo token moderatora z uprawnieniami czatu) i eliminuje potrzebę hostowania własnej strony logowania przez dewelopera WoWs Toolkit, co dałoby ten sam rezultat.
ui-settings-twitch-paste_token_no_token = Wklej token (brak tokenu)
ui-settings-twitch-paste_token_tooltip = Odczytuje dane uwierzytelniające ze schowka
ui-settings-twitch-paste_token_valid = Wklej token (token prawidłowy)
ui-settings-twitch-paste_token_unvalidated = Sprawdzanie tokenu
ui-settings-twitch-paste_token_invalid = Wklej token (token nieprawidłowy / wygasły)
ui-settings-twitch-monitored_channel = Monitorowany kanał (domyślnie własny)
ui-settings-twitch-signed_in_as = Zalogowano jako { $who }

## ui.replay
ui-replay-copy_bug_report_info = Kopiuj informacje do zgłoszenia błędu
ui-replay-match_age = { $age } temu
ui-replay-listing_caption = Powtórki
ui-replay-loading_game_data_version = Wczytywanie danych gry dla { $version }...
ui-replay-spectator = Widz
ui-replay-loading = Wczytywanie...
ui-replay-preview_loading = Tworzenie podglądu...
ui-replay-preview_no_game_data = Brak danych gry dla wersji klienta { $value }
ui-replay-preview_unreadable_because = Nie udało się odczytać tej powtórki: { $value }
ui-replay-preview_unwalkable = Nie udało się przetworzyć bitwy z tej powtórki: { $value }
ui-replay-preview_unknown_version = Ta powtórka podaje nieczytelną wersję klienta: { $value }
ui-replay-preview_no_map = Brak danych mapy dla { $value }
ui-replay-preview_render_failed = Nie udało się narysować podglądu: { $value }
ui-replay-incomplete_results = Niekompletne wyniki meczu
ui-replay-incomplete_results_tooltip = Powtórka nie zawiera jeszcze wyników końcowych. Dane zostaną automatycznie ponownie wczytane po zakończeniu meczu i dodaniu wyników.
ui-replay-no_chat = W tej powtórce nie wysłano żadnych wiadomości na czacie
ui-replay-chat_copy_all = Kopiuj wszystko
ui-replay-chat_save_to_file = Zapisz do pliku
ui-replay-chat_save_title = Zapisz czat z gry
ui-replay-export_results_title = Eksportuj wyniki
ui-replay-results_exported = Wyeksportowano wyniki
ui-replay-chat_saved = Zapisano czat do { $path }
ui-replay-chat_save_failed = Nie zapisano czatu: { $error }
ui-replay-team_damage = Obrażenia drużynowe:{" "}
ui-replay-team_share = { $pct } sumy drużyny ({ $team })
ui-replay-hide_my_stats = Ukryj statystyki moich testowych okrętów
ui-replay-open_manually = Ręcznie otwórz plik powtórki...
ui-replay-open_directory = Otwórz katalog powtórek...
ui-replay-search_these_replays = Przeszukaj te powtórki
ui-replay-import_constants = Importuj plik constants.json...
ui-replay-refresh_persisted_data = Odśwież zapisane dane powtórek...
ui-replay-contributing = Wysyłanie { $done } z { $total ->
    [one] { $total } powtórki
    [few] { $total } powtórek
    [many] { $total } powtórek
   *[other] { $total } powtórki
}
ui-replay-contributed = Wysłano { $sent } z { $total ->
    [one] { $total } powtórki
    [few] { $total } powtórek
    [many] { $total } powtórek
   *[other] { $total } powtórki
}
ui-replay-contribute_nothing_shared = Najpierw wybierz w Ustawieniach, co chcesz udostępniać
ui-replay-contribute_none_listed = Brak powtórek do wysłania
ui-replay-contribute_not_ready = Powtórki można wysłać po wczytaniu danych gry i ustawień
ui-replay-contribute_already_running = Wysyłanie powtórek już trwa
ui-replay-auto_export_failed = Nie udało się zapisać bitwy: { $error }
ui-replay-auto_exported = Wyeksportowano bitwę do { $file }
ui-replay-opened_new_battle = Otwarto nową bitwę: { $battle }
ui-replay-new_battle_listed = Nowa bitwa: { $battle }
ui-replay-results_arrived = Dotarły wyniki bitwy { $battle }
ui-replay-constants_written = Pobrano mapowania wyników dla { $count ->
    [one] { $count } buildu
    [few] { $count } buildów
    [many] { $count } buildów
   *[other] { $count } buildu
}
ui-replay-constants_import_title = Importuj plik constants.json
ui-replay-constants_imported = Zaimportowano mapowanie wyników dla builda { $build }
ui-replay-constants_import_failed = Nie zaimportowano mapowania: { $reason }
ui-replay-constants_no_build = Najpierw wczytaj build gry
ui-replay-constants_latest_written = Pobrano najnowsze mapowanie wyników
ui-replay-constants_check_failed = Nie udało się sprawdzić mapowań wyników: { $reason }
ui-replay-constants_build_label = build { $build }
ui-replay-constants_generating = Aktualizowanie stałych gry dla { $version }...
ui-replay-constants_generated = Wygenerowano stałe gry dla { $version } w { $seconds } s
ui-replay-constants_generation_failed = Nie udało się zaktualizować stałych gry dla { $version }: { $reason }. { $fallback }
ui-replay-constants_fallback_kept = Zachowano zapisane już dla niej stałe.
ui-replay-constants_fallback_published = Używane są stałe opublikowane dla builda { $build }.
ui-replay-constants_fallback_embedded = Używane są wbudowane stałe.
ui-replay-copy_latest_log = Kopiuj najnowszy log do schowka
ui-replay-load_alt_perspective = Wczytaj perspektywę drugiej drużyny...
ui-replay-load_alt_perspective_failed = Nie udało się scalić powtórki: { $error }
ui-replay-load_alt_perspective_limit = Ta bitwa jest już odczytywana z { $limit ->
    [one] { $limit } innego nagrania
    [few] { $limit } innych nagrań
    [many] { $limit } innych nagrań
   *[other] { $limit } innego nagrania
}
ui-replay-consumable_charges_remaining = pozostało { $remaining } z { $total ->
    [one] { $total } ładunku
    [few] { $total } ładunków
    [many] { $total } ładunków
   *[other] { $total } ładunku
}
ui-replay-consumable_charges_unlimited = użyto { $used } (nieograniczona liczba ładunków)
ui-replay-consumable_header_remaining = Pozostało
ui-replay-consumable_header_total = Łącznie
ui-replay-consumable_charges_infinite = ∞
ui-replay-autoload_latest = Automatycznie wczytuj najnowszą powtórkę
ui-replay-autoload_latest_tooltip = Otwórz mecz, gdy tylko gra skończy go zapisywać
ui-replay-anonymize_names = Anonimizuj nazwy graczy
ui-replay-anonymize_names_tooltip = Pokazuj wygenerowane nazwy zamiast nazw graczy i tagów klanów. Eksporty zachowują prawdziwe nazwy.
ui-replay-group_win_rate_all_modes = Uwzględniaj wszystkie tryby gry we wskaźniku wygranych grup
ui-replay-group_win_rate_all_modes_tooltip = Domyślnie nagłówki grup obejmują tylko bitwy losowe i rankingowe. Po włączeniu liczą się wszystkie tryby gry, a wskaźnik wygranych nie jest kolorowany, ponieważ kolory są skalibrowane dla bitew losowych.
ui-replay-select_replay = Wybierz powtórkę
ui-replay-drop_hint = Upuść tutaj powtórkę, aby ją otworzyć, lub folder, aby wyświetlić jego powtórki
ui-replay-scanning = Skanowanie powtórek...
ui-replay-scanning_count = Odczytywanie powtórki { $done } z { $total }
ui-replay-no_replays_in = Brak powtórek w { $folder }
ui-replay-drop_folder_hint = Upuść folder na okno, aby wyświetlić go w osobnej karcie
ui-replay-new_tag = Nowa
ui-replay-loading_game_data = Wczytywanie danych gry...
ui-replay-game_data_failed = Nie udało się wczytać danych gry: { $error }
ui-replay-group_label = Grupowanie powtórek
ui-replay-column_filters = Filtry kolumn
ui-replay-row_elements = Elementy wiersza
ui-replay-row_element_personal_rating = Osobisty rating
ui-replay-row_element_match_contribution = Wkład w bitwę
ui-replay-row_element_division = Dywizjon
ui-replay-row_element_twitch = Czat Twitch
ui-replay-row_element_damage = Obrażenia
ui-replay-row_element_kills = Zabójstwa
ui-replay-row_element_time = Czas bitwy
ui-replay-listing_options = Opcje listy
ui-replay-open = Otwórz
ui-replay-open_file = Otwórz plik powtórki...
ui-replay-check_game_data = Sprawdź dane gry
ui-replay-check_game_data_tooltip = Sprawdź dane gry wymagane przez powtórki w tym katalogu
ui-replay-close_side_panel = Zamknij panel
ui-replay-nda = NDA
ui-replay-chat = Czat
ui-replay-game_chat = Czat w grze
ui-replay-save_to_file = Zapisz do pliku
ui-replay-chat_copied = Skopiowano czat
ui-replay-copy_message = Kopiuj wiadomość
ui-replay-actions = Akcje
ui-replay-row_actions_hint = Akcje gracza (lub kliknij wiersz prawym przyciskiem)
ui-replay-section_match = Mecz
ui-replay-section_export = Eksport
ui-replay-section_debug = Debugowanie
ui-replay-timeline = Oś czasu meczu
ui-replay-row_not_indexed = niezindeksowana
ui-replay-hover_survived = Przetrwał
ui-replay-hover_sunk = Zatopiony
ui-replay-hover_not_indexed = Niezindeksowana
ui-replay-row_kills = { $count ->
    [one] { $count } zniszczenie
    [few] { $count } zniszczenia
    [many] { $count } zniszczeń
   *[other] { $count } zniszczenia
}
ui-replay-row_survived = przetrwał
ui-replay-row_sunk = zatopiony
ui-replay-row_division = Dywizjon: { $members }
ui-replay-export_results_json = Wyniki: JSON
ui-replay-export_results_cbor = Wyniki: CBOR
ui-replay-export_results_csv = Wyniki: CSV
ui-replay-timeline_filter_all = Wszystkie
ui-replay-timeline_filter_none = Brak
ui-replay-timeline_search_hint = Szukaj okrętu lub gracza
ui-replay-timeline_no_events = Nie wykryto istotnych zdarzeń.
ui-replay-timeline_no_matches = Żadne zdarzenie nie pasuje do bieżącego filtra.
ui-replay-timeline_parsing = Przetwarzanie zdarzeń...
ui-replay-timeline_failed = Nie udało się wyodrębnić osi czasu meczu: { $error }
ui-replay-timeline_retry = Ponów
ui-replay-timeline_export_tooltip = Dotyczy zdarzeń widocznych po zastosowaniu filtrów.
ui-replay-timeline_copied = Skopiowano oś czasu
ui-replay-timeline_event_copied = Skopiowano zdarzenie
ui-replay-timeline_saved = Oś czasu zapisano w { $path }
ui-replay-timeline_save_failed = Nie zapisano osi czasu: { $error }
ui-replay-timeline_kind_health_lost = Duże obrażenia
ui-replay-timeline_kind_death = Zniszczenie
ui-replay-timeline_kind_cap_contested = Punkt sporny
ui-replay-timeline_kind_cap_flipped = Punkt zdobyty
ui-replay-timeline_kind_cap_being_captured = Punkt zdobywany
ui-replay-timeline_kind_radar = Użycie radaru
ui-replay-timeline_kind_advantage = Zmiana przewagi
ui-replay-timeline_kind_disconnected = Rozłączenie

## ui.replay.context
ui-replay-context-open_in_new_tab = Otwórz w nowej karcie
ui-replay-context-copy_cell = Kopiuj komórkę
ui-replay-context-copy_row = Kopiuj wiersz
ui-replay-context-copy_table = Kopiuj tabelę
ui-replay-context-pin_column = Przypnij kolumnę
ui-replay-context-unpin_column = Odepnij kolumnę
ui-replay-context-copied = Skopiowano
ui-replay-context-copy_path = Kopiuj ścieżkę
ui-replay-context-copy_replays = { $count ->
    [one] Kopiuj powtórkę
    [few] Kopiuj { $count } powtórki
    [many] Kopiuj { $count } powtórek
   *[other] Kopiuj { $count } powtórki
}
ui-replay-context-copy_paths = { $count ->
    [one] Kopiuj ścieżkę
    [few] Kopiuj { $count } ścieżki
    [many] Kopiuj { $count } ścieżek
   *[other] Kopiuj { $count } ścieżki
}
ui-replay-context-show_in_explorer = Pokaż w eksploratorze plików
ui-replay-context-show_replay_controls = Pokaż sterowanie powtórką
ui-replay-context-open_in_game = Otwórz w grze
ui-replay-context-render_replay = Renderuj powtórkę
ui-replay-render = { ui-replay-context-render_replay }
ui-replay-context-set_session_stats = Ustaw jako statystyki sesji ({ $count ->
    [one] { $count } powtórka
    [few] { $count } powtórki
    [many] { $count } powtórek
   *[other] { $count } powtórki
})
ui-replay-context-add_session_stats = Dodaj do statystyk sesji ({ $count ->
    [one] { $count } powtórka
    [few] { $count } powtórki
    [many] { $count } powtórek
   *[other] { $count } powtórki
})
ui-replay-context-render_to_video = { $count ->
    [one] Eksportuj jako wideo
    [few] Eksportuj { $count } powtórki jako wideo
    [many] Eksportuj { $count } powtórek jako wideo
   *[other] Eksportuj { $count } powtórki jako wideo
}
ui-replay-context-render_to_clipboard = { $count ->
    [one] Eksportuj do schowka
    [few] Eksportuj { $count } powtórki do schowka
    [many] Eksportuj { $count } powtórek do schowka
   *[other] Eksportuj { $count } powtórki do schowka
}

## ui.replay.column
ui-replay-column-player_name = Nazwa gracza
ui-replay-column-base_xp = Bazowe XP
ui-replay-column-ship_name = Nazwa okrętu
ui-replay-column-hits = Trafienia
ui-replay-column-heals = Naprawy
ui-replay-column-heals_tooltip = Liczba zaobserwowanych aktywacji Grupy naprawczej u tego gracza. Może być niedokładna dla okrętów nierenderowanych na ekranie (tj. okrętów wroga, które nigdy nie zostały wykryte).
ui-replay-column-heals_no_repair_tooltip = Ten okręt nie ma sprzętu eksploatacyjnego Grupa naprawcza.
ui-replay-column-kills = Zabójstwa
ui-replay-column-observed_damage = Zaobserwowane obrażenia
ui-replay-column-actual_damage = Rzeczywiste obrażenia
ui-replay-column-potential_damage = Potencjalne obrażenia
ui-replay-column-time_lived = Czas przeżycia
ui-replay-column-fires = Pożary
ui-replay-column-floods = Zalania
ui-replay-column-citadels = Cytadele
ui-replay-column-crits = Trafienia krytyczne
ui-replay-column-received_damage = Otrzymane obrażenia
ui-replay-column-distance_traveled = Przebyty dystans
ui-replay-column-skills = Umiejętności
ui-replay-column-personal_rating = PR
ui-replay-column-match_contribution = MC

## ui.rating
ui-rating-personal_rating = Osobisty rating
ui-rating-match_contribution = Wkład w bitwę
ui-rating-versus_average = względem średniej
ui-rating-damage = Obrażenia
ui-rating-kills = Zabójstwa
ui-rating-wins = Zwycięstwo
ui-rating-team_damage_share = Udział w obrażeniach drużyny
ui-rating-tanking = Ściągnięty ogień
ui-rating-spotting = Wykrywanie
ui-rating-objectives = Cele
ui-rating-anti_air = Obrona przeciwlotnicza
ui-rating-torpedo_warnings = Ostrzeżenia o torpedach
ui-rating-critical_bursts = Krytyczne serie
ui-rating-unrated_early_exit = Brak MC: nagranie zakończyło się przed końcem bitwy
ui-rating-mc_unrated_battle_type = Brak MC: wkład w bitwę dotyczy tylko bitew losowych i rankingowych

## ui.rating.detail
ui-rating-detail-versus_expected = { $actual } wobec oczekiwanych { $expected }
ui-rating-detail-wins = wygrane: { $actual } wobec oczekiwanych { $expected }
ui-rating-detail-weighted_damage = { $actual } wobec oczekiwanych { $expected }, obrażenia niszczycielom i okrętom podwodnym x1.2
ui-rating-detail-team_damage_share = { $share }% obrażeń drużyny zadanych przeciwnikom, równy udział to { $even }%
ui-rating-detail-tanking = { $potential } potencjalnych obrażeń, { $class }x średnia klasy, { $team }x równy udział w drużynie
ui-rating-detail-spotting = { $damage } obrażeń po wykryciu (średnia klasy { $baseline }), wykryte PW okrętów: { $ships } (średnia { $ships_baseline })
ui-rating-detail-objectives = Punkty: przejmowanie { $capture }, blokowanie { $block }, resetowanie { $reset }, utrzymane na koniec { $battle_end }, utrzymywanie { $hold } (liczone w jednej czwartej)
ui-rating-detail-anti_air_planes = { $strike ->
    [one] { $strike } samolot uderzeniowy
    [few] { $strike } samoloty uderzeniowe
    [many] { $strike } samolotów uderzeniowych
   *[other] { $strike } samolotu uderzeniowego
}, { $support ->
    [one] { $support } myśliwiec lub samolot zwiadowczy
    [few] { $support } myśliwce i samoloty zwiadowcze
    [many] { $support } myśliwców i samolotów zwiadowczych
   *[other] { $support } myśliwca lub samolotu zwiadowczego
}
ui-rating-detail-anti_air_damage = { $damage } obrażeń OPL
ui-rating-detail-torpedo_warnings = { $threatening ->
    [one] Wykryto { $threatening } groźną torpedę
    [few] Wykryto { $threatening } groźne torpedy
    [many] Wykryto { $threatening } groźnych torped
   *[other] Wykryto { $threatening } groźnej torpedy
}
ui-rating-detail-critical_bursts = { $count ->
    [one] { $count } krytyczna seria
    [few] { $count } krytyczne serie
    [many] { $count } krytycznych serii
   *[other] { $count } krytycznej serii
}: { $bursts }. Widoczne tylko dla okrętów w pobliżu nagrywającego gracza
ui-rating-detail-critical_burst = { $share }% { $ship } w { $seconds } s
ui-rating-detail-critical_burst_at_once = { $share }% { $ship } naraz

## ui.personal_rating
ui-personal_rating-unrated_hover = Osobisty rating dotyczy tylko bitew losowych i rankingowych.

## ui.rating.basis
ui-rating-basis-static = Statyczny
ui-rating-basis-dynamic = Dynamiczny
ui-rating-basis-static_explained = Statyczny: mierzony względem stałej wartości lub poziomu bazowego klasy
ui-rating-basis-dynamic_explained = Dynamiczny: mierzony względem wartości oczekiwanych wows-numbers lub innych graczy w tej bitwie

## ui.replay.group
ui-replay-group-date = Data
ui-replay-group-ship = Okręt
ui-replay-group-none = Brak

## ui.replay.player
ui-replay-player-hidden_profile = Gracz ma ukryty profil
ui-replay-player-never_connected = Gracz nigdy nie połączył się z meczem

## ui.replay.build
ui-replay-build-open_shiptool = Otwórz build w ShipTool
ui-replay-build-open_in_ship_builder = Otwórz build w WoWs SB
ui-replay-build-copy_link = Kopiuj link do buildu
ui-replay-build-copy_short_link = Kopiuj krótki link do buildu
ui-replay-build-link_copied = Skopiowano link do buildu
ui-replay-build-open_wows_numbers = Otwórz stronę WoWs Numbers
ui-replay-build-not_spotted = Ten okręt nie został nigdy wykryty. Informacje o buildzie niedostępne.

## ui.replay.sections
ui-replay-sections-achievements = Osiągnięcia
ui-replay-sections-ribbons = Wstążki
ui-replay-sections-modules = Moduły:
ui-replay-sections-modules_none = Brak modułów
ui-replay-sections-signals = Sygnały:
ui-replay-sections-loadout = Wyposażenie:
ui-replay-sections-loadout_none = Brak wyposażenia
ui-replay-sections-captain_skills = Umiejętności kapitana:
ui-replay-sections-captain_skills_none = Brak umiejętności kapitana
ui-replay-sections-consumables = Sprzęt eksploatacyjny:
ui-replay-sections-fire_chance = Efektywna szansa na pożar
ui-replay-sections-fire_chance_expected = oczekiwane
ui-replay-sections-fire_chance_fires = { $fires ->
    [one] { $fires } pożar
    [few] { $fires } pożary
    [many] { $fires } pożarów
   *[other] { $fires } pożaru
}
ui-replay-sections-fire_chance_eligible_hits = { $hits ->
    [one] { $hits } trafienie, które mogło go wywołać
    [few] { $hits } trafienia, które mogły go wywołać
    [many] { $hits } trafień, które mogły go wywołać
   *[other] { $hits } trafienia, które mogło go wywołać
}
ui-replay-sections-fire_chance_ships = na { $ships ->
    [one] { $ships } okręcie celu
    [few] { $ships } okrętach celu
    [many] { $ships } okrętach celu
   *[other] { $ships } okrętu celu
}
ui-replay-sections-fire_chance_expected_fires = oczekiwane pożary: { $fires }
ui-replay-sections-fire_chance_per_ship = Na okręt celu
ui-replay-sections-fire_chance_no_eligible_hits = brak trafień, które mogłyby wywołać pożar
ui-replay-sections-fire_chance_formula = Wzór na szansę pożaru atakującego
ui-replay-sections-fire_chance_formula_base = bazowe burnProb
ui-replay-sections-fire_chance_formula_clamped = ograniczone
ui-replay-sections-fire_chance_breakdown = Szczegóły
ui-replay-sections-fire_chance_shells_fired = { $count ->
    [one] wystrzelony pocisk HE
    [few] wystrzelone pociski HE
    [many] wystrzelonych pocisków HE
   *[other] wystrzelonego pocisku HE
}
ui-replay-sections-fire_chance_he_hits = { $count ->
    [one] trafienie HE w okręt
    [few] trafienia HE w okręt
    [many] trafień HE w okręt
   *[other] trafienia HE w okręt
}
ui-replay-sections-fire_chance_no_target_ship = { $count ->
    [one] trafienie HE nieprzypisane do okrętu celu
    [few] trafienia HE nieprzypisane do okrętu celu
    [many] trafień HE nieprzypisanych do okrętu celu
   *[other] trafienia HE nieprzypisanego do okrętu celu
}
ui-replay-sections-fire_chance_eligible = mogły wywołać pożar
ui-replay-sections-fire_chance_ribbons = { $count ->
    [one] wstążka SetFire
    [few] wstążki SetFire
    [many] wstążek SetFire
   *[other] wstążki SetFire
}
ui-replay-sections-fire_chance_ribbons_credited = przypisane do pocisku
ui-replay-sections-fire_chance_ribbons_uncredited = nieprzypisane
ui-replay-sections-fire_chance_unattributed_no_burn_state = brak zmiany stanu pożaru do dopasowania
ui-replay-sections-fire_chance_unattributed_already_credited = wcześniejszy pożar wykorzystał już pobliskie trafienie
ui-replay-sections-fire_chance_unattributed_secondary_contest = równie dobrze mogła go wywołać nasza artyleria pomocnicza
ui-replay-sections-fire_chance_unattributed_hit_contest = każde nasze pobliskie trafienie było niejednoznaczne z innym
ui-replay-sections-fire_chance_unattributed_all_excluded = każde nasze pobliskie trafienie zostało już wykluczone
ui-replay-sections-fire_chance_unattributed_none_could_burn = żadne nasze pobliskie trafienie nie mogło wywołać pożaru
ui-replay-sections-fire_chance_unattributed_no_hit = żadne nasze trafienie nie wypadło w tym oknie czasowym
ui-replay-sections-fire_chance_not_applicable = ofiara już zniszczona, nie dotyczy
ui-replay-sections-fire_chance_copied = Skopiowano szczegóły szansy na pożar
ui-replay-sections-fire_chance_click_to_copy = Kliknij, aby skopiować te szczegóły
ui-replay-sections-fire_chance_exclusion_already_burning = sekcja już się pali
ui-replay-sections-fire_chance_exclusion_victim_build_unknown = nieznany build ofiary, strefy pożaru mogą być scalone
ui-replay-sections-fire_chance_exclusion_damage_control_active = aktywna Grupa naprawczo-ratunkowa
ui-replay-sections-fire_chance_exclusion_damage_control_unknown = nieznany stan grupy naprawczo-ratunkowej
ui-replay-sections-fire_chance_exclusion_observation_gap = luka w obserwacji
ui-replay-sections-fire_chance_exclusion_consumable_unreliable = niepewny model aktywacji sprzętu eksploatacyjnego
ui-replay-sections-fire_chance_exclusion_victim_fate_unknown = nieznany los ofiary
ui-replay-sections-fire_chance_exclusion_hit_type_does_not_roll = ten typ trafienia nie losuje pożaru
ui-replay-sections-fire_chance_exclusion_no_geometry = brak geometrii sekcji pożarowych
ui-replay-sections-fire_chance_exclusion_impact_unplaceable = nie udało się umiejscowić trafienia na dopasowanym okręcie
ui-replay-sections-fire_chance_exclusion_victim_pose_unknown = nieznane położenie ofiary w chwili trafienia
ui-replay-sections-fire_chance_exclusion_ambiguous_hit = niejednoznaczne z innym naszym trafieniem
ui-replay-sections-damage_events = Zdarzenia obrażeń

## ui.replay.debug
ui-replay-debug-view_raw_metadata = Zobacz surowe metadane gracza
ui-replay-debug-raw_metadata = Surowe metadane
ui-replay-debug-raw_results = Surowe wyniki
ui-replay-debug-mapped_results = Zmapowane wyniki
ui-replay-debug-player_metadata = Metadane gracza
ui-replay-debug-search = Szukaj
ui-replay-debug-previous_match = Poprzednie dopasowanie (Shift+Enter)
ui-replay-debug-next_match = Następne dopasowanie (Enter)

## ui.replay.controls
ui-replay-controls-window_title = Sterowanie powtórką
ui-replay-controls-unavailable = Sterowanie niedostępne (nie znaleziono commands.scheme.xml w plikach gry).

## ui.collab
ui-collab-session = Sesja
ui-collab-starting = Uruchamianie sesji...
ui-collab-session_active = Sesja aktywna
ui-collab-connected_to_session = Połączono z sesją
ui-collab-session_token = Token sesji:
ui-collab-toggle_visibility = Przełącz widoczność tokenu
ui-collab-copy_token = Kopiuj token
ui-collab-token_copied = Token skopiowany do schowka
ui-collab-copy_web_link = Kopiuj link webowy
ui-collab-web_link_copied = Link webowy skopiowany do schowka
ui-collab-copy_localhost_link = Kopiuj link localhost
ui-collab-localhost_copied = Link localhost skopiowany do schowka
ui-collab-connected_count = { $count } połączonych
ui-collab-promote_cohost = Awansuj na współgospodarza
ui-collab-permissions = Uprawnienia
ui-collab-lock_annotations = Zablokuj adnotacje
ui-collab-lock_settings = Zablokuj ustawienia
ui-collab-reset_overrides = Resetuj nadpisania klienta
ui-collab-reset_overrides_tooltip = Resetuj wszystkie zmiany ustawień wyświetlania klienta
ui-collab-leave = Opuść
ui-collab-role_host = Gospodarz
ui-collab-role_cohost = Współgospodarz
ui-collab-you = (ty)
ui-collab-display_name = Nazwa wyświetlana:
ui-collab-display_name_hint = Twoja nazwa...
ui-collab-enter_display_name = Najpierw podaj nazwę wyświetlaną
ui-collab-host_session = Utwórz sesję
ui-collab-start_session = Rozpocznij sesję
ui-collab-join_session = Dołącz do sesji
ui-collab-paste_and_join = Wklej token i dołącz
ui-collab-clipboard_empty = Schowek jest pusty
ui-collab-invalid_token = Nieprawidłowy token: { $error }
ui-collab-no_runtime = Nie udało się rozpocząć sesji: brak środowiska uruchomieniowego
ui-collab-shared_windows = Udostępnione okna
ui-collab-watching = Oglądanie
ui-collab-waiting_for_frames = Oczekiwanie na dane bitwy
ui-collab-open = Otwórz
ui-collab-open_for_everyone = Otwórz dla wszystkich
ui-collab-tactics_board = Tablica taktyczna

## battle
battle-victory = ZWYCIĘSTWO
battle-defeat = PORAŻKA
battle-draw = REMIS
battle-battle_starts_in = BITWA ZACZYNA SIĘ ZA

## finish_type
finish_type-unknown = Bitwa zakończona
finish_type-extermination = Wszystkie okręty wroga zniszczone
finish_type-base_captured = Baza zdobyta
finish_type-timeout = Czas wygasł
finish_type-failure = Misja nieudana
finish_type-technical = Zakończenie techniczne
finish_type-score = Osiągnięto limit punktów
finish_type-score_on_timeout = Przewaga punktowa po upływie czasu
finish_type-pve_success = Misja zakończona sukcesem
finish_type-pve_failed = Misja nieudana
finish_type-score_zero = Punkty wyczerpane
finish_type-score_excess = Przekroczono limit punktów

## ui.stats
ui-stats-overview = Przegląd
ui-stats-sample_notice = Dane przykładowe, nie zapisane gry
ui-stats-div_all = Wszystkie
ui-stats-div_solo = Solo
ui-stats-div_div = Dywizja
ui-stats-filters = Filtry
ui-stats-own_filters = Osobne filtry dla tego wykresu
ui-stats-own_filters_tooltip = Zawęź ten wykres bez zmiany zawartości pozostałych paneli
ui-stats-mode_label = Tryb:
ui-stats-clear = Wyczyść
ui-stats-pr = PR:
ui-stats-mc = MC:
ui-stats-best_frags = Najlepszy: { $ship } ({ $count ->
    [one] { $count } zniszczenie
    [few] { $count } zniszczenia
    [many] { $count } zniszczeń
   *[other] { $count } zniszczenia
})
ui-stats-max_damage = Maks. obrażenia: { $ship } ({ $damage })
ui-stats-copy_markdown = Kopiuj jako Markdown
ui-stats-copy_image = Kopiuj jako obraz
ui-stats-copy_image_done = Skopiowano wykres do schowka
ui-stats-copy_image_failed = Nie udało się skopiować wykresu
ui-stats-copy_csv = Kopiuj jako CSV
ui-stats-remove_games_hint = Usuń wszystkie gry na { $ship } (Ctrl+klik, aby pominąć potwierdzenie)
ui-stats-settings = Ustawienia
ui-stats-options = Opcje
ui-stats-ships = Okręty
ui-stats-filter_ships = Filtruj okręty na wykresie
ui-stats-all_ships = Wszystkie
ui-stats-no_ships = Brak
ui-stats-search_ships = Szukaj okrętów...
ui-stats-no_ship_matches = Żaden okręt nie pasuje do wyszukiwania
ui-stats-search_statistics = Szukaj statystyk
ui-stats-no_statistic_matches = Żadna statystyka nie pasuje do wyszukiwania
ui-stats-ship_selection_count = Wybrano { $selected } z { $total }
ui-stats-all_ships_tooltip = Zaznacz wszystkie pasujące okręty
ui-stats-no_ships_tooltip = Odznacz wszystkie pasujące okręty
ui-stats-limit_to_recent = Ogranicz do ostatnich gier
ui-stats-limit_to_recent_tooltip = Limit dotyczy każdego okrętu osobno w zakładkach Okręty i Wykresy oraz wszystkich gier łącznie w Przeglądzie.
ui-stats-division_label = Dywizjon:
ui-stats-add_chart = Dodaj wykres
ui-stats-clear_tooltip = Usuń wszystkie zapisane gry
ui-stats-clear_failed = Nie wyczyszczono: { $reason }
ui-stats-clear_ship_confirm = Naciśnij ponownie, aby usunąć gry tego okrętu
ui-stats-copy_table = Kopiuj tę tabelę
ui-stats-copied = Skopiowano do schowka
ui-stats-no_games = Brak zapisanych gier dla bieżących filtrów
ui-stats-nothing_to_plot = Brak danych do wykresu dla bieżących filtrów
ui-stats-column_ship = Okręt
ui-stats-column_battles = Bitwy
ui-stats-column_win_rate = Wskaźnik wygranych
ui-stats-column_avg_damage = Śr. obrażenia
ui-stats-column_avg_frags = Śr. fragi
ui-stats-frags = { $count ->
    [one] { $count } fraga
    [few] { $count } fragi
    [many] { $count } fragów
   *[other] { $count } fraga
}
ui-stats-statistic = Statystyka
ui-stats-open_directory_stats = Statystyki katalogu
ui-stats-open_directory_stats_tooltip = Otwórz statystyki wszystkich powtórek w tym katalogu i jego podkatalogach
ui-stats-directory_stats_unavailable = Dane gry są nadal wczytywane
ui-stats-directory_reading_task = Odczytywanie powtórek do statystyk katalogu...
ui-stats-directory_walking = Wyszukiwanie powtórek...
ui-stats-directory_scanning = Sprawdzanie { $done } z { $total ->
    [one] { $total } powtórki...
    [few] { $total } powtórek...
    [many] { $total } powtórek...
   *[other] { $total } powtórki...
}
ui-stats-directory_reading = Odczytywanie { $done } z { $total ->
    [one] { $total } powtórki...
    [few] { $total } powtórek...
    [many] { $total } powtórek...
   *[other] { $total } powtórki...
}
ui-stats-directory_reload = Wczytaj ponownie
ui-stats-directory_reload_tooltip = Odczytaj ponownie powtórki z tego katalogu
ui-stats-directory_skipped = { $count ->
    [one] Pominięto { $count } powtórkę
    [few] Pominięto { $count } powtórki
    [many] Pominięto { $count } powtórek
   *[other] Pominięto { $count } powtórki
}
ui-stats-directory_skipped_detail =
    Nieczytelne: { $unreadable }
    Brak danych gry dla ich builda: { $missing_build }
    Brak wyników bitwy: { $no_results }
ui-stats-chart = Wykres
ui-stats-combine_ships = Połącz okręty
ui-stats-running_average = Średnia krocząca
ui-stats-value_labels = Etykiety wartości
ui-stats-reset_view = Resetuj widok
ui-stats-keep_current_layout = Zachowaj bieżący układ
ui-stats-session_title = Statystyki sesji
ui-stats-session_manual = Ręcznie
ui-stats-session_since_opened = Od uruchomienia aplikacji
ui-stats-session_manual_tooltip = Sesja zawiera rozegrane bitwy oraz powtórki ustawione lub dodane z listy powtórek. Jest zachowywana między uruchomieniami.
ui-stats-session_since_opened_tooltip = Sesja zawiera każdą bitwę rozpoczętą o { $time } lub później, gdy aplikacja została uruchomiona. Twoja ręczna sesja jest zachowana i wraca po przełączeniu na Ręcznie.
ui-stats-session_battles = { $count ->
    [one] { $count } bitwa
    [few] { $count } bitwy
    [many] { $count } bitew
   *[other] { $count } bitwy
}
ui-stats-session_chip_manual = Sesja: { $battles }
ui-stats-session_chip_since = Sesja od { $time }: { $battles }
ui-stats-session_empty_title = Brak bitew w tej sesji
ui-stats-session_empty_manual = Rozegrane bitwy dołączają do sesji po zakończeniu. Możesz też ustawić lub dodać powtórki z menu listy powtórek.
ui-stats-session_empty_since = Bitwy rozpoczęte po { $time } dołączają do sesji po zakończeniu.
ui-stats-session_set_add_unavailable = Sesja zawiera bitwy od { $time }. Przełącz na Ręcznie, aby ustawić lub dodać powtórki.
ui-stats-clear_unavailable_since = Sesja od uruchomienia aplikacji nie jest zapisywana. Przełącz na Ręcznie, aby wyczyścić zapisaną sesję.
ui-stats-charts_save_failed = Nie zapisano ustawień wykresów: { $reason }
ui-stats-layout_save_failed = Nie zapisano układu wykresów: { $reason }
ui-stats-axis_game = Gra
ui-stats-chart_hover_game = Gra { $number }, { $time }
ui-stats-series_average = { $stat } (średnia)
ui-stats-series_running = { $stat } (krocząca)
ui-stats-series_more = +{ $count } więcej
ui-stats-mode_random = Losowe
ui-stats-mode_ranked = Rankingowe
ui-stats-mode_other = Inne
ui-stats-win_rate_rated_only = Wskaźnik wygranych obejmuje tylko bitwy losowe i rankingowe
ui-stats-win_rate_breakdown_line = { $mode }: { $rate } (bitwy: { $battles })
ui-stats-matches = Bitwy
ui-stats-column_time = Czas
ui-stats-column_mode = Tryb
ui-stats-column_result = Wynik
ui-stats-keep_game = Uwzględnij tę bitwę w statystykach
ui-stats-leave_out_game = Pomiń tę bitwę w statystykach
ui-stats-leave_out_ship = Pomiń wszystkie bitwy na { $ship }
ui-stats-leave_out_ship_hint = Pomiń { $ship } w statystykach
ui-stats-keep_all_games = Uwzględnij wszystkie bitwy
ui-stats-keep_all_games_tooltip = Ponownie uwzględnij wszystkie ręcznie pominięte bitwy
ui-stats-left_out_only = Tylko pominięte
ui-stats-match_count = { $shown } z { $total ->
    [one] { $total } bitwy
    [few] { $total } bitew
    [many] { $total } bitew
   *[other] { $total } bitwy
}, pominięto { $left_out }
ui-stats-no_matches = Żadne bitwy nie pasują do bieżących filtrów
ui-stats-search_matches = Szukaj według okrętu...
ui-stats-ship_filter_tooltip = Wybierz, które okręty są uwzględniane w statystykach
ui-stats-keep_ships_tooltip = Uwzględnij wszystkie pasujące okręty
ui-stats-leave_out_ships_tooltip = Pomiń wszystkie pasujące okręty w statystykach
ui-stats-dates = Daty
ui-stats-all_dates = Wszystkie daty
ui-stats-clear_dates = Pokaż wszystkie daty
ui-stats-games_left_out = { $count ->
    [one] Pominięto { $count } bitwę
    [few] Pominięto { $count } bitwy
    [many] Pominięto { $count } bitew
   *[other] Pominięto { $count } bitwy
}
ui-stats-reset_filters = Resetuj filtry
ui-stats-reset_filters_tooltip = Pokaż wszystkie bitwy: bez limitu, zakresu dat, dywizjonu, trybu ani pominiętych okrętów i bitew

## ui.stats.table
ui-stats-table-min = Min
ui-stats-table-max = Maks
ui-stats-table-total = Łącznie
ui-stats-table-average = Średnia

## ui.player_tracker
ui-player_tracker-clear_stats = Wyczyść statystyki
ui-player_tracker-populate_from_replays = Wypełnij danymi z powtórek
ui-player_tracker-find_matches = Znajdź mecze
ui-player_tracker-open_wows_numbers = Otwórz na wows-numbers
ui-player_tracker-open_shipbuilds = Otwórz buildy na shipbuilds
ui-player_tracker-subtab_players = Gracze
ui-player_tracker-filter_hint = Filtruj po graczu lub klanie...
ui-player_tracker-time_period = Okres
ui-player_tracker-allies = Sojusznicy
ui-player_tracker-enemies = Przeciwnicy
ui-player_tracker-live_no_directory = Ustaw katalog World of Warships, aby obserwować bitwy
ui-player_tracker-live_checking = Sprawdzanie, czy trwa bitwa...
ui-player_tracker-live_none = Brak trwającej bitwy
ui-player_tracker-live_loading_build = Wczytywanie danych okrętów dla tego builda. Nazwy i klasy pojawią się po ich wczytaniu.
ui-player_tracker-waiting_for_index = Oczekiwanie na indeks powtórek
ui-player_tracker-loading_clans = Wczytywanie klanów...
ui-player_tracker-loading_players = Wczytywanie graczy...
ui-player_tracker-index_failed = Nie udało się odczytać indeksu: { $reason }
ui-player_tracker-no_players = Brak zindeksowanych graczy w tym okresie
ui-player_tracker-note_not_saved = Nie zapisano: { $reason }
ui-player_tracker-subtab_current_match = Bieżący mecz
ui-player_tracker-encounters_hover = Widziany łącznie { $total ->
    [one] { $total } raz
    [few] { $total } razy
    [many] { $total } razy
   *[other] { $total } raza
}, { $range } w wybranym zakresie czasu. Ostatnio widziany { $last }.
ui-player_tracker-never_encountered = Wcześniej niewidziany
ui-player_tracker-copy_wg_id = Kliknij, aby skopiować ID WG
ui-player_tracker-copied_wg_id = Skopiowano ID WG { $id }
ui-player_tracker-aliases_hover = Znany także jako: { $names }
ui-player_tracker-notes_hint = Notatki o tym graczu
ui-player_tracker-arena_count = { $count ->
    [one] { $count } zapisany mecz
    [few] { $count } zapisane mecze
    [many] { $count } zapisanych meczów
   *[other] { $count } zapisanego meczu
}
ui-player_tracker-last_encountered_exact = Ostatnie spotkanie { $timestamp }
ui-player_tracker-subtab_clans = Klany
ui-player_tracker-clan_no_data = Nie zapisano jeszcze żadnych spotkań z klanami.
ui-player_tracker-find_clan_matches = Znajdź mecze zawierające ten tag. Indeks nie ma pola wyłącznie dla klanu, więc pasują też nazwy graczy zawierające ten tag.
ui-player_tracker-clan_sightings_hover = { $range } z nich mieści się w wybranym zakresie czasu.
ui-player_tracker-clan_member_matches = widziany w { $count ->
    [one] { $count } meczu
    [few] { $count } meczach
    [many] { $count } meczach
   *[other] { $count } meczu
}
ui-player_tracker-show_division_mates = Pokaż towarzyszy z dywizjonu
ui-player_tracker-show_division_mates_hover = Licz mecze rozegrane w dywizjonie. Domyślnie wyłączone, ponieważ tych graczy spotkałeś tam dlatego, że sam ich zaprosiłeś. Pozostałe mecze przeciwko tym samym graczom są zawsze liczone.
ui-player_tracker-seen_in_twitch_chat = Widziany na czacie Twitch
ui-player_tracker-seen_in_twitch_chat_hover = Pokaż tylko graczy, których nazwa pasowała do loginu z czatu Twitch podczas meczu z tobą.
ui-player_tracker-twitch_seen_hover = Widziany na czacie Twitch jako { $logins } w { $count ->
    [one] { $count } meczu
    [few] { $count } meczach
    [many] { $count } meczach
   *[other] { $count } meczu
} z tobą
ui-player_tracker-win_rate_overall = Ogółem
ui-player_tracker-win_rate_ship = Okręt
ui-player_tracker-view_compact = Kompaktowy
ui-player_tracker-view_detailed = Szczegółowy
ui-player_tracker-ship_stats = Statystyki okrętu
ui-player_tracker-stats_resolving = Odczytywanie składu meczu...
ui-player_tracker-stats_fetching = Pobieranie statystyk graczy...
ui-player_tracker-stats_unavailable = Statystyki niedostępne: { $reason }
ui-player_tracker-stats_hidden = Profil tego gracza jest prywatny
ui-player_tracker-stats_no_data = Brak statystyk dla tego gracza
ui-player_tracker-pr_overall_hover = PR konta ze wszystkich okrętów
ui-player_tracker-pr_ship_hover = PR na tym okręcie
ui-player_tracker-win_rate_hover_line = { $label }: { $rate } w { $count ->
    [one] { $battles } bitwie
    [few] { $battles } bitwach
    [many] { $battles } bitwach
   *[other] { $battles } bitwy
}
ui-player_tracker-win_rate_hover_line_damage = { $label }: { $rate } w { $count ->
    [one] { $battles } bitwie
    [few] { $battles } bitwach
    [many] { $battles } bitwach
   *[other] { $battles } bitwy
}, średnie obrażenia { $damage }
ui-player_tracker-team_average_win_rate = Śr. WR: { $rate }
ui-player_tracker-team_players = { $count ->
    [one] { $count } gracz
    [few] { $count } graczy
    [many] { $count } graczy
   *[other] { $count } gracza
}
ui-player_tracker-debug_replay_picker = Debug: traktuj powtórkę jako bieżący mecz
ui-player_tracker-debug_replay_picker_hover = Wczytuje skład zakończonej powtórki do tej karty, aby można było przetestować ścieżkę statystyk poza bitwą.
ui-player_tracker-debug_replay_none = Wybierz powtórkę

## ui.player_tracker.column
ui-player_tracker-column-win_rate = WR
ui-player_tracker-column-personal_rating = PR
ui-player_tracker-column-battles = Bitwy
ui-player_tracker-column-avg_damage = Obr.
ui-player_tracker-column-encounters = Spotkania
ui-player_tracker-column-clan = Klan
ui-player_tracker-column-player_name = Nazwa gracza
ui-player_tracker-column-total_encounters = Łączne spotkania
ui-player_tracker-column-encounters_in_range = Spotkania w zakresie czasu
ui-player_tracker-column-last_encountered = Ostatnie spotkanie
ui-player_tracker-column-members = Członkowie
ui-player_tracker-column-matches = Mecze
ui-player_tracker-column-sightings = Wystąpienia

## ui.player_tracker.period
ui-player_tracker-period-past_hour = Ostatnia godzina
ui-player_tracker-period-past_six_hours = Ostatnie 6 godzin
ui-player_tracker-period-past_day = Ostatnie 24 godziny
ui-player_tracker-period-past_week = Ostatni tydzień
ui-player_tracker-period-past_month = Ostatni miesiąc
ui-player_tracker-period-all_time = Od początku

## ui.twitch
ui-twitch-possible_name = Możliwa nazwa streamu: { $name }
ui-twitch-seen_minutes = Widziany: { $minutes } minut po rozpoczęciu meczu
ui-twitch-click_to_copy = Kliknij, aby skopiować
ui-twitch-copied = Skopiowano { $name }

## ui.renderer
ui-renderer-preparing_export = Przygotowywanie eksportu...
ui-renderer-encoding = Kodowanie
ui-renderer-muxing = Multipleksowanie

## ui.renderer.advantage
ui-renderer-advantage-breakdown = Podział przewagi
ui-renderer-advantage-team_eliminated = Drużyna została wyeliminowana
ui-renderer-advantage-score_projection = Prognoza punktów
ui-renderer-advantage-fleet_power = Siła floty
ui-renderer-advantage-strategic_threat = Zagrożenie strategiczne
ui-renderer-advantage-total = Łącznie
ui-renderer-advantage-hp_incomplete = Dane HP/okrętów niekompletne. Pokazano tylko czynniki punktowe

## ui.renderer.context
ui-renderer-context-show_trail = Pokaż ślad
ui-renderer-context-disable_other_trails = Wyłącz wszystkie inne ślady
ui-renderer-context-ranges = Zasięgi
ui-renderer-context-detection = Wykrywalność
ui-renderer-context-main_battery = Artyleria główna
ui-renderer-context-secondary = Artyleria pomocnicza
ui-renderer-context-torpedo = Torpedy
ui-renderer-context-radar = Radar
ui-renderer-context-hydro = Hydroakustyka
ui-renderer-context-enable_all = Włącz wszystkie
ui-renderer-context-disable_all = Wyłącz wszystkie
ui-renderer-context-disable_other_ranges = Wyłącz wszystkie inne zasięgi
ui-renderer-context-enable_all_ranges = Włącz zasięgi wszystkich okrętów
ui-renderer-context-realtime_armor = Pokaż pancerz w czasie rzeczywistym

## ui.renderer.controls
ui-renderer-controls-jump_to_start = Skocz na początek
ui-renderer-controls-previous_event = Poprzednie zdarzenie (Shift+Left)
ui-renderer-controls-back_10s = Cofnij 10s (Left)
ui-renderer-controls-forward_10s = Naprzód 10s (Right)
ui-renderer-controls-next_event = Następne zdarzenie (Shift+Right)
ui-renderer-controls-jump_to_end = Skocz na koniec
ui-renderer-controls-speed = Prędkość odtwarzania

## ui.renderer.settings
ui-renderer-settings-title = Ustawienia wyświetlania
ui-renderer-settings-hp_bars = Paski HP
ui-renderer-settings-armament = Uzbrojenie
ui-renderer-settings-dead_ships = Zniszczone okręty
ui-renderer-settings-dead_ship_names = Nazwy zniszczonych okrętów
ui-renderer-settings-player_names = Nazwy graczy
ui-renderer-settings-ship_ranges = Zasięgi okrętów
ui-renderer-settings-ship_names = Nazwy okrętów
ui-renderer-settings-camera_direction = Kierunek wieżyczek
ui-renderer-settings-heat_trail = Ślad cieplny
ui-renderer-settings-speed_trails = Ślady prędkości
ui-renderer-settings-dead_ship_trails = Ślady zniszczonych okrętów
ui-renderer-settings-buildings = Budynki
ui-renderer-settings-capture_points = Punkty zdobycia
ui-renderer-settings-consumables = Materiały eksploatacyjne
ui-renderer-settings-planes = Samoloty
ui-renderer-settings-smoke = Dym
ui-renderer-settings-torpedoes = Torpedy
ui-renderer-settings-tracers = Smugi pocisków
ui-renderer-settings-battle_result = Wynik bitwy
ui-renderer-settings-buff_counters = Liczniki wzmocnień
ui-renderer-settings-chat_label = Czat
ui-renderer-settings-kill_feed = Lista zabójstw
ui-renderer-settings-score_label = Wynik
ui-renderer-settings-score_timers = Liczniki punktów
ui-renderer-settings-team_advantage = Przewaga drużyny
ui-renderer-settings-team_rosters = Składy drużyn
ui-renderer-settings-stats_panel = Panel statystyk
ui-renderer-settings-timer = Licznik czasu
ui-renderer-settings-export_settings = Ustawienia eksportu
ui-renderer-settings-prefer_cpu = Preferuj koder CPU
ui-renderer-settings-prefer_cpu_tooltip = Użyj kodera programowego (CPU) zamiast GPU do eksportu wideo
ui-renderer-settings-include_pre_battle = Uwzględnij fazę przed bitwą
ui-renderer-settings-include_pre_battle_tooltip = Uwzględnij w eksportowanym wideo pojawienie się okrętów i odliczanie przed rozpoczęciem bitwy
ui-renderer-settings-export_timing = Tempo wideo
ui-renderer-settings-export_timing_length = Dopasuj do długości
ui-renderer-settings-export_timing_speed = Prędkość
ui-renderer-settings-export_timing_result_speed = Odtwarzanie w około { $speed }x
ui-renderer-settings-export_timing_result_length = Wideo trwa { $length }
ui-renderer-settings-codec = Kodek
ui-renderer-settings-codec_auto = Automatyczny
ui-renderer-settings-save_defaults = Zapisz jako domyślne
ui-renderer-settings-save_defaults_done = Zapisano jako ustawienia początkowe renderera
ui-renderer-settings-save_defaults_failed = Nie udało się zapisać ustawień domyślnych
ui-renderer-settings-event_timeline = Oś czasu zdarzeń

## ui.unpacker
ui-unpacker-assets_bin = Assets.bin
ui-unpacker-loading_assets = Wczytywanie assets.bin...
ui-unpacker-load_failed = Nie udało się wczytać: { $error }
ui-unpacker-no_game_data = Nie wczytano danych gry. Ustaw katalog World of Warships w Ustawieniach.
ui-unpacker-filter_files = Filtruj pliki...
ui-unpacker-search_in_files = Szukaj w plikach
ui-unpacker-search_hint = Szukaj w plikach...
ui-unpacker-path_filter_hint = Filtr ścieżki (np. *.xml)
ui-unpacker-queue_all_files = Kolejkuj wszystkie pliki
ui-unpacker-view_as_json = Wyświetl jako JSON
ui-unpacker-extract_as_json = Wyodrębnij jako JSON
ui-unpacker-view_contents = Wyświetl zawartość
ui-unpacker-version = Wersja:
ui-unpacker-decode_prototypes = Dekoduj prototypy jako JSON
ui-unpacker-decode_prototypes_tooltip = Dekoduj obsługiwane prototypy Assets.bin (.visual, .model, .mfm) do JSON podczas wyodrębniania
ui-unpacker-extraction_queue = Kolejka wyodrębniania
ui-unpacker-collapse_folders = Zwiń drzewo folderów
ui-unpacker-expand_folders = Rozwiń drzewo folderów
ui-unpacker-clear_all = Wyczyść wszystko
ui-unpacker-browse = Przeglądaj
ui-unpacker-output_dir_hint = Katalog wyjściowy...
ui-unpacker-remove_from_queue = Usuń z kolejki
ui-unpacker-queue_folder = Kolejkuj cały folder
ui-unpacker-queue_file = Dodaj ten plik do kolejki
ui-unpacker-extraction_output = Wyodrębnij do
ui-unpacker-queue_empty = Zaznacz plik lub folder na liście, aby dodać go do kolejki
ui-unpacker-packages = Pakiety
ui-unpacker-item_count = { $count ->
    [one] { $count } element
    [few] { $count } elementy
    [many] { $count } elementów
   *[other] { $count } elementu
}
ui-unpacker-search_title = Wyszukiwanie: { $query }
ui-unpacker-search_in = w { $source }
ui-unpacker-image_decode_failed = Nie udało się zdekodować obrazu: { $reason }
ui-unpacker-find = Znajdź
ui-unpacker-find_previous = Poprzednie dopasowanie
ui-unpacker-find_next = Następne dopasowanie
ui-unpacker-find_close = Zamknij wyszukiwanie
ui-unpacker-find_count = { $current } z { $total }
ui-unpacker-find_none = Brak dopasowań
ui-unpacker-find_match_case = Uwzględniaj wielkość liter
ui-unpacker-queue = Kolejka
ui-unpacker-reveal_in_tree = Pokaż w drzewie pakietów
ui-unpacker-hit_offset = w { $offset }
ui-unpacker-queued_count = W kolejce: { $count }
ui-unpacker-counting_files = Liczenie plików...
ui-unpacker-extracting = Wyodrębnianie { $done } z { $total }
ui-unpacker-extract_failed = Wyodrębnianie nie powiodło się: { $reason }
ui-unpacker-extracted = Wyodrębniono { $count ->
    [one] { $count } plik
    [few] { $count } pliki
    [many] { $count } plików
   *[other] { $count } pliku
}
ui-unpacker-cancelled = Anulowano po { $count ->
    [one] { $count } pliku
    [few] { $count } plikach
    [many] { $count } plikach
   *[other] { $count } pliku
}
ui-unpacker-extract = { $count ->
    [0] Wyodrębnij
    [one] Wyodrębnij { $count } element
    [few] Wyodrębnij { $count } elementy
    [many] Wyodrębnij { $count } elementów
   *[other] Wyodrębnij { $count } elementu
}
ui-unpacker-choose_directory = Najpierw wybierz katalog docelowy
ui-unpacker-dump_parameters = Eksportuj GameParams
ui-unpacker-dump_parameters_tooltip = Eksportuj GameParams.data dla tego builda
ui-unpacker-load_a_build = Najpierw wczytaj build
ui-unpacker-parameters_written = Zapisano parametry
ui-unpacker-save_parameters_title = Zapisz parametry gry
ui-unpacker-writing = Zapisywanie...
ui-unpacker-prototype_written = Zapisano prototyp
ui-unpacker-whole_tree = Całe drzewo
ui-unpacker-base_only = Tylko parametry bazowe

## ui.unpacker.column
ui-unpacker-column-name = Nazwa
ui-unpacker-column-size = Rozmiar
ui-unpacker-column-type_col = Typ

## ui.armor
ui-armor-load_settings = Wczytaj dane gry w Ustawieniach, aby użyć Przeglądarki pancerza.
ui-armor-load_failed = Nie udało się wczytać danych okrętu: { $error }
ui-armor-empty = Pusty
ui-armor-mirror_cameras = Lustrzane kamery
ui-armor-sync_options = Synchronizuj opcje
ui-armor-show_armor_by_default = Domyślnie pokazuj pancerz
ui-armor-sync_tooltip = Synchronizuj widoczność pancerza/kadłuba oraz nakładki wyświetlania (środek okrętu, linia wodna, pierścienie kamery) we wszystkich panelach
ui-armor-common_settings = Wspólne ustawienia
ui-armor-tier_filter = Poziomy od { $lowest } do { $highest }
ui-armor-fortifications = Fortyfikacje
ui-armor-show_hidden_ships = Pokaż ukryte okręty
ui-armor-show_hidden_ships_tooltip = Wyświetlaj też okręty pomijane na liście, np. okręty klanowe do wypożyczenia
ui-armor-compare_split = Porównaj w nowym panelu
ui-armor-open_in_new_tab = Otwórz w nowej karcie
ui-armor-export_model = Eksportuj model okrętu
ui-armor-export_disclaimer = Te modele 3D i tekstury są własnością intelektualną Wargaming i jakiekolwiek użycie tych modeli powinno być zgodne z lokalnymi przepisami prawa.
ui-armor-export_button = Eksportuj
ui-armor-hull_toggle = Kadłub
ui-armor-splash_tooltip = Przełącz widoczność strefy rozbryzgu
ui-armor-show_hidden = Pokaż ukryte
ui-armor-show_hidden_tooltip = Przełącz widoczność (prawdopodobnie) ukrytych paneli
ui-armor-gaps = Luki
ui-armor-gaps_tooltip = Podświetl krawędzie graniczne, gdzie panele pancerza się nie stykają (potencjalne luki)
ui-armor-display = Wyświetlanie
ui-armor-marker_opacity = Przezroczystość znaczników
ui-armor-export_tooltip = Eksportuj model okrętu do pliku OBJ
ui-armor-pen_check = Sprawdzenie penetracji
ui-armor-trajectory = Trajektoria
ui-armor-trajectory_range = Dystans
ui-armor-trajectory_clear = Wyczyść wszystko
ui-armor-arc_heading = Tor { $number }
ui-armor-arc_summary = { $hits ->
    [one] { $hits } trafienie
    [few] { $hits } trafienia
    [many] { $hits } trafień
   *[other] { $hits } trafienia
}, { $armor } mm z { $range } km
ui-armor-arc_delete = Usuń ten tor
ui-armor-arc_isolate_plates = Wyizoluj płyty
ui-armor-arc_isolate_plates_tooltip = Pokaż tylko płyty pancerza, przez które przeszedł ten tor
ui-armor-arc_isolate_zones = Wyizoluj strefy
ui-armor-arc_isolate_zones_tooltip = Pokaż wszystkie części stref pancerza, do których wszedł ten tor
ui-armor-arc_simulation_caveat = Ta symulacja opiera się na danych z inżynierii wstecznej i może nie odpowiadać sposobowi, w jaki gra oblicza balistykę.
ui-armor-arc_no_shell = Nie wybrano pocisku. Przecięte płyty są pokazane bez niego
ui-armor-arc_detonated_in = Detonacja wewnątrz: { $zone }
ui-armor-arc_detonated_past_armor = Detonacja za całym pancerzem
ui-armor-arc_ricochet = Rykoszet na { $plate }
ui-armor-arc_shatter = Brak penetracji na { $plate }
ui-armor-arc_stopped = Zatrzymany na { $plate }
ui-armor-arc_overpen = Przebicie na wylot
ui-armor-arc_overpen_unarmed = Przebicie na wylot (zapalnik nie uzbroił się)
ui-armor-arc_plate_unknown = niezapisanej płycie
ui-armor-angle_good = poniżej 30 deg
ui-armor-angle_mid = od 30 do 45 deg
ui-armor-angle_bad = powyżej 45 deg
ui-armor-trajectory_ship = Okręt:
ui-armor-trajectory_select_ship = Wybierz okręt
ui-armor-trajectory_ship_tooltip = Wybierz okręt, którego pocisk zostanie użyty do trajektorii
ui-armor-trajectory_add_ship = Dodaj strzelający okręt
ui-armor-trajectory_add_ship_tooltip = Otwórz Sprawdzenie penetracji, aby wyszukać i dodać okręt
ui-armor-trajectory_shell = Pocisk:
ui-armor-trajectory_select_shell = Wybierz pocisk
ui-armor-trajectory_shell_tooltip = Wybierz pocisk używany do symulacji trajektorii
ui-armor-continue_ricochet = Kontynuuj po rykoszecie
ui-armor-splash_mode = Rozbryzg
ui-armor-splash_no_data = Brak danych o rozbryzgu dla tego okrętu
ui-armor-splash_no_shells = Dodaj okręt z pociskami HE/SAP do listy porównania
ui-armor-splash_tooltip_mode = Kliknij pancerz, aby zwizualizować objętość obrażeń rozbryzgu HE
ui-armor-select_ship = Wybierz okręt z listy
ui-armor-all_btn = Wszystkie
ui-armor-none_btn = Brak
ui-armor-opaque = Nieprzezroczysty
ui-armor-upgrade = Ulepszenie:
ui-armor-camo_none = Podstawowy (brak)
ui-armor-camo_group_universal = Kamuflaże uniwersalne
ui-armor-camo_group_expendable = Kamuflaże jednorazowe
ui-armor-camo_group_other = Inne kamuflaże
ui-armor-show_armor_thickness = Pokaż legendę grubości pancerza
ui-armor-plate_edges = Krawędzie płyt
ui-armor-waterline = Linia wodna
ui-armor-camera_rings = Pierścienie kamery
ui-armor-realtime_readout = { $health } HP  -  { $hits ->
    [one] { $hits } trafienie
    [few] { $hits } trafienia
    [many] { $hits } trafień
   *[other] { $hits } trafienia
}
ui-armor-show_camera_rings = Pokaż pierścienie kamery
ui-armor-camera_fov = FOV kamery
ui-armor-camera_height = Wysokość kamery
ui-armor-show_camera_zoom_path = Pokaż ścieżkę przybliżenia
ui-armor-zoom_path_regular_fov = Zwykłe FOV
ui-armor-zoom_path_max_fov = Maks. FOV
ui-armor-camera_perspective = Perspektywa kamery
ui-armor-perspective_fov = FOV widoku
ui-armor-perspective_projection = Projekcja
ui-armor-perspective_proj_game = Jak w grze
ui-armor-perspective_proj_center = Przez środek
ui-armor-ship_center = Środek okrętu
ui-armor-grid = Siatka
ui-armor-length_marks = Znaczniki długości
ui-armor-length_readout = Długość { $length }, szerokość { $beam }
ui-armor-length_metres = { $value } m
ui-armor-length_centimetres = { $value } cm
ui-armor-filter_options = Filtruj opcje...
ui-armor-filter_zones = Filtruj strefy...
ui-armor-filter_parts = Filtruj części...
ui-armor-tree_matches = Dopasowania: { $count }
ui-armor-tree_no_matches = Brak dopasowań
ui-armor-no_matching_options = Żadna opcja nie pasuje do filtra
ui-armor-opacity = Przezroczystość
ui-armor-opaque_hull = Nieprzezroczysty kadłub
ui-armor-lighting = Oświetlenie kadłuba
ui-armor-lighting_enabled = Włącz oświetlenie
ui-armor-lighting_preset_ingame = W grze
ui-armor-lighting_preset_flat = Płaskie
ui-armor-lighting_preset_studio = Studyjne
ui-armor-lighting_flat = Płaskie (otoczenie)
ui-armor-lighting_intensity = Intensywność światła
ui-armor-lighting_azimuth = Azymut światła
ui-armor-lighting_elevation = Elewacja światła
ui-armor-lighting_rim = Konturowe
ui-armor-lighting_specular = Połysk
ui-armor-lighting_shininess = Ostrość połysku
ui-armor-lighting_key_color = Kolor światła głównego
ui-armor-lighting_ambient_color = Kolor otoczenia
ui-armor-zero_mm_plates = Płyty 0 mm
ui-armor-armor_opacity = Przezroczystość pancerza
ui-armor-ballistic_disclaimer = Wyniki balistyczne opierają się na inżynierii wstecznej/oszacowaniach działania symulacji
ui-armor-roll = Przechył
ui-armor-reset_plates = Resetuj płyty
ui-armor-armor_options = Pancerz
ui-armor-no_ship_loaded = Nie wczytano okrętu
ui-armor-lod = LOD:
ui-armor-lod_highest = 0 (najwyższy)
ui-armor-camouflage = Kamuflaż
ui-armor-ships = Okręty
ui-armor-compare = Porównaj
ui-armor-compare_tooltip = Otwórz nowy panel porównania lub kliknij okręt prawym przyciskiem, aby go porównać
ui-armor-mirror_cameras_tooltip = Poruszaj kamerami wszystkich paneli jednocześnie
ui-armor-show_armor_by_default_tooltip = Otwieraj modele z widocznym pancerzem zamiast kadłuba
ui-armor-xray_highlight = Podświetlenie rentgenowskie po najechaniu w widoku
ui-armor-xray_always = Zawsze
ui-armor-xray_while_held = Podczas przytrzymania { $key }
ui-armor-xray_off = Wyłączone
ui-armor-stack_panes = Panele w pionie
ui-armor-stack_panes_tooltip = Układaj panele porównania jeden nad drugim zamiast obok siebie
ui-armor-viewports = Widoki
ui-armor-options_panel = Panel opcji
ui-armor-options_pane = Panel { $number } z { $count }
ui-armor-splash_boxes = Strefy rozbryzgu
ui-armor-cast_here_hint = Przytrzymaj { $key } i kliknij, aby wystrzelić tu pocisk
ui-armor-trajectory_next_cast = Następny strzał
ui-armor-arc_pick_hint = Kliknij znacznik, aby edytować jego tor.
ui-armor-arc_marker_tooltip = Tor { $number }. Kliknij, aby edytować. Kliknij prawym przyciskiem, aby usunąć.
ui-armor-zone_toggle_tooltip = Pokazuje lub ukrywa wszystkie płyty w tej strefie. Przytrzymaj { $key }, aby pokazać tylko tę strefę.
ui-armor-mode_solo_zone = tylko strefa
ui-armor-mode_cast_trajectory = wystrzel tor
ui-armor-loading_catalog = Wczytywanie katalogu okrętów...
ui-armor-legend_collapse = Zwiń
ui-armor-legend_hide = Ukryj legendę
ui-armor-armor_thickness_title = Grubość pancerza
ui-armor-initializing = Inicjalizacja widoku 3D...
ui-armor-init_failed = Nie udało się zainicjalizować widoku pancerza: { $reason }
ui-armor-loading_named_ship = Wczytywanie { $ship }...
ui-armor-options = Opcje

## ui.armor.compass
ui-armor-compass-north = N
ui-armor-compass-east = E
ui-armor-compass-south = S
ui-armor-compass-west = W

## ui.armor.realtime
ui-armor-realtime-incoming_fire = Ogień przychodzący
ui-armor-realtime-attacker_filter = Filtr atakujących
ui-armor-realtime-all_enemies = Wszyscy wrogowie
ui-armor-realtime-show_secondaries = Pokaż uzbrojenie pomocnicze
ui-armor-realtime-seek_to_salvo = Przejdź do salwy
ui-armor-realtime-no_data = Dane o pociskach nie są już dostępne
ui-armor-realtime-sim_agrees = Symulacja potwierdza
ui-armor-realtime-sim_unchecked = { $count ->
    [one] Nie udało się zasymulować { $count } pocisku
    [few] Nie udało się zasymulować { $count } pocisków
    [many] Nie udało się zasymulować { $count } pocisków
   *[other] Nie udało się zasymulować { $count } pocisku
}
ui-armor-realtime-sim_rng_zone = Strefa losowa ({ $angle } deg w zakresie { $from }-{ $to })
ui-armor-realtime-sim_disagrees = Symulacja: { $sim } / Serwer: { $server }
ui-armor-realtime-no_armor_hit = Brak trafień w pancerz
ui-armor-realtime-tracked = Śledzone: { $salvos ->
    [one] { $salvos } salwa
    [few] { $salvos } salwy
    [many] { $salvos } salw
   *[other] { $salvos } salwy
}, { $shells ->
    [one] { $shells } pocisk
    [few] { $shells } pociski
    [many] { $shells } pocisków
   *[other] { $shells } pocisku
}
ui-armor-realtime-nothing_landed = Nic jeszcze nie trafiło w ten okręt
ui-armor-realtime-shells = { $count }x
ui-armor-realtime-salvo_shell = { $caliber } mm { $shell }
ui-armor-realtime-salvo_range = { $km } km
ui-armor-realtime-unknown_attacker = Okręt { $id }
ui-armor-realtime-server_outcome = Serwer: { $outcome }
ui-armor-realtime-not_cast = Nie można prześledzić tego pocisku przez kadłub: nie zapisano jego lotu lub pozycji okrętu
ui-armor-realtime-pick_hint = Kliknij salwę lub pocisk, aby prześledzić go przez kadłub

## ui.armor.plates
ui-armor-plates-heading = Trafione płyty ({ $count })
ui-armor-plates-arrival = { $velocity } m/s przy trafieniu, { $time } s lotu, opadanie pod kątem { $fall } deg
ui-armor-plates-overmatch = Overmatch: { $penetration } mm penetracji przy { $velocity } m/s
ui-armor-plates-penetrated = Penetruje: { $penetration } mm wobec { $effective } mm efektywnych przy { $velocity } m/s
ui-armor-plates-ricochet = Rykoszet przy { $angle } deg
ui-armor-plates-shatter = Rozbija się: { $penetration } mm wobec { $effective } mm efektywnych
ui-armor-plates-fuse_armed = Zapalnik uzbrojony
ui-armor-plates-detonation = Wybucha wewnątrz { $zone }, { $distance } m za płytą #{ $number }
ui-armor-plates-detonation_past_armor = Wybucha za całym pancerzem, { $distance } m za płytą #{ $number }
ui-armor-plates-flat_penetrates = { $ammo } penetruje: { $penetration } mm wobec { $thickness } mm
ui-armor-plates-flat_shatters = { $ammo } rozbija się: { $penetration } mm wobec { $thickness } mm
ui-armor-plates-server_angle = serwer { $angle } deg

## advantage
advantage-absolute = Absolutna
advantage-strong = Silna
advantage-moderate = Umiarkowana
advantage-weak = Słaba

## ui.search
ui-search-edit_value = Edytuj wartość
ui-search-add_filter = Dodaj filtr
ui-search-pill_select = Zaznacz
ui-search-pill_deselect = Odznacz
ui-search-pill_group_all = Grupuj z I
ui-search-pill_group_any = Grupuj z LUB
ui-search-pill_ungroup = Rozgrupuj
ui-search-pill_negate = Zaneguj
ui-search-pill_flip_connector = Przełącz I/LUB
ui-search-pill_delete = Usuń
ui-search-match_count_truncated = mecze: { $count }+
ui-search-outcome_win = Zwycięstwo
ui-search-outcome_loss = Porażka
ui-search-outcome_draw = Remis
ui-search-outcome_unknown = Nieznany
ui-search-bool_true = prawda
ui-search-bool_false = fałsz
ui-search-match_count = { $count ->
    [one] { $count } mecz
    [few] { $count } mecze
    [many] { $count } meczów
   *[other] { $count } meczu
}
ui-search-row_actions_hint = Akcje powtórki (lub kliknij wiersz prawym przyciskiem)
ui-search-looking_up_values = Wyszukiwanie wartości...
ui-search-type_a_query = Wpisz zapytanie i naciśnij Enter
ui-search-parse_failed = Nie udało się przetworzyć zapytania: { $reason }
ui-search-searching = Wyszukiwanie...
ui-search-failed = Wyszukiwanie nie powiodło się: { $reason }
ui-search-no_matches = Brak wyników
ui-search-game_mode_gap_hint = { $count ->
    [one] { $count } zindeksowany mecz nie ma zapisanego trybu gry. Użyj opcji Zindeksuj ponownie wszystkie powtórki w Ustawieniach, aby go uzupełnić.
    [few] { $count } zindeksowane mecze nie mają zapisanego trybu gry. Użyj opcji Zindeksuj ponownie wszystkie powtórki w Ustawieniach, aby je uzupełnić.
    [many] { $count } zindeksowanych meczów nie ma zapisanego trybu gry. Użyj opcji Zindeksuj ponownie wszystkie powtórki w Ustawieniach, aby je uzupełnić.
   *[other] { $count } zindeksowanego meczu nie ma zapisanego trybu gry. Użyj opcji Zindeksuj ponownie wszystkie powtórki w Ustawieniach, aby je uzupełnić.
}
ui-search-open = Otwórz
ui-search-open_missing = Plik powtórki już nie istnieje
ui-search-free_text = zawiera "{ $text }"
ui-search-quant_any = dowolny
ui-search-quant_none = żaden
ui-search-quant_ge = { $n } lub więcej
ui-search-quant_eq = dokładnie { $n }
ui-search-quant_le = co najwyżej { $n }
ui-search-quant_gt = więcej niż { $n }
ui-search-quant_lt = mniej niż { $n }
ui-search-quant_ne = nie { $n }
ui-search-scope_self = Ja:
ui-search-scope_ally = Sojusznik:
ui-search-scope_enemy = Przeciwnik:
ui-search-scope_div = Dywizjon:

## ui.search.suggest
ui-search-suggest-preset_divmate_test_ship = Ktoś z mojego dywizjonu grał okrętem testowym
ui-search-suggest-preset_no_enemy_cv = Brak lotniskowca u przeciwnika
ui-search-suggest-preset_all_enemies_died = Wszyscy przeciwnicy zginęli
ui-search-suggest-preset_high_damage_enemies = 3 lub więcej przeciwników powyżej 100k
ui-search-suggest-preset_stream_sniper = Zawiera stream snipera
ui-search-suggest-preset_i_survived = Przetrwałem
ui-search-suggest-preset_i_disconnected = Rozłączyłem się
ui-search-suggest-my_damage = Moje obrażenia
ui-search-suggest-enemy_ship = Okręt przeciwnika
ui-search-suggest-allied_ship = Okręt sojusznika
ui-search-suggest-player_in_match = Gracz był w meczu
ui-search-suggest-someone_in_division = Ktoś z mojego dywizjonu
ui-search-suggest-scope_self = Ja:
ui-search-suggest-scope_ally = Sojusznik:
ui-search-suggest-scope_enemy = Przeciwnik:
ui-search-suggest-scope_division = Dywizjon:
ui-search-suggest-scope_anyone = Dowolny gracz:

## ui.search.field
ui-search-field-outcome = Wynik
ui-search-field-map = Mapa
ui-search-field-class = Klasa
ui-search-field-tier = Poziom
ui-search-field-date = Data
ui-search-field-player_present = Gracz
ui-search-field-group = Grupa powtórek
ui-search-field-stat_damage = Obrażenia
ui-search-field-stat_kills = Zabójstwa
ui-search-field-stat_spotting = Obrażenia po wykryciu
ui-search-field-stat_potential = Potencjalne obrażenia
ui-search-field-stat_received = Otrzymane obrażenia
ui-search-field-stat_pr = PR
ui-search-field-rating = Ocena
ui-search-field-stat_survived = Przetrwał
ui-search-field-stat_disconnected = Rozłączony
ui-search-field-game_type = Typ gry
ui-search-field-game_mode = Tryb gry
ui-search-field-match_group = Grupa meczu
ui-search-field-build = Build
ui-search-field-results_available = Dostępne wyniki
ui-search-field-relation = Relacja
ui-search-field-division = Dywizjon
ui-search-field-name = Nazwa
ui-search-field-clan = Klan
ui-search-field-realm = Serwer
ui-search-field-ship = Okręt
ui-search-field-ship_index = Indeks okrętu
ui-search-field-nation = Kraj
ui-search-field-test_ship = Okręt testowy
ui-search-field-stream_sniper = Stream sniper
ui-search-field-sniper_login = Login Twitch snipera

## ui.search.op
ui-search-op-contains = zawiera
ui-search-op-equals = równa się
ui-search-op-not_equals = nie równa się
ui-search-op-eq = =
ui-search-op-ne = !=
ui-search-op-gt = >
ui-search-op-ge = >=
ui-search-op-lt = <
ui-search-op-le = <=
ui-search-op-is = jest
ui-search-op-is_not = nie jest
ui-search-op-is_set = ma wartość
ui-search-op-is_not_set = nie ma wartości

## ui.search.value
ui-search-value-relation_self = Ja
ui-search-value-relation_ally = Sojusznik
ui-search-value-relation_enemy = Przeciwnik
ui-search-value-division_mine = Mój
ui-search-value-division_any = Dowolny
ui-search-value-division_none = Brak
ui-search-value-class_air_carrier = Lotniskowiec
ui-search-value-class_battleship = Pancernik
ui-search-value-class_cruiser = Krążownik
ui-search-value-class_destroyer = Niszczyciel
ui-search-value-class_submarine = Okręt podwodny
ui-search-value-class_auxiliary = Okręt transportowy
ui-search-value-rating_bad = Zły
ui-search-value-rating_below_average = Poniżej średniej
ui-search-value-rating_average = Średni
ui-search-value-rating_good = Dobry
ui-search-value-rating_very_good = Bardzo dobry
ui-search-value-rating_great = Świetny
ui-search-value-rating_unicum = Unicum
ui-search-value-rating_super_unicum = Super Unicum
ui-search-value-game_mode_test = Testowy
ui-search-value-game_mode_standard = Standardowy
ui-search-value-game_mode_singlebase = Jedna baza
ui-search-value-game_mode_domination = Dominacja
ui-search-value-game_mode_tutorial = Samouczek
ui-search-value-game_mode_megabase = Megabaza
ui-search-value-game_mode_forts = Forty
ui-search-value-game_mode_standard_domination = Bitwa standardowa
ui-search-value-game_mode_epicenter = Epicentrum
ui-search-value-game_mode_assault_defense = Szturm i obrona
ui-search-value-game_mode_pve = PVE
ui-search-value-game_mode_arms_race = Wyścig zbrojeń
ui-search-value-game_mode_epicenter_ring = Epicentrum - pierścień
ui-search-value-game_mode_anti_standard = Anty-standardowy
ui-search-value-game_mode_attack_defense = Atak i obrona
ui-search-value-game_mode_torpedo_beat = Torpedo Beat
ui-search-value-game_mode_team_battle_royale = Drużynowy battle royale
ui-search-value-game_mode_escape_to_portal = Ucieczka do portalu
ui-search-value-game_mode_domination_asymm = Dominacja asymetryczna
ui-search-value-game_mode_key_battle = Kluczowa bitwa
ui-search-value-game_mode_portal_2021 = Portal 2021
ui-search-value-game_mode_team_battle_royale_2021 = Drużynowy battle royale 2021
ui-search-value-game_mode_convoy_event = Konwój - wydarzenie
ui-search-value-game_mode_convoy_airship = Konwój - sterowiec
ui-search-value-game_mode_two_teams_battle_royale = Battle royale dwóch drużyn
ui-search-value-game_mode_pinata_event = Wydarzenie Pinata
ui-search-value-game_mode_respawns = Odrodzenia
ui-search-value-game_mode_respawns_sectors = Odrodzenia - sektory

## ui.search.column
ui-search-column-date = Data
ui-search-column-map = Mapa
ui-search-column-mode = Tryb
ui-search-column-ship = Twój okręt
ui-search-column-result = Wynik
ui-search-column-damage = Obrażenia
ui-search-column-kills = Zabójstwa
ui-search-column-pr = PR
ui-search-column-mc = MC

## ui.settings.index
ui-settings-index-heading = Indeks powtórek
ui-settings-index-description = Przeszukiwalny rejestr wszystkich powtórek w Twoim katalogu
ui-settings-index-build = Zbuduj indeks
ui-settings-index-rebuild = Zindeksuj wszystko ponownie
ui-settings-index-rebuild_hint = Odczytaj ponownie każdą powtórkę, aby poprawić wiersze błędnie przetworzone przez starszą wersję. Zbuduj indeks odczytuje tylko brakujące.
ui-settings-index-progress = Odczytano { $done } z { $total }, nie udało się: { $failed }
ui-settings-index-progress_skipped = Odczytano { $done } z { $total }, już w indeksie: { $skipped }, nie udało się: { $failed }
ui-settings-index-built = Zindeksowano { $indexed ->
    [one] { $indexed } powtórkę
    [few] { $indexed } powtórki
    [many] { $indexed } powtórek
   *[other] { $indexed } powtórki
}, nie udało się odczytać: { $failed }
ui-settings-index-built_with_skipped = Zindeksowano { $indexed ->
    [one] { $indexed } powtórkę
    [few] { $indexed } powtórki
    [many] { $indexed } powtórek
   *[other] { $indexed } powtórki
}, { $skipped ->
    [one] { $skipped } była już w indeksie
    [few] { $skipped } były już w indeksie
    [many] { $skipped } było już w indeksie
   *[other] { $skipped } było już w indeksie
}, nie udało się odczytać: { $failed }

## ui.settings
ui-settings-loading = Wczytywanie ustawień...
ui-settings-load_failed = Nie udało się wczytać ustawień
ui-settings-not_set = (nie ustawiono)

## ui.settings.panel
ui-settings-panel-mcp = Serwer MCP
ui-settings-panel-open = Otwórz ustawienia
ui-settings-panel-search_placeholder = Szukaj w ustawieniach
ui-settings-panel-count = { $count ->
    [one] { $count } ustawienie
    [few] { $count } ustawienia
    [many] { $count } ustawień
   *[other] { $count } ustawienia
}
ui-settings-panel-no_matches = Żadne ustawienie nie pasuje do wyszukiwania
ui-settings-panel-close = Zamknij ustawienia
ui-settings-panel-in_error = { $name }: ustawienie wymaga uwagi
ui-settings-panel-general = Ogólne
ui-settings-panel-appearance = Wygląd
ui-settings-panel-game = World of Warships
ui-settings-panel-replays = Powtórki
ui-settings-panel-collaboration = Współpraca
ui-settings-panel-twitch = Twitch
ui-settings-panel-language_description = Język interfejsu
ui-settings-panel-data_sharing_description = Co jest wysyłane do ShipBuilds.com po bitwach rankingowych i losowych
ui-settings-panel-code_integrity_description = Czy kod niepodpisany przez Microsoft może być ładowany do tej aplikacji
ui-settings-panel-check_for_updates_description = Sprawdzaj nowszą wersję przy każdym uruchomieniu aplikacji
ui-settings-panel-data_folder = Folder danych aplikacji
ui-settings-panel-theme_description = Kolory interfejsu. Wskaż opcję, aby wyświetlić podgląd.
ui-settings-panel-zoom_description = Rozmiar całego tekstu i elementów sterujących
ui-settings-panel-zoom = Powiększenie
ui-settings-panel-zoom_shortcuts = { $zoom_in } powiększa, { $zoom_out } pomniejsza, { $reset } przywraca 1x
ui-settings-panel-zoom_changed = Powiększenie: { $factor }x
ui-settings-panel-cache_directory = Katalog pamięci podręcznej
ui-settings-panel-cache_directory_description = Miejsce przechowywania buildów w pamięci podręcznej
ui-settings-panel-cached_builds = Buildy w pamięci podręcznej
ui-settings-panel-current_path_description = Ostatnio otwarty plik powtórki
ui-settings-panel-show_raw_xp_description = Dodaj kolumnę surowego XP do tabeli wyników
ui-settings-panel-show_observed_damage_description = Dodaj kolumnę zaobserwowanych obrażeń do tabeli wyników
ui-settings-panel-show_entity_id_description = Dodaj kolumnę ID encji do tabeli wyników
ui-settings-panel-show_heals_description = Dodaj kolumnę leczenia do tabeli wyników
ui-settings-panel-auto_export_description = Zapisuj dane każdej bitwy do pliku w wybranym formacie i katalogu
ui-settings-panel-display_name_description = Nazwa widoczna dla innych w sesji
ui-settings-panel-get_token_description = Otwórz stronę logowania, która wydaje token czatu
ui-settings-panel-token = Token logowania
ui-settings-panel-monitored_channel_description = Obserwowany kanał. Pozostaw puste, aby obserwować własny.
ui-settings-panel-armor_defaults = Zapisane ustawienia domyślne
ui-settings-panel-armor_saved_description = Zapisane z opcji Przeglądarki modeli

## ui.settings.licenses
ui-settings-licenses-title = Informacje i licencje
ui-settings-licenses-back = Wróć do ustawień
ui-settings-licenses-third_party_intro = Ta aplikacja zawiera wymienione poniżej komponenty innych firm. Wybierz pozycję, aby wyświetlić pakiety, które z niej korzystają, oraz pełny tekst licencji.

## ui.settings.armor
ui-settings-armor-no_defaults = (brak zapisanych ustawień domyślnych)

## ui.app
ui-app-status_cache_checking = Sprawdzanie pamięci podręcznej danych gry
ui-app-status_cache_validating = Weryfikowanie pamięci podręcznej danych gry
ui-app-status_cache_downloading = Pobieranie danych gry
ui-app-status_cache_planning = Sprawdzanie, jakie dane gry są opublikowane
ui-app-drop_to_load = Upuść, aby wczytać { $file }
ui-app-drop_to_list = Upuść, aby wyświetlić powtórki z { $folder }
ui-app-drop_to_list_replays = Upuść, aby wyświetlić { $count ->
    [one] { $count } powtórkę
    [few] { $count } powtórki
    [many] { $count } powtórek
   *[other] { $count } powtórki
}
ui-app-drop_to_list_folders = Upuść, aby wyświetlić powtórki z { $count ->
    [one] { $count } folderu
    [few] { $count } folderów
    [many] { $count } folderów
   *[other] { $count } folderu
}
ui-app-drop_to_open_and_list = Upuść, aby otworzyć { $file } i wyświetlić powtórki z: { $folders }
ui-app-drop_to_list_replays_and_folders = Upuść, aby wyświetlić { $count ->
    [one] { $count } powtórkę
    [few] { $count } powtórki
    [many] { $count } powtórek
   *[other] { $count } powtórki
} oraz powtórki z { $folders }
ui-app-drop_folder_count = { $count ->
    [one] { $count } folderu
    [few] { $count } folderów
    [many] { $count } folderów
   *[other] { $count } folderu
}
ui-app-reading_replay = Odczytywanie { $replay }
ui-app-reading_replays = Odczytywanie { $count ->
    [one] { $count } powtórki
    [few] { $count } powtórek
    [many] { $count } powtórek
   *[other] { $count } powtórki
}
ui-app-status_constants_generating = Aktualizowanie stałych gry dla { $version }
ui-app-worker_constants = Proces stałych gry
ui-app-worker_live_match = Obserwacja bitwy
ui-app-worker_replay_loader = Wczytywanie powtórek
ui-app-worker_restarting = { $worker }: zatrzymano, trwa ponowne uruchamianie
ui-app-worker_stalled = { $worker }: brak odpowiedzi
ui-app-worker_stopped = { $worker }: zatrzymano
ui-app-status_indexing = Indeksowanie powtórek
ui-app-status_of = { $done } z { $total }
ui-app-status_tasks = { $count ->
    [one] { $count } uruchomione zadanie
    [few] { $count } uruchomione zadania
    [many] { $count } uruchomionych zadań
   *[other] { $count } uruchomionego zadania
}
ui-app-status_show_panel = Pokaż panel: { $panel }
ui-app-status_hide_panel = Ukryj panel: { $panel }
ui-app-panel_left = Lewy
ui-app-panel_right = Prawy
ui-app-panel_bottom = Dolny
ui-app-debug_build = Wersja debugowa

## ui.palette
ui-palette-title = Polecenia
ui-palette-placeholder = Wpisz polecenie lub wyszukaj...
ui-palette-go_to = Przejdź do: { $tab }
ui-palette-advanced_search = Wyszukiwanie zaawansowane...
ui-palette-set_theme = Motyw: { $theme }
ui-palette-games_i_died_in = Gry, w których zginąłem
ui-palette-games_i_won = Gry, które wygrałem
ui-palette-send_all_replays = Wyślij wszystkie powtórki do ShipBuilds
ui-palette-send_all_replays_again = Wyślij wszystkie powtórki do ShipBuilds (ignoruj rejestr wysłanych)
ui-palette-search_players = Szukaj graczy...
ui-palette-my_matches_in_ship = Moje mecze na okręcie...
ui-palette-open_ship_in_armor_viewer = Otwórz okręt w Przeglądarce modeli...
ui-palette-open_ship_in_shiptool = Otwórz okręt w ShipTool...
ui-palette-players_title = Szukaj graczy
ui-palette-my_ships_title = Moje mecze na okręcie
ui-palette-armor_ships_title = Otwórz okręt w Przeglądarce modeli
ui-palette-shiptool_ships_title = Otwórz okręt w ShipTool
ui-palette-players_placeholder = Szukaj graczy po nazwie...
ui-palette-my_ships_placeholder = Szukaj okrętów, którymi grałeś...
ui-palette-armor_ships_placeholder = Szukaj dowolnego okrętu po nazwie...
ui-palette-armor_ship_row = { $name } -- poziom { $tier }, { $class }, { $nation }
ui-palette-back_hint = Esc: wstecz
ui-palette-ships_loading = Wczytywanie listy okrętów...
ui-palette-ships_unavailable = Brak listy okrętów: { $reason }
ui-palette-matches_found = { $count ->
    [one] { $count } dopasowanie
    [few] { $count } dopasowania
    [many] { $count } dopasowań
   *[other] { $count } dopasowania
}
ui-palette-sample_stats = Debug: pokaż przykładowe dane w Statystykach
ui-palette-restore_stats = Debug: pokaż zapisane dane w Statystykach

## ui.replay.hover
ui-replay-hover-ship = Okręt
ui-replay-hover-map = Mapa
ui-replay-hover-mode = Tryb
ui-replay-hover-blank_mode = Brak
ui-replay-hover-played = Rozegrano
ui-replay-hover-damage = Obrażenia
ui-replay-hover-kills = Zabójstwa
ui-replay-hover-result = Wynik
ui-replay-hover-division = Dywizjon
ui-replay-hover-twitch = Czat Twitch

## ui.replay.renderer
ui-replay-renderer-export_video = Renderuj do wideo
ui-replay-renderer-export_clipboard = Renderuj do schowka
ui-replay-renderer-export_saved = Zapisano wideo w { $path }
ui-replay-renderer-export_copied = Skopiowano wideo do schowka
ui-replay-renderer-export_not_written = Nie udało się wyrenderować wideo: { $reason }
ui-replay-renderer-pop_out = Otwórz w osobnym oknie
ui-replay-renderer-export_path_unusable = Nie można zapisywać w tej lokalizacji
ui-replay-renderer-batch_started = Renderowanie { $count ->
    [one] { $count } powtórki
    [few] { $count } powtórek
    [many] { $count } powtórek
   *[other] { $count } powtórki
} do { $dir }
ui-replay-renderer-batch_finished = Wyrenderowano { $written ->
    [one] { $written } powtórkę
    [few] { $written } powtórki
    [many] { $written } powtórek
   *[other] { $written } powtórki
}; nie udało się wyrenderować: { $failed }
ui-replay-renderer-batch_all_written = Wyrenderowano { $written ->
    [one] { $written } powtórkę
    [few] { $written } powtórki
    [many] { $written } powtórek
   *[other] { $written } powtórki
}
ui-replay-renderer-batch_copied = { $written ->
    [one] Wyrenderowano { $written } powtórkę i umieszczono ją w schowku
    [few] Wyrenderowano { $written } powtórki i umieszczono je w schowku
    [many] Wyrenderowano { $written } powtórek i umieszczono je w schowku
   *[other] Wyrenderowano { $written } powtórki i umieszczono je w schowku
}
ui-replay-renderer-batch_not_copied = Wyrenderowano do { $dir }, ale schowek ich nie przyjął: { $reason }
ui-replay-renderer-batch_progress = Renderowanie { $done } z { $total }: { $replay }
ui-replay-renderer-baking = Odczytywanie powtórki i odtwarzanie przebiegu bitwy...
ui-replay-renderer-failed = Nie udało się odtworzyć tej powtórki
ui-replay-renderer-play = Odtwórz
ui-replay-renderer-pause = Pauza

## ui.renderer.annotations
ui-renderer-annotations-title = Narzędzia rysowania
ui-renderer-annotations-undo = Cofnij
ui-renderer-annotations-redo = Ponów
ui-renderer-annotations-ship_hint = Znajdź okręt
ui-renderer-annotations-no_ship = Nie wybrano okrętu
ui-renderer-annotations-nib = Grubość linii
ui-renderer-annotations-thinner = Cieńsza linia
ui-renderer-annotations-thicker = Grubsza linia
ui-renderer-annotations-shortcuts_title = Skróty klawiszowe
ui-renderer-annotations-shortcuts_tools = Narzędzia
ui-renderer-annotations-shortcuts_actions = Akcje
ui-renderer-annotations-tool_select = Zaznaczanie
ui-renderer-annotations-tool_arrow = Strzałka
ui-renderer-annotations-tool_freehand = Odręcznie
ui-renderer-annotations-tool_line = Linia
ui-renderer-annotations-tool_circle = Okrąg
ui-renderer-annotations-tool_rectangle = Prostokąt
ui-renderer-annotations-tool_triangle = Trójkąt
ui-renderer-annotations-tool_measurement = Pomiar
ui-renderer-annotations-tool_eraser = Gumka
ui-renderer-annotations-action_undo = Cofnij
ui-renderer-annotations-action_multi_select = Zaznaczanie wielokrotne
ui-renderer-annotations-action_stroke_width = Grubość linii
ui-renderer-annotations-action_delete = Usuń
ui-renderer-annotations-action_cancel = Anuluj / odznacz
ui-renderer-annotations-toggle_fill = Kliknij ponownie, aby przełączyć wypełnienie
ui-renderer-annotations-clear_all = Wyczyść wszystko
ui-renderer-annotations-edit = Edytuj rysunek
ui-renderer-annotations-size = Rozmiar
ui-renderer-annotations-color = Kolor
ui-renderer-annotations-custom_ink = Własny kolor
ui-renderer-annotations-filled = Wypełniony
ui-renderer-annotations-locked = Adnotacje zablokowane przez hosta

## ui.armor.cast
ui-armor-cast-prompt = { $key }+kliknij kadłub, aby dodać punkt trafienia
ui-armor-cast-empty = Brak punktów trafienia
ui-armor-cast-miss_nothing_under_pointer = Pod wskaźnikiem nie ma płyty pancerza
ui-armor-cast-miss_thickness_unknown = Płyty tego modelu nie mają znanej grubości, w którą można strzelić
ui-armor-cast-miss_no_plate_on_path = Tor pocisku pod tym kątem nie przecina żadnej płyty pancerza

## ui.armor.pen
ui-armor-pen-search_hint = Szukaj okrętów do porównania...
ui-armor-pen-add_a_ship = Wyszukaj i dodaj okręty powyżej, aby porównać penetrację
ui-armor-pen-hover_a_plate = Najedź na płytę okrętu, aby ją sprawdzić
ui-armor-pen-awaiting_plate = najedź na płytę
ui-armor-pen-clear_all = Wyczyść wszystko
ui-armor-pen-remove = Usuń z porównania
ui-armor-pen-firing_ship_tooltip = Użyj pocisków tego okrętu do symulacji trajektorii
ui-armor-pen-use_for_trajectory = Użyj do trajektorii
ui-armor-pen-no_shells = { $ship } nie ma pocisków artylerii głównej
ui-armor-pen-ifhe = IFHE
ui-armor-pen-ifhe_tooltip = Zapalnik bezwładnościowy dla pocisków OB zwiększa penetrację pocisków HE o jedną czwartą
ui-armor-pen-penetrates = penetruje
ui-armor-pen-shatters = nie penetruje
ui-armor-pen-angle_dependent = zależy od kąta
ui-armor-pen-no_data = brak danych o penetracji

## ui.armor.splash
ui-armor-splash-hint = Kliknij pancerz w trybie rozbryzgu, aby zobaczyć, do których stref dociera wybuch
ui-armor-splash-half_extent = Połowa rozmiaru wybuchu: { $value } jednostek modelu
ui-armor-splash-triangles = Trójkąty pancerza w wybuchu: { $total }, spenetrowane: { $penetrated }
ui-armor-splash-no_zones = W tym punkcie nie nakładają się żadne strefy rozbryzgu
ui-armor-splash-zones = Strefy w wybuchu
ui-armor-splash-direct_hit = Trafienie bezpośrednie
ui-armor-splash-hp = { $value } PW
ui-armor-splash-section = Strefy rozbryzgu

## ui.armor.export
ui-armor-export-hull = Kadłub
ui-armor-export-contents = Zawartość
ui-armor-export-contents_mesh = Siatka
ui-armor-export-contents_armor = Pancerz
ui-armor-export-contents_both = Siatka i pancerz
ui-armor-export-lod = Szczegółowość siatki
ui-armor-export-lod_tooltip = 0 to najwyższa szczegółowość. Pancerz nie ma poziomów szczegółowości i nie jest tym objęty.
ui-armor-export-res = Rozdzielczość tekstur
ui-armor-export-res_full = Pełna
ui-armor-export-res_2048 = 2048 px
ui-armor-export-res_1024 = 1024 px
ui-armor-export-res_512 = 512 px
ui-armor-export-camos = Kamuflaże
ui-armor-export-camo_select_none = Odznacz wszystkie
ui-armor-export-camo_reset = Przywróć domyślne
ui-armor-export-loading_details = Wczytywanie...
ui-armor-export-size_estimate = Szacowany rozmiar pliku: { $size }
ui-armor-export-size_breakdown = Geometria { $geometry } (dokładnie), tekstury { $textures } (szacunkowo)
ui-armor-export-size_incomplete = Nie udało się wycenić { $count ->
    [one] { $count } kamuflażu
    [few] { $count } kamuflaży
    [many] { $count } kamuflaży
   *[other] { $count } kamuflażu
}. Rozmiar może być niepełny
ui-armor-export-exporting = Eksportowanie { $ship }...
ui-armor-export-already_running = Eksport już trwa
ui-armor-export-exported = Wyeksportowano { $ship }
ui-armor-export-exported_with_size = Wyeksportowano { $ship } ({ $size })
ui-armor-export-vehicle_not_found = Nie znaleziono okrętu
ui-armor-export-export_failed = Eksport nie powiódł się: { $error }

## ui.armor.outcome
ui-armor-outcome-penetration = Penetracja
ui-armor-outcome-citadel = Cytadela
ui-armor-outcome-ricochet = Rykoszet
ui-armor-outcome-shatter = Brak penetracji
ui-armor-outcome-overpenetration = Przebicie na wylot
ui-armor-outcome-underwater = Pod wodą
ui-armor-outcome-unknown = Nierozpoznany ({ $raw })
ui-armor-outcome-stopped = Zatrzymany w pancerzu

## ui.armor.sim
ui-armor-sim-overmatch_rules_out_ricochet = Overmatch, więc bez rykoszetu
ui-armor-sim-always_ricochet_band = Rykoszet, w zakresie pewnego rykoszetu
ui-armor-sim-ricochet_angle_too_low = Rykoszet niemożliwy, zbyt mały kąt

## ui.tactics
ui-tactics-a_peers_board = Tablica udostępniona przez innego uczestnika tej sesji
ui-tactics-map = Mapa
ui-tactics-map_hint = Znajdź mapę
ui-tactics-mode = Tryb
ui-tactics-pick_a_map = Wybierz mapę, na której chcesz rysować
ui-tactics-drawing = Rysowanie mapy...
ui-tactics-reset_view = Resetuj widok
ui-tactics-add_cap = Dodaj punkt
ui-tactics-add_cap_tooltip = Umieść punkt zdobycia następnym kliknięciem na mapie
ui-tactics-selected_cap = Punkt { $letter }
ui-tactics-cap_radius = { $km } km
ui-tactics-cap_team = Drużyna
ui-tactics-cap_team_tooltip = Zmieniaj drużynę wybranego punktu zdobycia
ui-tactics-team_neutral = Neutralny
ui-tactics-team_one = Drużyna 1 (zielona)
ui-tactics-team_two = Drużyna 2 (czerwona)
ui-tactics-delete_cap = Usuń
ui-tactics-delete_cap_tooltip = Usuń wybrany punkt zdobycia
ui-tactics-cap_replay_delete_tooltip = Nie można usunąć punktu zdobycia wczytanego z powtórki
ui-tactics-cap_narrow_tooltip = Zmniejsz promień wybranego punktu zdobycia
ui-tactics-cap_widen_tooltip = Zwiększ promień wybranego punktu zdobycia
ui-tactics-undo_tooltip = Cofnij ostatnią zmianę tablicy ({ $shortcut })
ui-tactics-redo_tooltip = Ponów ostatnio cofniętą zmianę tablicy ({ $shortcut } lub { $alternate })
ui-tactics-clear_caps = Wyczyść dodane punkty
ui-tactics-clear_added_caps_tooltip = Usuń punkty zdobycia dodane przez użytkownika
ui-tactics-preset_name = Nazwa tablicy
ui-tactics-preset_save = Zapisz
ui-tactics-preset_delete = Usuń
ui-tactics-reset_board = Resetuj tablicę
ui-tactics-reset_board_tooltip = Przywróć punkty z wybranego trybu i wyczyść rysunki
ui-tactics-preset_needs_a_name = Najpierw nadaj tablicy nazwę
ui-tactics-preset_needs_a_map = Najpierw wybierz mapę dla tablicy
ui-tactics-preset_saved = Zapisano { $name }
ui-tactics-preset_save_failed = Nie udało się zapisać tablicy: { $error }
ui-tactics-preset_load_failed = Nie udało się otworzyć tablicy: { $error }
ui-tactics-preset_delete_failed = Nie udało się usunąć tablicy: { $error }
ui-tactics-presets = Zapisane tablice
ui-tactics-preset_open = Otwórz
ui-tactics-clear_drawing = Wyczyść rysunek
ui-tactics-scan_replays = Wypełnij punkty z powtórek
ui-tactics-scan_replays_tooltip = Odczytaj z powtórek układy punktów zdobycia dla trybów, w których je rozegrano
ui-tactics-scan_needs_game_data = Najpierw wczytaj build gry
ui-tactics-scan_running = Odczytywanie powtórek...
ui-tactics-scan_progress = Odczytywanie powtórek... { $read } / { $total }
ui-tactics-scan_done = Odczytano { $total ->
    [one] { $total } powtórkę
    [few] { $total } powtórki
    [many] { $total } powtórek
   *[other] { $total } powtórki
} i znaleziono { $added ->
    [one] { $added } nowy układ
    [few] { $added } nowe układy
    [many] { $added } nowych układów
   *[other] { $added } nowego układu
}
ui-tactics-session_boards_full = Sesja ma już otwarte { $limit ->
    [one] { $limit } tablicę
    [few] { $limit } tablice
    [many] { $limit } tablic
   *[other] { $limit } tablicy
}, więc ta tablica zostaje na swojej mapie
ui-tactics-placing = Umieszczanie { $ship }
ui-tactics-side_friendly = Sojusznik
ui-tactics-side_enemy = Przeciwnik
ui-tactics-ink_white = Biały tusz
ui-tactics-ink_gray = Szary tusz
ui-tactics-ink_red = Czerwony tusz
ui-tactics-ink_orange = Pomarańczowy tusz
ui-tactics-ink_yellow = Żółty tusz
ui-tactics-ink_green = Zielony tusz
ui-tactics-ink_blue = Niebieski tusz
ui-tactics-ink_purple = Fioletowy tusz
ui-tactics-ink_pink = Różowy tusz
ui-tactics-no_map_art = Ten build nie zawiera grafiki tej mapy
ui-tactics-ship_config = Konfiguracja okrętu
ui-tactics-hull = Kadłub
ui-tactics-hull_default = Domyślny
ui-tactics-modifiers = Modyfikatory
ui-tactics-concealment_expert = Ekspert od kamuflażu
ui-tactics-gun_range_mod = Mod. zasięgu dział
ui-tactics-secondary_range_mod = Mod. zasięgu artylerii pomocniczej
ui-tactics-ship_ranges = Okręgi zasięgu
ui-tactics-shape = Kształt
ui-tactics-shape_size = Rozmiar
ui-tactics-shape_size_km = Rozmiar (km)
ui-tactics-shape_filled = Wypełniony
ui-tactics-shape_smaller = Zmniejsz kształt
ui-tactics-shape_larger = Powiększ kształt

## ui.launcher
ui-launcher-rescan = Skanuj ponownie
ui-launcher-add_install = Dodaj instalację...
ui-launcher-add_install_title = Katalog instalacji World of Warships
ui-launcher-no_installs = Nie znaleziono instalacji. Dodaj istniejącą lub zainstaluj grę poniżej.
ui-launcher-managed_by_steam = zarządzana przez Steam
ui-launcher-checking_updates = sprawdzanie aktualizacji
ui-launcher-up_to_date = Aktualna
ui-launcher-update_available = Dostępna aktualizacja: { $parts }
ui-launcher-not_installed = nie zainstalowano: { $parts }
ui-launcher-check_failed = nie udało się sprawdzić aktualizacji: { $error }
ui-launcher-recheck = Sprawdź ponownie
ui-launcher-download = Pobierz / zaktualizuj...
ui-launcher-download_title = Gdzie zainstalować build
ui-launcher-download_hint = Nowa instalacja zajmuje dziesiątki GB. Aktualizacje przyrostowe nie są jeszcze obsługiwane, więc zmieniona część jest pobierana w całości.
ui-launcher-pause = Wstrzymaj
ui-launcher-cancel = Anuluj
ui-launcher-resume = Wznów
ui-launcher-paused = Pobieranie wstrzymane. Pobrane pliki zostały zachowane.
ui-launcher-installed = Zainstalowano
ui-launcher-installed_skipped = Zainstalowano (pominięto { $count ->
    [one] { $count } część przyrostową
    [few] { $count } części przyrostowe
    [many] { $count } części przyrostowych
   *[other] { $count } części przyrostowej
}: nie są jeszcze obsługiwane, pobrano je, ale nie zastosowano)
ui-launcher-download_failed = Pobieranie nie powiodło się: { $error }
ui-launcher-update_in_place = Aktualizacja trafia do folderu tej instalacji. Wybierz { $path }, aby ją zaktualizować.
ui-launcher-cancelled = Pobieranie anulowane.
ui-launcher-launch_failed = Uruchomienie nie powiodło się: { $error }
ui-launcher-use_for_toolkit = Używaj w Toolkit
ui-launcher-install_heading = Zainstaluj grę
ui-launcher-install_hint = Nowa instalacja zajmuje dziesiątki GB.
ui-launcher-install_title = Gdzie zainstalować grę
ui-launcher-install = Zainstaluj...
ui-launcher-checking_version = sprawdzanie dostępnej wersji
ui-launcher-version = wersja { $version }
ui-launcher-version_unreported = nie podano wersji klienta
ui-launcher-unavailable = niedostępne: { $error }
ui-launcher-retry = Ponów

## ui.launcher.account
ui-launcher-account-sign_in = Zaloguj
ui-launcher-account-add_title = Dodaj konto
ui-launcher-account-again_title = Zaloguj ponownie
ui-launcher-account-memory_only = Zalogowano tylko na tę sesję. Dane nie są zapisywane, więc po ponownym uruchomieniu trzeba będzie zalogować się ponownie.
ui-launcher-account-password_note = Hasło jest wysyłane wyłącznie do Wargaming i nigdy nie jest przechowywane.
ui-launcher-account-realm_fixed = Region: { $realm }
ui-launcher-account-email = E-mail
ui-launcher-account-password = Hasło
ui-launcher-account-code = Kod uwierzytelniania dwuskładnikowego
ui-launcher-account-code_required = Wymagany kod uwierzytelniania dwuskładnikowego.
ui-launcher-account-use_backup_code = Użyj kodu zapasowego
ui-launcher-account-submit_code = Wyślij kod
ui-launcher-account-captcha = To logowanie wymaga CAPTCHA, której ten launcher nie potrafi jeszcze wyświetlić. Zaloguj się raz przez oficjalne Wargaming Game Center lub wargaming.net w przeglądarce, a następnie spróbuj ponownie.

## ui.launcher.sign_in_error
ui-launcher-sign_in_error-wrong_credentials = Nieprawidłowy e-mail lub hasło.
ui-launcher-sign_in_error-two_factor_required = To konto wymaga zatwierdzenia dwuskładnikowego. Zatwierdź logowanie w Wargaming Auth lub kodem z e-maila, a następnie zaloguj się ponownie.
ui-launcher-sign_in_error-two_factor_invalid = Kod dwuskładnikowy jest nieprawidłowy lub wygasł. Wpisz nowy kod.
ui-launcher-sign_in_error-captcha_required = Wargaming wymaga CAPTCHA. Zaloguj się raz w oficjalnym Game Center lub na wargaming.net, a następnie spróbuj ponownie.
ui-launcher-sign_in_error-rate_limited = Zbyt wiele prób logowania. Odczekaj kilka minut i spróbuj ponownie.
ui-launcher-sign_in_error-account_banned = To konto jest zbanowane.
ui-launcher-sign_in_error-network_unreachable = Nie można połączyć się z Wargaming. Sprawdź połączenie z internetem.
ui-launcher-sign_in_error-service_unavailable = Usługa logowania Wargaming jest niedostępna. Spróbuj ponownie później.
ui-launcher-sign_in_error-session_expired = Zapisane logowanie wygasło. Zaloguj się ponownie.
ui-launcher-sign_in_error-keyring_locked = Systemowy pęk kluczy jest zablokowany. Odblokuj go i zaloguj się ponownie.
ui-launcher-sign_in_error-not_saved = Zalogowano, ale nie udało się zapisać logowania. Po ponownym uruchomieniu trzeba będzie zalogować się ponownie.
ui-launcher-sign_in_error-unexpected = Logowanie nie powiodło się. Szczegóły są w dzienniku.

## ui.launcher.realm
ui-launcher-realm-eu = Europa
ui-launcher-realm-na = Ameryka Północna
ui-launcher-realm-asia = Azja

## ui.launcher.runner
ui-launcher-runner-looking = Wyszukiwanie Protona.
ui-launcher-runner-steam_proton = { $name } ze Steam
ui-launcher-runner-umu = umu-run z { $name }
ui-launcher-runner-umu_default = umu-run z UMU-Proton
ui-launcher-runner-native = Gra działa natywnie.
ui-launcher-runner-first_run_prefix = Pierwsze uruchomienie tworzy prefiks Wine, co zajmuje kilka sekund.
ui-launcher-runner-first_run_download = Pierwsze uruchomienie pobiera UMU-Proton i środowisko uruchomieniowe Steam, około 700 MB.
ui-launcher-runner-choose = Wybierz folder Protona
ui-launcher-runner-automatic = Znajdź automatycznie
ui-launcher-runner-detect = Szukaj ponownie
ui-launcher-runner-needed = Do uruchomienia w systemie Linux wymagany jest Proton. Zainstaluj Steam z Protonem lub umu-launcher.

## ui.replay.damage_share
ui-replay-damage_share-mode = Obrażenia jako udział w sumie drugiego okrętu
ui-replay-damage_share-dealt = { $pct } obrażeń zadanych przez { $ship } trafiło w { $other }.
ui-replay-damage_share-dealt_inverse = { $pct } obrażeń otrzymanych przez { $other } pochodziło od { $ship }.
ui-replay-damage_share-received = { $pct } obrażeń otrzymanych przez { $ship } pochodziło od { $other }.
ui-replay-damage_share-received_inverse = { $pct } obrażeń zadanych przez { $other } trafiło w { $ship }.
ui-replay-damage_share-hold = Przytrzymaj { $key }, aby odwrócić.
ui-replay-damage_share-release = Puść { $key }, aby wrócić.

## ui.reindex
ui-reindex-context = { $count ->
    [one] Wymuś ponowne indeksowanie
    [few] Wymuś ponowne indeksowanie { $count } powtórek
    [many] Wymuś ponowne indeksowanie { $count } powtórek
   *[other] Wymuś ponowne indeksowanie { $count } powtórki
}
ui-reindex-palette_by_version = Wymuś ponowne indeksowanie powtórek: według wersji
ui-reindex-palette_all = Wymuś ponowne indeksowanie powtórek: wszystkie
ui-reindex-versions_title = Wymuś ponowne indeksowanie powtórek: według wersji
ui-reindex-versions_placeholder = Filtruj wersje gry...
ui-reindex-version_row = { $version } -- { $count ->
    [one] { $count } powtórka
    [few] { $count } powtórki
    [many] { $count } powtórek
   *[other] { $count } powtórki
}
ui-reindex-no_versions = Żadna z wymienionych powtórek nie podaje wersji gry
ui-reindex-confirm_title = Wymuś ponowne indeksowanie powtórek
ui-reindex-confirm_ok = Zindeksuj ponownie
ui-reindex-confirm_count = { $count ->
    [one] { $count } powtórka zostanie ponownie odczytana, a jej wpis w indeksie przepisany.
    [few] { $count } powtórki zostaną ponownie odczytane, a ich wpisy w indeksie przepisane.
    [many] { $count } powtórek zostanie ponownie odczytanych, a ich wpisy w indeksie przepisane.
   *[other] { $count } powtórki zostanie ponownie odczytane, a ich wpisy w indeksie przepisane.
}
ui-reindex-confirm_range = Rozegrane od { $oldest } do { $newest }.
ui-reindex-confirm_undated = { $count ->
    [one] { $count } powtórka do odczytania nie ma czytelnej daty bitwy i nie mieści się w tym zakresie.
    [few] { $count } powtórki do odczytania nie mają czytelnej daty bitwy i nie mieszczą się w tym zakresie.
    [many] { $count } powtórek do odczytania nie ma czytelnej daty bitwy i nie mieści się w tym zakresie.
   *[other] { $count } powtórki do odczytania nie ma czytelnej daty bitwy i nie mieści się w tym zakresie.
}
ui-reindex-confirm_downloads = Najpierw zostaną pobrane dane gry dla tych wersji:
ui-reindex-confirm_unobtainable = Nie można uzyskać danych gry dla tych wersji, więc { $count ->
    [one] ich { $count } powtórka zostanie pominięta:
    [few] ich { $count } powtórki zostaną pominięte:
    [many] ich { $count } powtórek zostanie pominiętych:
   *[other] ich { $count } powtórki zostanie pominięte:
}
ui-reindex-confirm_no_cache_dir = Nie ustawiono katalogu pamięci podręcznej danych gry, więc nie można pobrać danych gry dla tych wersji, a { $count ->
    [one] ich { $count } powtórka zostanie pominięta:
    [few] ich { $count } powtórki zostaną pominięte:
    [many] ich { $count } powtórek zostanie pominiętych:
   *[other] ich { $count } powtórki zostanie pominięte:
}
ui-reindex-confirm_no_build = { $count ->
    [one] { $count } powtórka nie podaje buildu gry i zostanie pominięta.
    [few] { $count } powtórki nie podają buildu gry i zostaną pominięte.
    [many] { $count } powtórek nie podaje buildu gry i zostanie pominiętych.
   *[other] { $count } powtórki nie podaje buildu gry i zostanie pominięte.
}
ui-reindex-confirm_no_version = { $count ->
    [one] { $count } powtórka nie podaje czytelnej wersji gry i zostanie pominięta.
    [few] { $count } powtórki nie podają czytelnej wersji gry i zostaną pominięte.
    [many] { $count } powtórek nie podaje czytelnej wersji gry i zostanie pominiętych.
   *[other] { $count } powtórki nie podaje czytelnej wersji gry i zostanie pominięte.
}
ui-reindex-build_row = { $version } (build { $build }): { $count ->
    [one] { $count } powtórka
    [few] { $count } powtórki
    [many] { $count } powtórek
   *[other] { $count } powtórki
}
ui-reindex-nothing_listed = Lista nie zawiera powtórek do ponownego indeksowania.
ui-reindex-nothing_readable = Żadnej z tych powtórek nie można ponownie zindeksować: brak dostępnych danych gry (pominięto: { $count }).
ui-reindex-no_game_data = Powtórek nie można odczytać, dopóki nie zostanie ustawiony katalog gry.
ui-reindex-busy_index = Indeksowanie już trwa.
ui-reindex-busy_cache = Zadanie danych gry już trwa. Spróbuj ponownie po jego zakończeniu.
ui-reindex-planning = Sprawdzanie dostępnych danych gry...
ui-reindex-no_index = Indeks powtórek nie jest otwarty, więc nic nie zostało ponownie zindeksowane.
ui-reindex-done = Ponownie zindeksowano { $indexed ->
    [one] { $indexed } powtórkę
    [few] { $indexed } powtórki
    [many] { $indexed } powtórek
   *[other] { $indexed } powtórki
}; pominięto: { $skipped }, nie udało się odczytać: { $failed }
ui-reindex-stopped = Ponowne indeksowanie zatrzymane: zindeksowano { $indexed }, pominięto { $skipped }, nie udało się odczytać { $failed }
ui-reindex-stopped_before_reading = Ponowne indeksowanie zatrzymano przed odczytaniem jakiejkolwiek powtórki.

## ui.index_folder
ui-index_folder-tooltip = Zindeksuj wszystkie powtórki z tej listy, których indeks powtórek jeszcze nie zawiera
ui-index_folder-confirm_title = Indeksuj powtórki
ui-index_folder-confirm_ok = Indeksuj
ui-index_folder-confirm_count = { $count ->
    [one] { $count } powtórka z tej listy nie jest jeszcze w indeksie powtórek i zostanie odczytana oraz do niego dodana.
    [few] { $count } powtórki z tej listy nie są jeszcze w indeksie powtórek i zostaną odczytane oraz do niego dodane.
    [many] { $count } powtórek z tej listy nie ma jeszcze w indeksie powtórek i zostaną one odczytane oraz do niego dodane.
   *[other] { $count } powtórki z tej listy nie ma jeszcze w indeksie powtórek i zostanie odczytane oraz do niego dodane.
}
ui-index_folder-note_live = Zindeksowane powtórki są dodawane do indeksu powtórek, z którego korzystają Wyszukiwanie i Śledzenie graczy.
ui-index_folder-note_folder = Zindeksowane powtórki są dodawane do indeksu powtórek pod własnym źródłem tego katalogu ({ $dir }). Wyszukiwanie i lista graczy w Śledzeniu graczy je uwzględniają. Lista klanów w Śledzeniu graczy odczytuje tylko powtórki samej gry.
ui-index_folder-nothing_listed = Ta lista nie zawiera powtórek do zindeksowania.
ui-index_folder-nothing_missing = Wszystkie powtórki z tej listy są już zindeksowane.
ui-index_folder-unreadable_index = Nie można odczytać indeksu powtórek.
ui-index_folder-done = Zindeksowano { $indexed ->
    [one] { $indexed } powtórkę
    [few] { $indexed } powtórki
    [many] { $indexed } powtórek
   *[other] { $indexed } powtórki
}; pominięto: { $skipped }, nie udało się odczytać: { $failed }
ui-index_folder-stopped = Indeksowanie zatrzymane: zindeksowano { $indexed }, pominięto { $skipped }, nie można odczytać { $failed }
ui-index_folder-stopped_before_reading = Indeksowanie zatrzymano przed odczytaniem jakiejkolwiek powtórki.

## ui.replay_folders
ui-replay_folders-install = Gra
ui-replay_folders-watched = Obserwowany: nowe i zmienione powtórki pojawiają się tutaj
ui-replay_folders-unwatched = Nieobserwowany: użyj Odśwież, aby odczytać ponownie
ui-replay_folders-unseen = { $count ->
    [one] { $count } nowa lub zmieniona powtórka od ostatniego wyświetlenia
    [few] { $count } nowe lub zmienione powtórki od ostatniego wyświetlenia
    [many] { $count } nowych lub zmienionych powtórek od ostatniego wyświetlenia
   *[other] { $count } nowej lub zmienionej powtórki od ostatniego wyświetlenia
}
ui-replay_folders-missing = Ten katalog już nie istnieje: { $dir }
ui-replay_folders-missing_hint = Będzie otwierany tutaj przy każdym uruchomieniu, dopóki go nie odepniesz.
ui-replay_folders-pin = Przypnij
ui-replay_folders-unpin = Odepnij
ui-replay_folders-refresh = Odśwież
ui-replay_folders-close = Zamknij
ui-replay_folders-pins_unreadable = Nie udało się odczytać przypiętych katalogów powtórek.
ui-replay_folders-game_data_available = Dane gry są dostępne dla wymienionych powtórek.
ui-replay_folders-browse = Przeglądaj...
ui-replay_folders-recent_title = Otwórz katalog powtórek
ui-replay_folders-recent_placeholder = Filtruj ostatnie katalogi...
ui-replay_folders-recent_missing = Ten katalog już nie istnieje i został usunięty z listy ostatnich: { $dir }
ui-replay_folders-checking = Szukanie { $dir }...
ui-replay_folders-indexing = Indeksowanie { $dir }
ui-replay_folders-indexing_in_background = Ten katalog jest nadal indeksowany
ui-replay_folders-search_while_indexing = Ten katalog jest nadal indeksowany, więc wyniki nie są jeszcze kompletne. Wyszukaj ponownie po zakończeniu.
ui-replay_folders-source_failed = Nie można zindeksować powtórek z tego katalogu: { $error }
ui-replay_folders-restart_watch = Uruchom ponownie obserwowanie
ui-replay_folders-watch_stopped = Obserwowanie { $folder } zostało zatrzymane: { $cause }.
ui-replay_folders-watch_restarting = Ponowne uruchomienie za { $seconds } s (próba { $attempt } z { $attempts }).
ui-replay_folders-watch_gave_up = Ponowne uruchomienie nie powiodło się { $attempts ->
    [one] { $attempts } raz
    [few] { $attempts } razy
    [many] { $attempts } razy
   *[other] { $attempts } raza
} i nie będzie ponawiane.
ui-replay_folders-watch_restart_hint = Kliknij czerwoną kropkę lub wybierz Uruchom ponownie obserwowanie z menu karty, aby uruchomić je teraz.
ui-replay_folders-watch_resumed = Obserwowanie { $folder } zostało wznowione.
ui-replay_folders-watch_cause_backend = system plików zgłosił błąd: { $error }
ui-replay_folders-watch_cause_gone = katalog został przeniesiony lub usunięty
ui-replay_folders-watch_cause_panicked = błąd wewnętrzny: { $error }
ui-replay_folders-watch_cause_exited = obserwowanie nieoczekiwanie się zakończyło
ui-replay_folders-dropped = Upuszczone
ui-replay_folders-dropped_numbered = Upuszczone { $number }
ui-replay_folders-dropped_count = { $count ->
    [one] { $count } upuszczona powtórka
    [few] { $count } upuszczone powtórki
    [many] { $count } upuszczonych powtórek
   *[other] { $count } upuszczonej powtórki
}
ui-replay_folders-dropped_no_source = Upuszczone powtórki są indeksowane wraz z własnymi katalogami. Otwórz katalog, aby go przeszukać lub zindeksować.
ui-replay_folders-unreadable_replay = Nie udało się odczytać: { $error }

## ui.models
ui-models-heading = Modele
ui-models-loading = Wczytywanie modeli...
ui-models-unavailable = Modele niedostępne
ui-models-search = Szukaj okrętów i modeli...
ui-models-open = Otwórz
ui-models-export = Eksportuj model
ui-models-copy_path = Kopiuj ścieżkę modelu
ui-models-path_copied = Skopiowano { $path }
ui-models-skin_replaces = Zastępuje { $model }, gdy założono { $exterior }
ui-models-geometry_absent = Brak w plikach gry
ui-models-geometry_none = Ten model nie ma geometrii
ui-models-hardpoints_tooltip = Pokaż punkty mocowania
ui-models-common = Wspólne modele

## ui.models.group
ui-models-group-gameplay = Rozgrywka
ui-models-group-building = Budynki
ui-models-group-character = Postacie
ui-models-group-location = Lokacje
ui-models-group-port = Porty
ui-models-group-style = Style
ui-models-group-other = Inne modele

## ui.models.building
ui-models-building-weapon = Uzbrojenie budynków
ui-models-building-unassigned = Nieprzypisane budynki
ui-models-building-unspecified = Budynki bez typu

## ui.models.misc
ui-models-misc-crane = Dźwigi
ui-models-misc-anchor = Kotwice
ui-models-misc-deck_fitting = Okucia pokładowe
ui-models-misc-instrument = Przyrządy
ui-models-misc-propeller = Śruby napędowe
ui-models-misc-searchlight = Reflektory
ui-models-misc-boat = Łodzie
ui-models-misc-flag = Flagi
ui-models-misc-insignia = Insygnia
ui-models-misc-railing = Relingi
ui-models-misc-hatch = Luki
ui-models-misc-ventilation = Wentylacja
ui-models-misc-antenna = Anteny
ui-models-misc-weapon = Uzbrojenie
ui-models-misc-stowage = Schowki
ui-models-misc-other = Inne

## ui.models.variant
ui-models-variant-dead = Wrak
ui-models-variant-skin = Wygląd

## ui.models.kind
ui-models-kind-ship = Okręty
ui-models-kind-gun = Działa
ui-models-kind-director = Stanowiska kierowania ogniem
ui-models-kind-finder = Dalmierze
ui-models-kind-radar = Radary
ui-models-kind-catapult = Katapulty
ui-models-kind-aircraft = Samoloty
ui-models-kind-projectile = Pociski
ui-models-kind-misc = Różne
ui-models-kind-cap = Punkty przejęcia
ui-models-kind-other = Inne

## ui.models.sub_kind
ui-models-sub_kind-battleship = Pancerniki
ui-models-sub_kind-cruiser = Krążowniki
ui-models-sub_kind-destroyer = Niszczyciele
ui-models-sub_kind-aircraft_carrier = Lotniskowce
ui-models-sub_kind-submarine = Okręty podwodne
ui-models-sub_kind-auxiliary = Okręty pomocnicze
ui-models-sub_kind-main_battery = Artyleria główna
ui-models-sub_kind-secondary_battery = Artyleria pomocnicza
ui-models-sub_kind-anti_aircraft = Przeciwlotnicze
ui-models-sub_kind-torpedo_tubes = Wyrzutnie torped
ui-models-sub_kind-depth_charge_launcher = Miotacze bomb głębinowych
ui-models-sub_kind-missile_launcher = Wyrzutnie rakiet
ui-models-sub_kind-search_radar = Radary obserwacyjne
ui-models-sub_kind-fire_control_radar = Radary kierowania ogniem
ui-models-sub_kind-fighter = Myśliwce
ui-models-sub_kind-bomber = Bombowce
ui-models-sub_kind-dive_bomber = Bombowce nurkujące
ui-models-sub_kind-scout = Samoloty zwiadowcze
ui-models-sub_kind-torpedo = Torpedy
ui-models-sub_kind-bomb = Bomby
ui-models-sub_kind-rocket = Rakiety
ui-models-sub_kind-depth_charge = Bomby głębinowe
ui-models-sub_kind-mine = Miny
ui-models-sub_kind-missile = Pociski kierowane
ui-models-sub_kind-jato = Rakiety startowe
ui-models-sub_kind-shell = Pociski artyleryjskie

## ui.models.part
ui-models-part-hull = Kadłub
ui-models-part-main_battery = Artyleria główna
ui-models-part-secondaries = Artyleria pomocnicza
ui-models-part-anti_aircraft = OPL
ui-models-part-torpedoes = Torpedy
ui-models-part-directors = Stanowiska kierowania ogniem
ui-models-part-finders = Dalmierze
ui-models-part-radars = Radary
ui-models-part-depth_charges = Bomby głębinowe
ui-models-part-aircraft = Samoloty
ui-models-part-other = Inne
ui-models-part-misc = Różne

## ui.model_view
ui-model_view-thickness_from_building = Grubość według { $building }
ui-model_view-thickness_from = Grubość według { $ship } ({ $hp })
ui-model_view-thickness_unknown = Grubość nieznana: żaden okręt nie ma tego modelu
ui-model_view-thickness_other_ships = Inne okręty
ui-model_view-export_title = Eksportuj model
ui-model_view-export_tooltip = Eksportuj model do pliku GLB
ui-model_view-part_model = Model: { $model }
ui-model_view-part_hardpoint = Punkt mocowania: { $hardpoint }

## ui.model_view.part_group
ui-model_view-part_group-hull = Kadłub
ui-model_view-part_group-main_battery = Artyleria główna
ui-model_view-part_group-secondary_battery = Artyleria pomocnicza
ui-model_view-part_group-aa_guns = OPL
ui-model_view-part_group-torpedoes = Torpedy
ui-model_view-part_group-depth_charges = Bomby głębinowe
ui-model_view-part_group-fire_control = Kierowanie ogniem
ui-model_view-part_group-radar = Radary
ui-model_view-part_group-missiles = Wyrzutnie rakiet
ui-model_view-part_group-decorations = Dekoracje
ui-model_view-part_group-misc = Różne
ui-model_view-part_group-armor = Pancerz
ui-model_view-part_group-other = Inne

## ui.scene_problem
ui-scene_problem-cannot_show = Nie można wyświetlić { $name }
ui-scene_problem-geometry_absent = Geometrii tego modelu nie ma w plikach gry.
ui-scene_problem-model_unreadable = Nie można odczytać danych tego modelu. Szczegóły są w dzienniku.
ui-scene_problem-ship_unreadable = Nie można odczytać danych tego okrętu. Szczegóły są w dzienniku.
ui-scene_problem-no_meshes = Ten model nie ma siatek.
ui-scene_problem-no_meshes_at_lod = Ten model nie ma siatek na wybranym poziomie szczegółów.
ui-scene_problem-no_visible_surface = Ten model nie ma widocznej powierzchni do narysowania.
ui-scene_problem-section_stand_in = Ta sekcja kadłuba nie ma własnej geometrii. Okręt rysuje tę część kadłuba w innej sekcji.

## ui.armor.option_tabs
ui-armor-option_tabs-label = Karty opcji
ui-armor-option_tabs-ship = Okręt
ui-armor-option_tabs-layers = Warstwy
ui-armor-option_tabs-analysis = Analiza
ui-armor-option_tabs-configuration = Konfiguracja
ui-armor-option_tabs-hull_parts = Części kadłuba
ui-armor-option_tabs-tab_matches = { $tab }, dopasowania: { $count }
ui-armor-option_tabs-empty = Nic na tej karcie nie dotyczy wyświetlanego modelu.
ui-armor-option_tabs-no_choices = Ten model nie ma kadłuba, modułu, kamuflażu ani poziomu szczegółowości do wyboru.

## ui.armor.hidden_ship
ui-armor-hidden_ship-clan_rental = Wypożyczenie klanowe
ui-armor-hidden_ship-other_class = To nie jest klasa okrętu

## ui.armor.textures
ui-armor-textures-title = Tekstury
ui-armor-textures-hint = Gra odczytuje plik z folderu res_mods danego builda zamiast spakowanego pliku o tej samej ścieżce. Kliknij teksturę prawym przyciskiem, aby skopiować tę ścieżkę.
ui-armor-textures-under_pointer = Pod kursorem: { $material }
ui-armor-textures-reading = Odczytywanie tekstur...
ui-armor-textures-summary = { $materials ->
    [one] { $materials } materiał
    [few] { $materials } materiały
    [many] { $materials } materiałów
   *[other] { $materials } materiału
}, { $textures ->
    [one] { $textures } tekstura
    [few] { $textures } tekstury
    [many] { $textures } tekstur
   *[other] { $textures } tekstury
}
ui-armor-textures-no_material = Brak materiału
ui-armor-textures-unresolved = Ścieżki nie ma w assets.bin
ui-armor-textures-absent = Brak w zainstalowanych pakietach
ui-armor-textures-unreadable = Nie udało się odczytać nagłówka DDS
ui-armor-textures-unknown_format = Nierozpoznany format
ui-armor-textures-details = { $format }, { $size }, { $count ->
    [one] { $count } plik
    [few] { $count } pliki
    [many] { $count } plików
   *[other] { $count } pliku
}, { $bytes }
ui-armor-textures-role_albedo = Albedo
ui-armor-textures-role_normal = Normalne
ui-armor-textures-role_metal_gloss = Metaliczność-połysk
ui-armor-textures-role_ambient_occlusion = Okluzja otoczenia
ui-armor-textures-role_detail = Detal
ui-armor-textures-role_name_matched = Albedo, znalezione po nazwie pliku
ui-armor-textures-copy_path = Kopiuj ścieżkę
ui-armor-textures-copy_overrides = { $count ->
    [one] Kopiuj ścieżkę res_mods
    [few] Kopiuj { $count } ścieżki res_mods
    [many] Kopiuj { $count } ścieżek res_mods
   *[other] Kopiuj { $count } ścieżki res_mods
}
ui-armor-textures-paths_copied = Skopiowano { $count ->
    [one] { $count } ścieżkę
    [few] { $count } ścieżki
    [many] { $count } ścieżek
   *[other] { $count } ścieżki
}
ui-armor-textures-no_build = Dane gry nie zostały wczytane, więc folder modów dla builda jest nieznany.
ui-armor-textures-show_in_unpacker = Pokaż w Rozpakowywaczu zasobów
ui-armor-textures-show_textures = Pokaż tekstury
ui-armor-textures-extract = Wypakuj pliki...
ui-armor-textures-extract_title = Wypakuj pliki tekstur do
ui-armor-textures-extracting = Wypakowywanie plików tekstur...
ui-armor-textures-extracted = Wypakowano { $count ->
    [one] { $count } plik
    [few] { $count } pliki
    [many] { $count } plików
   *[other] { $count } pliku
} do { $folder }
ui-armor-textures-extract_failed = Nie udało się wypakować plików tekstur: { $reason }
ui-armor-textures-replace_title = Zastąpić istniejące pliki?
ui-armor-textures-replace_description = { $count ->
    [one] { $count } z tych plików jest już w { $folder }. Wypakowanie go zastąpi.
    [few] { $count } z tych plików są już w { $folder }. Wypakowanie je zastąpi.
    [many] { $count } z tych plików jest już w { $folder }. Wypakowanie je zastąpi.
   *[other] { $count } z tych plików jest już w { $folder }. Wypakowanie je zastąpi.
}
ui-armor-textures-replace = Zastąp

## ui.settings.mcp
ui-settings-mcp-enabled = Włącz serwer MCP
ui-settings-mcp-enabled_description = Udostępnia powtórki i dane gry asystentom AI przez MCP. Pokazuje przełącznik na pasku stanu.
ui-settings-mcp-start_on_launch = Uruchamiaj z programem
ui-settings-mcp-start_on_launch_description = Uruchamia serwer przy otwarciu WoWs Toolkit.
ui-settings-mcp-bind_address = Adres nasłuchu
ui-settings-mcp-bind_address_description = Adres IP pętli zwrotnej, na którym nasłuchuje serwer, np. 127.0.0.1 lub ::1. Serwer jest dostępny tylko z tego komputera.
ui-settings-mcp-port = Port
ui-settings-mcp-port_description = Port TCP nasłuchu. Losowany przy pierwszym włączeniu i zachowywany.
ui-settings-mcp-port_unset = Wybierany przy pierwszym włączeniu
ui-settings-mcp-address_not_loopback = { $input } nie jest adresem pętli zwrotnej. Serwer nasłuchuje tylko na tym komputerze, np. na 127.0.0.1.
ui-settings-mcp-address_invalid = { $input } nie jest adresem IP
ui-settings-mcp-port_invalid = { $input } nie jest portem od 1 do 65535
ui-settings-mcp-port_in_use = Inny program używa tego portu. Wybierz inny port.

## ui.mcp
ui-mcp-started = Serwer MCP działa pod { $url }
ui-mcp-copy_url = Kopiuj URL
ui-mcp-start_failed = Nie można uruchomić serwera MCP: { $reason }
ui-mcp-port_in_use = port { $port } na { $address } jest używany przez inny program
ui-mcp-no_watchdog = nadzorca zadań w tle nie działa
ui-mcp-status_running = Serwer MCP działa pod { $url }. Kliknij, aby zatrzymać.
ui-mcp-status_starting = Uruchamianie serwera MCP na { $address }
ui-mcp-status_failed = Nie udało się uruchomić serwera MCP. Kliknij, aby ponowić.
ui-mcp-status_stopped = Serwer MCP zatrzymany. Kliknij, aby uruchomić.

## ui.launcher.opt_in
ui-launcher-opt_in-setting = Włącz launcher gry
ui-launcher-opt_in-description = Graj w World of Warships z poziomu Toolkit w systemie Linux, zalogowany na swoje konto Wargaming.
ui-launcher-opt_in-title = Włączyć launcher gry?
ui-launcher-opt_in-notice_account = Korzystając z funkcji launchera, przyjmujesz do wiadomości, że ta funkcja nie jest scenariuszem wspieranym przez twórcę gry. Wprowadzasz swoje hasło bezpośrednio do tej aplikacji, która, jeśli jej nie ufasz, mogłaby zostać użyta do kradzieży twojego konta (choć twórca aplikacji nie ma takiego zamiaru).
ui-launcher-opt_in-notice_reports = Ponadto przyjmujesz do wiadomości, że jeśli korzystasz z tej funkcji, nie będziesz zgłaszać twórcy gry błędów graficznych ani błędów stabilności, chyba że naprawdę uważasz, że jest to problem wieloplatformowy, który NIE dotyczy WYŁĄCZNIE systemu Linux.
ui-launcher-opt_in-continue = Kontynuuj
ui-launcher-opt_in-cancel = Anuluj

## ui.launcher.keyring
ui-launcher-keyring-unencrypted = Twoje logowanie jest zapisywane bez szyfrowania w folderze danych aplikacji, ponieważ nie jest dostępny żaden dostawca Secret Service (np. GNOME Keyring, KeePassXC lub KDE Wallet).
ui-launcher-keyring-how = Jak zachować szyfrowanie
ui-launcher-keyring-how_detail = Zainstaluj i uruchom dostawcę Secret Service, np. GNOME Keyring, KeePassXC lub KDE Wallet, a następnie uruchom ponownie Toolkit. Zapisane logowania zostaną do niego przeniesione.
ui-launcher-keyring-unlock_hint = Systemowy pęk kluczy może poprosić o odblokowanie, aby Toolkit mógł odczytać zapisane logowanie.

## ui.launcher.play
ui-launcher-play-play = Graj
ui-launcher-play-play_through_steam = Graj przez Steam
ui-launcher-play-set_up_game = Skonfiguruj grę
ui-launcher-play-set_up_runner = Skonfiguruj Proton
ui-launcher-play-sign_in_again = Zaloguj ponownie
ui-launcher-play-update = Aktualizuj { $size }
ui-launcher-play-updating = Aktualizowanie { $percent }%
ui-launcher-play-updating_unknown = Aktualizowanie
ui-launcher-play-paused = Wstrzymano
ui-launcher-play-starting = Uruchamianie...
ui-launcher-play-running = Uruchomiona
ui-launcher-play-in_battle = W bitwie
ui-launcher-play-menu = Konta i opcje gry

## ui.launcher.popover
ui-launcher-popover-game = World of Warships
ui-launcher-popover-no_install = Nie ustawiono instalacji gry.
ui-launcher-popover-update_available = Dostępna aktualizacja
ui-launcher-popover-check_failed = Nie udało się sprawdzić aktualizacji
ui-launcher-popover-accounts = Konta
ui-launcher-popover-no_accounts = Brak kont. Dodaj konto, aby grać po zalogowaniu.
ui-launcher-popover-close_to_switch = Zamknij grę, aby przełączyć konto.
ui-launcher-popover-default = Domyślne
ui-launcher-popover-ready = Gotowe
ui-launcher-popover-restoring = Sprawdzanie...
ui-launcher-popover-not_saved = Niezapisane
ui-launcher-popover-unencrypted = Zapisano bez szyfrowania
ui-launcher-popover-keyring_locked = Pęk kluczy zablokowany: odblokuj go, aby przywrócić zapisane logowania
ui-launcher-popover-forget = Usuń
ui-launcher-popover-sign_out = Wyloguj
ui-launcher-popover-expires_in = Wygasa za { $duration }
ui-launcher-popover-expired = Sesja wygasła
ui-launcher-popover-expiry_unknown = Czas wygaśnięcia nieznany
ui-launcher-popover-make_default = Ustaw jako domyślne
ui-launcher-popover-account_actions = Akcje konta
ui-launcher-popover-play_anyway = Graj mimo to
ui-launcher-popover-play_safe_mode = Graj w trybie bezpiecznym
ui-launcher-popover-add_account = Dodaj konto...
ui-launcher-popover-game_settings = Ustawienia gry...
ui-launcher-popover-installs = Instalacje i konfiguracja...
ui-launcher-popover-open_replay = Otwórz powtórkę w grze...

## ui.launcher.palette
ui-launcher-palette-play = Graj
ui-launcher-palette-play_as = Graj jako { $nickname } ({ $realm })
ui-launcher-palette-game_settings = Ustawienia gry
ui-launcher-palette-add_account = Dodaj konto
ui-launcher-palette-update = Aktualizuj World of Warships
ui-launcher-palette-open_replay = Otwórz powtórkę w grze

## ui.launcher.installs
ui-launcher-installs-toolkit = Katalog gry używany przez Toolkit
ui-launcher-installs-played = Używana przez konto domyślne
ui-launcher-installs-play_from = Graj z tej instalacji
ui-launcher-installs-update = Aktualizuj

## ui.launcher.wgc_login
ui-launcher-wgc_login-label = Użyj logowania Wargaming
ui-launcher-wgc_login-about = Uruchamia tę instalację ze Steam przez logowanie Toolkit zamiast przez Steam. Ustawienia i powtórki są przechowywane osobno od uruchomień przez Steam. Portfel Steam, osiągnięcia i czas gry nie działają w tym trybie.
ui-launcher-wgc_login-steam_accounts = Konta utworzone przez Steam wymagają adresu e-mail i hasła. Najpierw dodaj je w Zarządzaniu kontem Wargaming.
ui-launcher-wgc_login-steam_accounts_link = Jak dodać adres e-mail i hasło
ui-launcher-wgc_login-ready = Pliki Game Center gotowe
ui-launcher-wgc_login-fetchable = Włączenie tej opcji pobiera pliki Game Center od WG dla tego buildu ({ $size }).
ui-launcher-wgc_login-downloading = Pobieranie plików Game Center: { $percent }%
ui-launcher-wgc_login-downloading_unknown = Pobieranie plików Game Center
ui-launcher-wgc_login-checking = Wyszukiwanie plików Game Center dla tego buildu...
ui-launcher-wgc_login-needs_sign_in = Najpierw dodaj konto Wargaming do Toolkit.
ui-launcher-wgc_login-unavailable = Niedostępne dla tego buildu: { $error }

## ui.launcher.replay
ui-launcher-replay-no_install = Żadna instalacja nie ma buildu gry { $build }. Zachowaj lub zainstaluj kopię tego buildu, aby obejrzeć tę powtórkę.

## ui.launcher.setup
ui-launcher-setup-title = Konfiguracja World of Warships
ui-launcher-setup-game = Gra
ui-launcher-setup-proton = Proton
ui-launcher-setup-accounts = Konta
ui-launcher-setup-replays = Powtórki
ui-launcher-setup-game_question = Której kopii gry ma używać Toolkit?
ui-launcher-setup-none_found = Nie znaleziono instalacji na tym komputerze.
ui-launcher-setup-choose_folder = Wybierz folder...
ui-launcher-setup-install_new = Zainstaluj nową kopię...
ui-launcher-setup-proton_explain = W systemie Linux gra działa przez Protona: ze Steam, gdy Steam jest zainstalowany, w przeciwnym razie przez umu-run.
ui-launcher-setup-proton_ready = Znaleziono Protona.
ui-launcher-setup-accounts_explain = Zaloguj się na konta Wargaming, na których grasz. Przycisk Graj loguje grę na konto domyślne.
ui-launcher-setup-replays_explain = Gra usuwa najstarsze powtórki, gdy osiągnie tę liczbę.
ui-launcher-setup-skip = Pomiń konfigurację
ui-launcher-setup-back = Wstecz
ui-launcher-setup-next = Dalej
ui-launcher-setup-done = Gotowe

## ui.launcher.game_settings
ui-launcher-game_settings-title = Ustawienia gry
ui-launcher-game_settings-build = build { $build }
ui-launcher-game_settings-display = Ekran
ui-launcher-game_settings-graphics = Grafika
ui-launcher-game_settings-sound = Dźwięk
ui-launcher-game_settings-replays = Powtórki
ui-launcher-game_settings-locked = Gra jest uruchomiona. Nadpisuje te pliki przy zamknięciu, więc do tego czasu są tylko do odczytu.
ui-launcher-game_settings-footer = Zapisywane w plikach preferencji gry. Kopia zapasowa jest zachowywana.
ui-launcher-game_settings-cancel = Anuluj
ui-launcher-game_settings-save = Zapisz
ui-launcher-game_settings-saved = Zapisano ustawienia gry.
ui-launcher-game_settings-no_install = Nie ustawiono instalacji gry.
ui-launcher-game_settings-window_mode = Tryb okna
ui-launcher-game_settings-windowed = W oknie
ui-launcher-game_settings-borderless = Bez ramki
ui-launcher-game_settings-fullscreen = Pełny ekran
ui-launcher-game_settings-monitor = Monitor
ui-launcher-game_settings-monitor_name = Nazwa monitora
ui-launcher-game_settings-resolution = Rozdzielczość pełnoekranowa
ui-launcher-game_settings-width = Szerokość
ui-launcher-game_settings-height = Wysokość
ui-launcher-game_settings-refresh = Częstotliwość odświeżania
ui-launcher-game_settings-display_rate = Częstotliwość wyświetlania
ui-launcher-game_settings-vsync = Synchronizacja pionowa
ui-launcher-game_settings-ui_scale = Skala interfejsu
ui-launcher-game_settings-battle_fps = Limit klatek w bitwie
ui-launcher-game_settings-port_fps = Limit klatek w porcie
ui-launcher-game_settings-unlimited = Bez limitu
ui-launcher-game_settings-skip_intro = Pomiń film wprowadzający
ui-launcher-game_settings-master = Głośność główna
ui-launcher-game_settings-music_port = Muzyka w porcie
ui-launcher-game_settings-music_battle = Muzyka w bitwie
ui-launcher-game_settings-preset = Ustawienie wstępne
ui-launcher-game_settings-antialiasing = Wygładzanie krawędzi
ui-launcher-game_settings-textures = Jakość tekstur
ui-launcher-game_settings-filtering = Filtrowanie tekstur
ui-launcher-game_settings-shadows = Jakość cieni
ui-launcher-game_settings-graphics_note = Zmieniaj grafikę w grze. Gra sprawdza ją pod kątem twojego sprzętu.
ui-launcher-game_settings-replays_kept = Liczba zachowywanych powtórek
ui-launcher-game_settings-replays_kept_zero = Zachowaj co najmniej jedną powtórkę.
ui-launcher-game_settings-versioned = Sortuj powtórki według wersji gry
ui-launcher-game_settings-replays_note = Zapisywane w res_mods tego buildu. Tryb bezpieczny z tego nie korzysta.
ui-launcher-game_settings-absent = Nie ustawiono w plikach gry
ui-launcher-game_settings-unrecognized = Nieznana wartość: { $value }
ui-launcher-game_settings-not_a_number = { $field }: wprowadź liczbę.

## ui.settings.launcher
ui-settings-launcher-installs = Instalacje gry
ui-settings-launcher-installs_description = Instalacje gry, które launcher uruchamia i aktualizuje.
ui-settings-launcher-runner = Proton (Linux)
ui-settings-launcher-runner_description = Sposób uruchamiania gry w systemie Linux. Proton Experimental lub najnowszy Proton ze Steam jest wyszukiwany automatycznie, w przeciwnym razie używany jest umu-run.
ui-settings-launcher-launch_args = Argumenty uruchamiania
ui-settings-launcher-launch_args_description = Dodatkowe argumenty wiersza poleceń dla gry, jeden w wierszu.
ui-settings-launcher-environment = Środowisko
ui-settings-launcher-environment_description = Zmienne środowiskowe dla gry, KEY=VALUE, jedna w wierszu.
ui-settings-launcher-safe_mode_args = Argumenty trybu bezpiecznego
ui-settings-launcher-safe_mode_args_description = Argumenty dodawane przy uruchamianiu gry w trybie bezpiecznym, jeden w wierszu.
