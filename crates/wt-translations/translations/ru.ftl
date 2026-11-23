# You are encouraged to suggest edits to this file on GitHub by either filing
# an issue, editing the file directly and creating a pull request, or just
# mentioning it in the Discord.
## meta
meta-language_name = Русский

## dialog
dialog-continue_in_english = Продолжить на английском
dialog-continue_in_language = Продолжить на русском
dialog-select_language = Выберите язык
dialog-machine_translation_warning = Большинство переводов были сгенерированы машинным переводом и могут быть неточными.

## ui.tabs
ui-tabs-unpacker = Распаковщик ресурсов
ui-tabs-settings = Настройки
ui-tabs-replay_parser = Просмотр реплеев
ui-tabs-player_tracker = Отслеживание игроков
ui-tabs-armor_viewer = Просмотр брони
ui-tabs-model_viewer = Просмотр моделей
ui-tabs-stats = Статистика
ui-tabs-directory_stats = Статистика: { $dir }
ui-tabs-close_directory_stats = Закрыть статистику каталога
ui-tabs-search = Поиск
ui-tabs-search_these_replays_unavailable = Эти реплеи ещё не проиндексированы
ui-tabs-zoom_in_panel = Развернуть
ui-tabs-zoom_out_panel = Свернуть
ui-tabs-close_panel = Закрыть
ui-tabs-close_tab = Закрыть
ui-tabs-close_other_tabs = Закрыть другие
ui-tabs-close_tabs_to_the_right = Закрыть вкладки справа

## ui.buttons
ui-buttons-yes = Да
ui-buttons-no = Нет
ui-buttons-send_replays = Отправлять реплеи
ui-buttons-send_build_data = Отправлять только данные о сборках
ui-buttons-send_nothing = Ничего не отправлять
ui-buttons-switch_to_replays = Перейти на реплеи
ui-buttons-keep_current = Оставить текущую настройку
ui-buttons-stop = Стоп
ui-buttons-copy = Копировать
ui-buttons-cancel = Отмена
ui-buttons-continue_ = Продолжить
ui-buttons-install_update = Установить обновление
ui-buttons-view_release = Посмотреть релиз
ui-buttons-clear_settings = Сбросить настройки
ui-buttons-more_options = Дополнительные параметры
ui-buttons-create_issue = Создать задачу
ui-buttons-discord = Discord
ui-buttons-github = GitHub
ui-buttons-view_github = Посмотреть проект на GitHub
ui-buttons-choose = Выбрать...
ui-buttons-reset = Сброс
ui-buttons-select_all = Выбрать все
ui-buttons-download = Скачать
ui-buttons-retry = Повторить
ui-buttons-refresh_persisted_data_confirm = Обновить
ui-buttons-clear = Очистить

## ui.menu
ui-menu-file = Файл
ui-menu-check_updates = Проверить обновления
ui-menu-about = О программе
ui-menu-quit = Выход

## ui.windows
ui-windows-build_consent = Согласие на сбор данных о сборках
ui-windows-replay_migration = Переход на отправку реплеев
ui-windows-crash_detected = Обнаружен сбой приложения
ui-windows-update_available = Доступно обновление
ui-windows-network_warning = Предупреждение о сети
ui-windows-download_game_data = Загрузка данных игры
ui-windows-error = Ошибка
ui-windows-software_encode = Нет аппаратного видеокодировщика
ui-windows-about = О программе
ui-windows-refresh_persisted_data = Обновление сохранённых данных реплеев
ui-windows-tactics_board = Тактическая доска
ui-windows-minimize = Свернуть
ui-windows-maximize = Развернуть
ui-windows-restore = Восстановить
ui-windows-close = Закрыть

## ui.dialogs
ui-dialogs-build_consent_message = Хотите ли вы отправлять информацию о сборках игроков из ранговых и случайных боёв разработчику? Этот сбор данных помогает сообществу массово анализировать сборки игроков. Вы можете отказаться в любое время в настройках.
ui-dialogs-replay_migration_message = Теперь Toolkit может отправлять полные реплеи, а это даёт гораздо более подробный анализ боёв, чем одни только данные о сборках. Бои, в которых вы играете на тестовом корабле, не отправляются. Переключиться на отправку полных реплеев?
ui-dialogs-crash_message = Похоже, WoWs Toolkit аварийно завершил работу при последнем запуске. Если вы хотите сообщить об этой проблеме, пожалуйста, создайте задачу на GitHub или присоединитесь к серверу Discord и предоставьте следующую информацию.
ui-dialogs-crash_clear_settings = Если по какой-либо причине сохранённые данные приложения могут быть причиной сбоя, вы можете попробовать сбросить настройки, нажав кнопку ниже. Это очистит все настройки. Отслеживаемые игроки сохранятся. Ваши реплеи и данные WoWs останутся в безопасности.
ui-dialogs-update_message = Доступна версия { $tag } WoWs Toolkit
ui-dialogs-update_notes = Примечания к выпуску
ui-dialogs-update_asset_pending = Загрузка для Windows пока недоступна. Статус сборки смотрите на странице релиза.
ui-dialogs-update_windows_only = Доступно обновление, но в настоящее время поддерживается только Windows.
ui-dialogs-p2p_warning = Совместные сессии используют одноранговую сеть. Другие пользователи в сессии могут видеть ваш IP-адрес.
ui-dialogs-software_encode_message = Поддерживаемый аппаратный видеокодировщик не найден, поэтому экспорт будет выполнен программно. Это значительно медленнее, и долгий бой может кодироваться несколько минут.
ui-dialogs-download_game_data_intro = Некоторым реплеям нужны данные игры, которые не установлены. Выберите сборки для загрузки.
ui-dialogs-download_build_row = Версия { $version } (сборка { $build })
ui-dialogs-download_replays_needing = требуется для { $count ->
    [one] { $count } реплея
    [few] { $count } реплеев
    [many] { $count } реплеев
   *[other] { $count } реплеев
}
ui-dialogs-download_availability_exact = опубликовано
ui-dialogs-download_availability_nearest = точного совпадения нет, ближайшая версия - { $version } (сборка { $build })
ui-dialogs-download_availability_unpublished = никогда не публиковалось
ui-dialogs-download_availability_unreachable = опубликовано, но список файлов прочитать не удалось
ui-dialogs-download_plan_pending = Определяем, что нужно загрузить...
ui-dialogs-download_objects_to_fetch = Объектов к загрузке для отмеченных сборок: { $count }
ui-dialogs-download_plan_failed = Не удалось определить, что нужно загрузить
ui-dialogs-download_plan_no_cache_dir = Каталог кэша данных игры не задан. Укажите его в настройках и повторите попытку.
ui-dialogs-refresh_persisted_data_message = Повторно разобрать каждый реплей в каталоге реплеев и перезаписать его сохранённые данные. Записи, сделанные с картой данных реплеев, не соответствующей их версии игры, остаются без изменений, однако личный рейтинг и режим игры пересчитываются по текущим ожидаемым значениям для всех реплеев. Это может занять некоторое время.
ui-dialogs-file_dialog_failed = Не удалось открыть диалог выбора файла: { $reason }

## ui.labels
ui-labels-error_occurred = Произошла ошибка:
ui-labels-made_by = Автор: landaire.
ui-labels-credits = Благодарности Trackpad, TTaro, lkolbly за их вклад.
ui-labels-pr_credits = Данные и формула расчёта персонального рейтинга (PR) предоставлены WoWs Numbers.
ui-labels-suppress_warning = Больше не показывать
ui-labels-more_info = Подробнее
ui-labels-app_version = Версия { $version }
ui-labels-project_license = WoWs Toolkit распространяется по лицензии { $license }.
ui-labels-third_party_notices = Уведомления сторонних компонентов
ui-labels-notices_search_hint = Поиск по уведомлениям
ui-labels-no_matching_notices = Нет уведомлений, соответствующих запросу.

## ui.messages
ui-messages-bug_report_info_copied = Информация для отчёта об ошибке скопирована.
ui-messages-update_downloading = Загрузка обновления: { $read } из { $total }
ui-messages-update_downloading_unknown = Загрузка обновления: загружено { $read }
ui-messages-app_up_to_date = Приложение обновлено
ui-messages-checking_app_updates = Проверка обновлений приложения...
ui-messages-update_starting = Загрузка обновления...
ui-messages-update_check_failed = Не удалось проверить обновления приложения
ui-messages-game_data_loaded = Данные игры успешно загружены
ui-messages-no_replays_detected = Реплеи не обнаружены. Правильно ли указан каталог WoWs?
ui-messages-game_data_builds_downloaded = Загружены игровые данные для { $count ->
    [one] { $count } версии
    [few] { $count } версий
    [many] { $count } версий
   *[other] { $count } версий
}
ui-messages-game_data_download_failed = Не удалось загрузить данные игры
ui-messages-game_data_up_to_date = Данные игры актуальны
ui-messages-game_data_updates_available = { $count ->
    [one] Для { $count } сборки в кэше доступны новые данные
    [few] Для { $count } сборок в кэше доступны новые данные
    [many] Для { $count } сборок в кэше доступны новые данные
   *[other] Для { $count } сборок в кэше доступны новые данные
}
ui-messages-game_data_cache_valid = Кэш данных игры корректен и совпадает с удалённым репозиторием
ui-messages-game_data_cache_invalid = { $count ->
    [one] { $count } сборка в кэше устарела или повреждена
    [few] { $count } сборки в кэше устарели или повреждены
    [many] { $count } сборок в кэше устарели или повреждены
   *[other] { $count } сборки в кэше устарели или повреждены
}
ui-messages-replay_load_failed = Не удалось загрузить реплей
ui-messages-twitch_token_invalid = Токен Twitch недействителен или истёк. Пожалуйста, обновите его в настройках.
ui-messages-wows_dir_invalid = Каталог World of Warships указан неверно. Проверьте путь в настройках.
ui-messages-wows_dir_not_set = Каталог World of Warships не задан
ui-messages-constants_version_mismatch =
    Версия файла карты данных реплеев не совпадает с версией игры.
    Результаты послебоевого отчёта могут быть неточными. Пожалуйста, подождите, пока разработчики обновят карту на сервере.
ui-messages-session_started = Сессия начата
ui-messages-user_joined = { $name } присоединился
ui-messages-user_timeout = { $name } — тайм-аут
ui-messages-user_left = { $name } покинул сессию
ui-messages-session_ended = Сессия завершена
ui-messages-session_error = Ошибка сессии: { $msg }
ui-messages-connected_to_session = Подключено к сессии
ui-messages-host_opened_replay = Хост открыл реплей: { $name }
ui-messages-host_closed_replay = Хост закрыл реплей
ui-messages-session_rejected = Сессия отклонена: { $reason }
ui-messages-drop_not_a_replay = Это не файл реплея
ui-messages-drop_not_replays = Это не файлы реплеев
ui-messages-drop_skipped = Пропущен{ $count ->
    [one] { $count } перетащенный файл, не являющийся реплеем или папкой
    [few] ы { $count } перетащенных файла, не являющихся реплеями или папками
    [many] о { $count } перетащенных файлов, не являющихся реплеями или папками
   *[other] ы { $count } перетащенных файла, не являющихся реплеями или папками
}
ui-messages-log_copied = Скопировано { $bytes } КБ из { $file } в буфер обмена
ui-messages-log_copy_failed = Не удалось прочитать файл журнала: { $error }
ui-messages-log_not_found = Файл журнала в { $dir } не найден

## stat
stat-damage = Урон
stat-spotting_damage = Урон по засвету
stat-frags = Фраги
stat-raw_xp = Чистый XP
stat-base_xp = Базовый XP
stat-win_rate = Процент побед
stat-personal_rating = Персональный рейтинг
stat-match_contribution = Вклад в бой
stat-capture_points = Очки захвата
stat-planes_shot_down = Сбито самолётов
stat-anti_air_damage = Урон ПВО
stat-survival_rate = Процент выживания
stat-torpedoes_spotted = Обнаружено торпед
stat-avg_pr = Средний PR
stat-avg_prefix = Средн. { $name }

## chart
chart-combined = Общее
chart-combined_suffix = (Общее)
chart-rolling_average_suffix = (Скользящее среднее)

## confirm
confirm-open_in_game = Это запустит World of Warships. Продолжить?
confirm-clear_all_session_stats = Это очистит всю статистику сессии. Продолжить?
confirm-set_as_session_stats = Это заменит вашу текущую статистику сессии. Продолжить?

## error
error-file_watcher_creation = Не удалось создать наблюдатель файлов: { $error }
error-replay_dir_watch = Не удалось отслеживать каталог реплеев: { $error }

## ui.settings.app
ui-settings-app-check_for_updates = Проверять обновления при запуске
ui-settings-app-enable_logging = Включить логирование (вступит в силу после перезапуска)
ui-settings-app-enable_logging_tooltip = Записывать отладочные логи в файл в каталоге данных приложения. Полезно для диагностики.
ui-settings-app-data_sharing_mode = Отправка данных (ранговые и случайные бои)
ui-settings-app-theme = Тема
ui-settings-app-theme_system = Системная
ui-settings-app-theme_dark = Тёмная
ui-settings-app-theme_light = Светлая
ui-settings-app-code_integrity = Блокировать неподписанный код (вступит в силу после перезапуска)
ui-settings-app-code_integrity_tooltip = Запрещает загружать код, не подписанный Microsoft, что не даёт оверлеям, программам захвата и звуковым утилитам попасть в это приложение. Именно они часто вызывают сбои и зависания при отрисовке. Графические драйверы подписаны через Windows и продолжают работать. После начала сессии эту защиту уже нельзя отключить, поэтому изменение применяется при следующем запуске. Вариант Автоматически пропускает её, если у вас установлен сторонний метод ввода, который иначе был бы заблокирован. Учтите, что остальные защиты остаются включёнными независимо от этой настройки; чтобы отключить их все, запустите приложение с ключом --no-hardening.
ui-settings-app-code_integrity_automatic = Автоматически
ui-settings-app-code_integrity_always = Всегда
ui-settings-app-code_integrity_never = Никогда
ui-settings-app-language = Язык
ui-settings-app-open_data_dir_tooltip = Открыть папку данных приложения (настройки, логи, кэшированные данные) в файловом менеджере.
ui-settings-app-proxy_url = HTTP-прокси
ui-settings-app-proxy_url_hint_live = host:port, или оставьте пустым для использования системных настроек прокси

## ui.settings.wows
ui-settings-wows-description = Путь к установке World of Warships
ui-settings-wows-directory_hint = Каталог World of Warships
ui-settings-wows-browse = Обзор...

## ui.settings.wows.cache
ui-settings-wows-cache-heading = Кэш игровых данных
ui-settings-wows-cache-description = Выгруженные игровые данные, чтобы открывались реплеи старых версий
ui-settings-wows-cache-auto_dump = Кэшировать игровые данные для совместимости со старыми повторами
ui-settings-wows-cache-browse = Обзор
ui-settings-wows-cache-open_folder = Открыть папку
ui-settings-wows-cache-delete_old = Удалить старые версии
ui-settings-wows-cache-stats = Кэш игровых данных: { $size } в { $count ->
    [one] { $count } версии
    [few] { $count } версиях
    [many] { $count } версиях
   *[other] { $count } версиях
}
ui-settings-wows-cache-check_updates = Проверить обновления
ui-settings-wows-cache-actions = Действия с кэшем
ui-settings-wows-cache-actions_tooltip = Проверить обновления или целостность кэшированных версий
ui-settings-wows-cache-update_all = Обновить все
ui-settings-wows-cache-updates_available = { $count ->
    [one] Для { $count } сборки в кэше доступны новые данные
    [few] Для { $count } сборок в кэше доступны новые данные
    [many] Для { $count } сборок в кэше доступны новые данные
   *[other] Для { $count } сборок в кэше доступны новые данные
}
ui-settings-wows-cache-validate = Проверить кэш
ui-settings-wows-cache-repair = Восстановить
ui-settings-wows-cache-repair_needed = { $count ->
    [one] { $count } сборка в кэше устарела или повреждена
    [few] { $count } сборки в кэше устарели или повреждены
    [many] { $count } сборок в кэше устарели или повреждены
   *[other] { $count } сборки в кэше устарели или повреждены
}

## ui.settings.replay
ui-settings-replay-show_raw_xp = Показывать чистый XP
ui-settings-replay-show_entity_id = Показывать ID сущности
ui-settings-replay-show_observed_damage = Показывать наблюдаемый урон
ui-settings-replay-enable_previews = Показывать превью миникарты при наведении
ui-settings-replay-enable_previews_tooltip = При наведении на реплей он читается и проигрывается в фоне, чтобы построить зацикленное превью миникарты. Отключите на медленных накопителях.
ui-settings-replay-auto_export_data = Автоэкспорт данных
ui-settings-replay-export_path_hint = Каталог автоэкспорта данных
ui-settings-replay-export_path_missing = Каталог не существует. Экспорт данных в него невозможен.
ui-settings-replay-index_all_replays = Проиндексировать все реплеи
ui-settings-replay-show_heals = Показывать лечение
ui-settings-replay-current_path = Путь к текущему реплею

## ui.settings.session
ui-settings-session-display_name = Отображаемое имя
ui-settings-session-suppress_ip_warning = Не показывать предупреждение о P2P IP-адресе
ui-settings-session-ip_warning_tooltip = Совместные сессии используют одноранговую сеть. Другие пользователи в сессии могут видеть ваш IP-адрес.
ui-settings-session-disable_auto_open = Отключить автооткрытие окон сессии
ui-settings-session-auto_open_tooltip = При включении окна сессии (реплеи, тактические доски) не будут открываться автоматически. Вы по-прежнему сможете открыть их из всплывающего окна сессии.

## ui.settings.twitch
ui-settings-twitch-get_token = Получить токен авторизации
ui-settings-twitch-get_token_tooltip = Мы используем страницу авторизации Chatterino, так как она предоставляет токен с необходимыми разрешениями (по сути, токен модератора с правами чата), и это избавляет от необходимости разработчику WoWs Toolkit размещать собственную страницу авторизации, которая давала бы тот же результат.
ui-settings-twitch-paste_token_no_token = Вставить токен (токен не задан)
ui-settings-twitch-paste_token_tooltip = Считывает учётные данные из буфера обмена
ui-settings-twitch-paste_token_valid = Вставить токен (токен действителен)
ui-settings-twitch-paste_token_unvalidated = Проверка токена
ui-settings-twitch-paste_token_invalid = Вставить токен (токен недействителен / истёк)
ui-settings-twitch-monitored_channel = Отслеживаемый канал (по умолчанию — свой)
ui-settings-twitch-signed_in_as = Выполнен вход как { $who }

## ui.replay
ui-replay-copy_bug_report_info = Копировать информацию для отчёта об ошибке
ui-replay-match_age = { $age } назад
ui-replay-listing_caption = Реплеи
ui-replay-loading_game_data_version = Загрузка игровых данных для { $version }...
ui-replay-spectator = Наблюдатель
ui-replay-loading = Загрузка...
ui-replay-preview_loading = Построение превью...
ui-replay-preview_no_game_data = Нет данных игры для версии клиента { $value }
ui-replay-preview_unreadable_because = Не удалось прочитать этот реплей: { $value }
ui-replay-preview_unwalkable = Не удалось обработать бой из этого реплея: { $value }
ui-replay-preview_unknown_version = Реплей сообщает нечитаемую версию клиента: { $value }
ui-replay-preview_no_map = Нет данных карты для { $value }
ui-replay-preview_render_failed = Не удалось отрисовать превью: { $value }
ui-replay-incomplete_results = Неполные результаты боя
ui-replay-incomplete_results_tooltip = Реплей ещё не содержит результатов боя. Данные будут автоматически перезагружены после окончания боя и добавления результатов в реплей.
ui-replay-no_chat = В этом реплее не было отправлено сообщений в чат
ui-replay-chat_copy_all = Копировать всё
ui-replay-chat_save_to_file = Сохранить в файл
ui-replay-chat_save_title = Сохранить чат боя
ui-replay-export_results_title = Экспорт результатов
ui-replay-results_exported = Результаты экспортированы
ui-replay-chat_saved = Чат сохранён в { $path }
ui-replay-chat_save_failed = Чат не сохранён: { $error }
ui-replay-team_damage = Урон по союзникам:{" "}
ui-replay-team_share = { $pct } от суммы команды ({ $team })
ui-replay-hide_my_stats = Скрыть статистику моего тестового корабля
ui-replay-open_manually = Открыть файл реплея вручную...
ui-replay-open_directory = Открыть каталог реплеев...
ui-replay-search_these_replays = Искать в этих реплеях
ui-replay-import_constants = Импортировать файл constants.json...
ui-replay-refresh_persisted_data = Обновить сохранённые данные реплеев...
ui-replay-contributing = Отправка: { $done } из { $total ->
    [one] { $total } реплея
    [few] { $total } реплеев
    [many] { $total } реплеев
   *[other] { $total } реплеев
}
ui-replay-contributed = Отправлено { $sent } из { $total ->
    [one] { $total } реплея
    [few] { $total } реплеев
    [many] { $total } реплеев
   *[other] { $total } реплеев
}
ui-replay-contribute_nothing_shared = Сначала выберите в настройках, что отправлять
ui-replay-contribute_none_listed = Нет реплеев для отправки
ui-replay-contribute_not_ready = Реплеи можно отправить после загрузки игровых данных и настроек
ui-replay-contribute_already_running = Отправка уже выполняется
ui-replay-auto_export_failed = Не удалось записать бой: { $error }
ui-replay-auto_exported = Бой экспортирован в { $file }
ui-replay-opened_new_battle = Открыт новый бой: { $battle }
ui-replay-new_battle_listed = Новый бой: { $battle }
ui-replay-results_arrived = Получены результаты боя { $battle }
ui-replay-constants_written = { $count ->
    [one] Загружена карта результатов для { $count } версии
    [few] Загружены карты результатов для { $count } версий
    [many] Загружены карты результатов для { $count } версий
   *[other] Загружены карты результатов для { $count } версий
}
ui-replay-constants_import_title = Импорт constants.json
ui-replay-constants_imported = Карта результатов для версии { $build } импортирована
ui-replay-constants_import_failed = Карта не импортирована: { $reason }
ui-replay-constants_no_build = Сначала загрузите версию игры
ui-replay-constants_latest_written = Загружена новейшая карта результатов
ui-replay-constants_check_failed = Не удалось проверить карты результатов: { $reason }
ui-replay-constants_build_label = сборка { $build }
ui-replay-constants_generating = Обновление игровых констант для { $version }...
ui-replay-constants_generated = Игровые константы для { $version } созданы за { $seconds } с
ui-replay-constants_generation_failed = Не удалось обновить игровые константы для { $version }: { $reason }. { $fallback }
ui-replay-constants_fallback_kept = Используются ранее сохранённые для неё константы.
ui-replay-constants_fallback_published = Используются константы, опубликованные для сборки { $build }.
ui-replay-constants_fallback_embedded = Используются встроенные константы.
ui-replay-copy_latest_log = Копировать последний журнал в буфер обмена
ui-replay-load_alt_perspective = Загрузить вид другой команды...
ui-replay-load_alt_perspective_failed = Не удалось объединить реплей: { $error }
ui-replay-load_alt_perspective_limit = Этот бой уже просматривается через { $limit ->
    [one] { $limit } другую запись
    [few] { $limit } другие записи
    [many] { $limit } других записей
   *[other] { $limit } другой записи
}
ui-replay-consumable_charges_remaining = Осталось { $remaining } из { $total ->
    [one] { $total } заряда
    [few] { $total } зарядов
    [many] { $total } зарядов
   *[other] { $total } зарядов
}
ui-replay-consumable_charges_unlimited = Использовано { $used } (заряды не ограничены)
ui-replay-consumable_header_remaining = Осталось
ui-replay-consumable_header_total = Всего
ui-replay-consumable_charges_infinite = ∞
ui-replay-autoload_latest = Автозагрузка последнего реплея
ui-replay-autoload_latest_tooltip = Открывать бой сразу после того, как игра закончит его запись
ui-replay-anonymize_names = Анонимизировать имена игроков
ui-replay-anonymize_names_tooltip = Показывать сгенерированные имена вместо имён игроков и тегов кланов. Экспорт сохраняет настоящие имена.
ui-replay-group_win_rate_all_modes = Учитывать все режимы игры в проценте побед групп
ui-replay-group_win_rate_all_modes_tooltip = По умолчанию заголовки групп учитывают только случайные и ранговые бои. Если включено, учитываются все режимы игры, а процент побед не окрашивается, так как цвета откалиброваны для случайных боёв.
ui-replay-select_replay = Выберите реплей
ui-replay-drop_hint = Перетащите сюда реплей, чтобы открыть его, или папку, чтобы показать её реплеи
ui-replay-scanning = Сканирование реплеев...
ui-replay-scanning_count = Чтение реплея { $done } из { $total }
ui-replay-no_replays_in = В { $folder } нет реплеев
ui-replay-drop_folder_hint = Перетащите папку в окно, чтобы показать её в отдельной вкладке
ui-replay-new_tag = Новый
ui-replay-loading_game_data = Загрузка игровых данных...
ui-replay-game_data_failed = Не удалось загрузить игровые данные: { $error }
ui-replay-group_label = Группировка реплеев
ui-replay-column_filters = Фильтры столбцов
ui-replay-row_elements = Элементы строки
ui-replay-row_element_personal_rating = Персональный рейтинг
ui-replay-row_element_match_contribution = Вклад в бой
ui-replay-row_element_division = Дивизион
ui-replay-row_element_twitch = Чат Twitch
ui-replay-row_element_damage = Урон
ui-replay-row_element_kills = Уничтожено
ui-replay-row_element_time = Время боя
ui-replay-listing_options = Параметры списка
ui-replay-open = Открыть
ui-replay-open_file = Открыть файл реплея...
ui-replay-check_game_data = Проверить игровые данные
ui-replay-check_game_data_tooltip = Проверить наличие игровых данных, необходимых для реплеев в этом каталоге
ui-replay-close_side_panel = Закрыть панель
ui-replay-nda = NDA
ui-replay-chat = Чат
ui-replay-game_chat = Игровой чат
ui-replay-save_to_file = Сохранить в файл
ui-replay-chat_copied = Чат скопирован
ui-replay-copy_message = Копировать сообщение
ui-replay-actions = Действия
ui-replay-row_actions_hint = Действия с игроком (или щёлкните строку правой кнопкой)
ui-replay-section_match = Бой
ui-replay-section_export = Экспорт
ui-replay-section_debug = Отладка
ui-replay-timeline = Хронология боя
ui-replay-row_not_indexed = не проиндексирован
ui-replay-hover_survived = Выжил
ui-replay-hover_sunk = Потоплен
ui-replay-hover_not_indexed = Не проиндексирован
ui-replay-row_kills = { $count ->
    [one] { $count } уничтожение
    [few] { $count } уничтожения
    [many] { $count } уничтожений
   *[other] { $count } уничтожения
}
ui-replay-row_survived = жив
ui-replay-row_sunk = потоплен
ui-replay-row_division = Дивизион: { $members }
ui-replay-export_results_json = Результаты: JSON
ui-replay-export_results_cbor = Результаты: CBOR
ui-replay-export_results_csv = Результаты: CSV
ui-replay-timeline_filter_all = Все
ui-replay-timeline_filter_none = Нет
ui-replay-timeline_search_hint = Поиск корабля или игрока
ui-replay-timeline_no_events = Значимых событий не обнаружено.
ui-replay-timeline_no_matches = Нет событий, подходящих под текущий фильтр.
ui-replay-timeline_parsing = Разбор событий...
ui-replay-timeline_failed = Не удалось получить хронологию боя: { $error }
ui-replay-timeline_retry = Повторить
ui-replay-timeline_export_tooltip = Применяется к событиям, видимым после фильтрации.
ui-replay-timeline_copied = Хронология скопирована
ui-replay-timeline_event_copied = Событие скопировано
ui-replay-timeline_saved = Хронология сохранена в { $path }
ui-replay-timeline_save_failed = Не удалось сохранить хронологию: { $error }
ui-replay-timeline_kind_health_lost = Тяжёлый урон
ui-replay-timeline_kind_death = Уничтожен
ui-replay-timeline_kind_cap_contested = Точка оспаривается
ui-replay-timeline_kind_cap_flipped = Точка захвачена
ui-replay-timeline_kind_cap_being_captured = Точка захватывается
ui-replay-timeline_kind_radar = Использован радар
ui-replay-timeline_kind_advantage = Смена преимущества
ui-replay-timeline_kind_disconnected = Отключение

## ui.replay.context
ui-replay-context-open_in_new_tab = Открыть в новой вкладке
ui-replay-context-copy_cell = Копировать ячейку
ui-replay-context-copy_row = Копировать строку
ui-replay-context-copy_table = Копировать таблицу
ui-replay-context-pin_column = Закрепить столбец
ui-replay-context-unpin_column = Открепить столбец
ui-replay-context-copied = Скопировано
ui-replay-context-copy_path = Копировать путь
ui-replay-context-copy_replays = { $count ->
    [1] Копировать реплей
    [one] Копировать { $count } реплей
    [few] Копировать { $count } реплея
    [many] Копировать { $count } реплеев
   *[other] Копировать { $count } реплея
}
ui-replay-context-copy_paths = { $count ->
    [1] Копировать путь
    [one] Копировать { $count } путь
    [few] Копировать { $count } пути
    [many] Копировать { $count } путей
   *[other] Копировать { $count } пути
}
ui-replay-context-show_in_explorer = Показать в проводнике
ui-replay-context-show_replay_controls = Показать управление реплеем
ui-replay-context-open_in_game = Открыть в игре
ui-replay-context-render_replay = Отрендерить реплей
ui-replay-context-set_session_stats = Задать как статистику сессии ({ $count ->
    [one] { $count } реплей
    [few] { $count } реплея
    [many] { $count } реплеев
   *[other] { $count } реплея
})
ui-replay-context-add_session_stats = Добавить к статистике сессии ({ $count ->
    [one] { $count } реплей
    [few] { $count } реплея
    [many] { $count } реплеев
   *[other] { $count } реплея
})
ui-replay-context-render_to_video = { $count ->
    [1] Экспорт в видео
    [one] Экспорт { $count } реплея в видео
    [few] Экспорт { $count } реплеев в видео
    [many] Экспорт { $count } реплеев в видео
   *[other] Экспорт { $count } реплеев в видео
}
ui-replay-context-render_to_clipboard = { $count ->
    [1] Экспорт в буфер обмена
    [one] Экспорт { $count } реплея в буфер обмена
    [few] Экспорт { $count } реплеев в буфер обмена
    [many] Экспорт { $count } реплеев в буфер обмена
   *[other] Экспорт { $count } реплеев в буфер обмена
}

## ui.replay.column
ui-replay-column-player_name = Имя игрока
ui-replay-column-base_xp = Базовый XP
ui-replay-column-ship_name = Название корабля
ui-replay-column-hits = Попадания
ui-replay-column-heals = Ремонты
ui-replay-column-heals_tooltip = Количество замеченных активаций ремонтной команды у этого игрока. Может быть неточным для кораблей, не отрисованных на экране (то есть для кораблей противника, которые ни разу не были обнаружены).
ui-replay-column-heals_no_repair_tooltip = У этого корабля нет ремонтной команды.
ui-replay-column-kills = Убийства
ui-replay-column-observed_damage = Наблюдаемый урон
ui-replay-column-actual_damage = Фактический урон
ui-replay-column-potential_damage = Потенциальный урон
ui-replay-column-time_lived = Время жизни
ui-replay-column-fires = Пожары
ui-replay-column-floods = Затопления
ui-replay-column-citadels = Цитадели
ui-replay-column-crits = Криты
ui-replay-column-received_damage = Полученный урон
ui-replay-column-distance_traveled = Пройденное расстояние
ui-replay-column-skills = Навыки
ui-replay-column-personal_rating = PR
ui-replay-column-match_contribution = MC

## ui.rating
ui-rating-personal_rating = Персональный рейтинг
ui-rating-match_contribution = Вклад в бой
ui-rating-versus_average = отн. среднего
ui-rating-damage = Урон
ui-rating-kills = Уничтожения
ui-rating-wins = Победа
ui-rating-team_damage_share = Доля урона команды
ui-rating-tanking = Отвлечение огня
ui-rating-spotting = Засвет
ui-rating-objectives = Цели
ui-rating-anti_air = ПВО
ui-rating-torpedo_warnings = Предупреждения о торпедах
ui-rating-critical_bursts = Критические залпы
ui-rating-unrated_early_exit = Нет MC: запись закончилась раньше боя
ui-rating-mc_unrated_battle_type = Нет MC: вклад в бой учитывается только в случайных и ранговых боях

## ui.rating.detail
ui-rating-detail-versus_expected = { $actual } при ожидаемом { $expected }
ui-rating-detail-wins = Побед: { $actual } при ожидаемых { $expected }
ui-rating-detail-weighted_damage = { $actual } при ожидаемом { $expected }, урон по эсминцам и подлодкам x1.2
ui-rating-detail-team_damage_share = { $share }% урона команды по противникам, равная доля: { $even }%
ui-rating-detail-tanking = Потенциальный урон: { $potential }, { $class }x от среднего по классу, { $team }x от равной доли команды
ui-rating-detail-spotting = Урон по засвету: { $damage } (среднее по классу { $baseline }), засвечено HP кораблей: { $ships } (среднее { $ships_baseline })
ui-rating-detail-objectives = Очки: захват { $capture }, блокировка { $block }, сброс { $reset }, удержание в конце боя { $battle_end }, удержание { $hold } (учитывается на четверть)
ui-rating-detail-anti_air_planes = { $strike ->
    [one] { $strike } ударный самолёт
    [few] { $strike } ударных самолёта
    [many] { $strike } ударных самолётов
   *[other] { $strike } ударных самолёта
}, { $support ->
    [one] { $support } истребитель или разведчик
    [few] { $support } истребителя и разведчика
    [many] { $support } истребителей и разведчиков
   *[other] { $support } истребителя и разведчика
}
ui-rating-detail-anti_air_damage = Урон ПВО: { $damage }
ui-rating-detail-torpedo_warnings = { $threatening ->
    [one] Обнаружена { $threatening } опасная торпеда
    [few] Обнаружены { $threatening } опасные торпеды
    [many] Обнаружено { $threatening } опасных торпед
   *[other] Обнаружено { $threatening } опасной торпеды
}
ui-rating-detail-critical_bursts = { $count ->
    [one] { $count } критический залп
    [few] { $count } критических залпа
    [many] { $count } критических залпов
   *[other] { $count } критических залпа
}: { $bursts }. Видны только для кораблей рядом с записывающим игроком
ui-rating-detail-critical_burst = { $share }% от { $ship } за { $seconds } с
ui-rating-detail-critical_burst_at_once = { $share }% от { $ship } разом

## ui.personal_rating
ui-personal_rating-unrated_hover = Персональный рейтинг учитывается только в случайных и ранговых боях.

## ui.rating.basis
ui-rating-basis-static = Статический
ui-rating-basis-dynamic = Динамический
ui-rating-basis-static_explained = Статический: сравнивается с фиксированной константой или базовым значением класса
ui-rating-basis-dynamic_explained = Динамический: сравнивается с ожидаемыми значениями wows-numbers или с другими игроками этого боя

## ui.replay.group
ui-replay-group-date = Дата
ui-replay-group-ship = Корабль
ui-replay-group-none = Нет

## ui.replay.player
ui-replay-player-hidden_profile = У игрока скрытый профиль
ui-replay-player-never_connected = Игрок не подключился к бою

## ui.replay.build
ui-replay-build-open_shiptool = Открыть сборку в ShipTool
ui-replay-build-open_in_ship_builder = Открыть сборку в WoWs SB
ui-replay-build-copy_link = Копировать ссылку на сборку
ui-replay-build-copy_short_link = Копировать короткую ссылку на сборку
ui-replay-build-link_copied = Ссылка на сборку скопирована
ui-replay-build-open_wows_numbers = Открыть страницу WoWs Numbers
ui-replay-build-not_spotted = Этот корабль не был обнаружен. Информация о сборке недоступна.

## ui.replay.sections
ui-replay-sections-achievements = Достижения
ui-replay-sections-ribbons = Ленточки
ui-replay-sections-modules = Модули:
ui-replay-sections-modules_none = Нет модулей
ui-replay-sections-signals = Сигналы:
ui-replay-sections-loadout = Оснащение:
ui-replay-sections-loadout_none = Нет оснащения
ui-replay-sections-captain_skills = Навыки командира:
ui-replay-sections-captain_skills_none = Нет навыков командира
ui-replay-sections-consumables = Снаряжение:
ui-replay-sections-fire_chance = Фактический шанс поджога
ui-replay-sections-fire_chance_expected = ожидалось
ui-replay-sections-fire_chance_fires = { $fires ->
    [one] { $fires } пожар
    [few] { $fires } пожара
    [many] { $fires } пожаров
   *[other] { $fires } пожара
}
ui-replay-sections-fire_chance_eligible_hits = { $hits ->
    [one] { $hits } попадание, способное вызвать пожар
    [few] { $hits } попадания, способных вызвать пожар
    [many] { $hits } попаданий, способных вызвать пожар
   *[other] { $hits } попадания, способных вызвать пожар
}
ui-replay-sections-fire_chance_ships = по { $ships ->
    [one] { $ships } кораблю-цели
    [few] { $ships } кораблям-целям
    [many] { $ships } кораблям-целям
   *[other] { $ships } кораблям-целям
}
ui-replay-sections-fire_chance_expected_fires = ожидалось пожаров: { $fires }
ui-replay-sections-fire_chance_per_ship = По каждой цели
ui-replay-sections-fire_chance_no_eligible_hits = нет попаданий, способных вызвать пожар
ui-replay-sections-fire_chance_formula = Формула шанса поджога атакующего
ui-replay-sections-fire_chance_formula_base = базовый burnProb
ui-replay-sections-fire_chance_formula_clamped = ограничено
ui-replay-sections-fire_chance_breakdown = Разбор
ui-replay-sections-fire_chance_shells_fired = { $count ->
    [one] выпущенный фугасный снаряд
    [few] выпущенных фугасных снаряда
    [many] выпущенных фугасных снарядов
   *[other] выпущенных фугасных снаряда
}
ui-replay-sections-fire_chance_he_hits = { $count ->
    [one] попадание фугасом по кораблю
    [few] попадания фугасами по кораблю
    [many] попаданий фугасами по кораблю
   *[other] попадания фугасами по кораблю
}
ui-replay-sections-fire_chance_no_target_ship = { $count ->
    [one] попадание фугасом без привязки к кораблю-цели
    [few] попадания фугасами без привязки к кораблю-цели
    [many] попаданий фугасами без привязки к кораблю-цели
   *[other] попадания фугасами без привязки к кораблю-цели
}
ui-replay-sections-fire_chance_eligible = могли вызвать пожар
ui-replay-sections-fire_chance_ribbons = { $count ->
    [one] ленточка SetFire
    [few] ленточки SetFire
    [many] ленточек SetFire
   *[other] ленточки SetFire
}
ui-replay-sections-fire_chance_ribbons_credited = привязано к снаряду
ui-replay-sections-fire_chance_ribbons_uncredited = не привязано
ui-replay-sections-fire_chance_unattributed_no_burn_state = нет изменения состояния горения для сопоставления
ui-replay-sections-fire_chance_unattributed_already_credited = ближайшее попадание уже засчитано более раннему пожару
ui-replay-sections-fire_chance_unattributed_secondary_contest = поджечь мог с тем же успехом и наш вспомогательный калибр
ui-replay-sections-fire_chance_unattributed_hit_contest = каждое наше ближайшее попадание неоднозначно с другим
ui-replay-sections-fire_chance_unattributed_all_excluded = все наши ближайшие попадания уже исключены
ui-replay-sections-fire_chance_unattributed_none_could_burn = ни одно наше ближайшее попадание не могло вызвать пожар
ui-replay-sections-fire_chance_unattributed_no_hit = в этом окне не было наших попаданий
ui-replay-sections-fire_chance_not_applicable = цель уже уничтожена, неприменимо
ui-replay-sections-fire_chance_copied = Разбор шанса поджога скопирован
ui-replay-sections-fire_chance_click_to_copy = Нажмите, чтобы скопировать этот разбор
ui-replay-sections-fire_chance_exclusion_already_burning = секция уже горит
ui-replay-sections-fire_chance_exclusion_victim_build_unknown = сборка цели неизвестна, зоны пожаров могут быть объединены
ui-replay-sections-fire_chance_exclusion_damage_control_active = активна аварийная команда
ui-replay-sections-fire_chance_exclusion_damage_control_unknown = состояние аварийной команды неизвестно
ui-replay-sections-fire_chance_exclusion_observation_gap = пропуск в наблюдении
ui-replay-sections-fire_chance_exclusion_consumable_unreliable = модель активации снаряжения ненадёжна
ui-replay-sections-fire_chance_exclusion_victim_fate_unknown = судьба цели неизвестна
ui-replay-sections-fire_chance_exclusion_hit_type_does_not_roll = этот тип попадания не даёт шанса поджога
ui-replay-sections-fire_chance_exclusion_no_geometry = нет геометрии секций пожаров
ui-replay-sections-fire_chance_exclusion_impact_unplaceable = не удалось разместить попадание на сопоставленном корабле
ui-replay-sections-fire_chance_exclusion_victim_pose_unknown = положение цели в момент попадания неизвестно
ui-replay-sections-fire_chance_exclusion_ambiguous_hit = неоднозначно с другим нашим попаданием
ui-replay-sections-damage_events = События урона

## ui.replay.debug
ui-replay-debug-view_raw_metadata = Просмотреть необработанные метаданные игрока
ui-replay-debug-raw_metadata = Необработанные метаданные
ui-replay-debug-raw_results = Необработанные результаты
ui-replay-debug-mapped_results = Сопоставленные результаты
ui-replay-debug-player_metadata = Метаданные игрока
ui-replay-debug-search = Поиск
ui-replay-debug-previous_match = Предыдущее совпадение (Shift+Enter)
ui-replay-debug-next_match = Следующее совпадение (Enter)

## ui.replay.controls
ui-replay-controls-window_title = Управление реплеем
ui-replay-controls-unavailable = Управление недоступно (commands.scheme.xml не найден в файлах игры).

## ui.collab
ui-collab-session = Сессия
ui-collab-starting = Запуск сессии...
ui-collab-session_active = Сессия активна
ui-collab-connected_to_session = Подключено к сессии
ui-collab-session_token = Токен сессии:
ui-collab-toggle_visibility = Переключить видимость токена
ui-collab-copy_token = Копировать токен
ui-collab-token_copied = Токен скопирован в буфер обмена
ui-collab-copy_web_link = Копировать веб-ссылку
ui-collab-web_link_copied = Веб-ссылка скопирована в буфер обмена
ui-collab-copy_localhost_link = Копировать ссылку localhost
ui-collab-localhost_copied = Ссылка localhost скопирована в буфер обмена
ui-collab-connected_count = { $count } подключено
ui-collab-promote_cohost = Повысить до со-ведущего
ui-collab-permissions = Разрешения
ui-collab-lock_annotations = Заблокировать аннотации
ui-collab-lock_settings = Заблокировать настройки
ui-collab-reset_overrides = Сбросить переопределения клиента
ui-collab-reset_overrides_tooltip = Сбросить все изменения настроек отображения клиента
ui-collab-leave = Покинуть
ui-collab-role_host = Ведущий
ui-collab-role_cohost = Со-ведущий
ui-collab-you = (вы)
ui-collab-display_name = Отображаемое имя:
ui-collab-display_name_hint = Ваше имя...
ui-collab-enter_display_name = Сначала введите отображаемое имя
ui-collab-host_session = Создать сессию
ui-collab-start_session = Начать сессию
ui-collab-join_session = Присоединиться к сессии
ui-collab-paste_and_join = Вставить токен и присоединиться
ui-collab-clipboard_empty = Буфер обмена пуст
ui-collab-invalid_token = Недействительный токен: { $error }
ui-collab-no_runtime = Не удалось запустить сеанс: нет среды выполнения
ui-collab-shared_windows = Общие окна
ui-collab-watching = Просмотр
ui-collab-waiting_for_frames = Ожидание данных боя
ui-collab-open = Открыть
ui-collab-open_for_everyone = Открыть для всех
ui-collab-tactics_board = Тактическая доска

## battle
battle-victory = ПОБЕДА
battle-defeat = ПОРАЖЕНИЕ
battle-draw = НИЧЬЯ
battle-battle_starts_in = ДО НАЧАЛА БОЯ

## finish_type
finish_type-unknown = Бой завершён
finish_type-extermination = Все корабли противника уничтожены
finish_type-base_captured = База захвачена
finish_type-timeout = Время истекло
finish_type-failure = Задание провалено
finish_type-technical = Техническое завершение
finish_type-score = Достигнут лимит очков
finish_type-score_on_timeout = Преимущество в очках по истечении времени
finish_type-pve_success = Задание выполнено
finish_type-pve_failed = Задание провалено
finish_type-score_zero = Очки исчерпаны
finish_type-score_excess = Лимит очков превышен

## ui.stats
ui-stats-overview = Обзор
ui-stats-sample_notice = Пример данных, а не записанные бои
ui-stats-div_all = Все
ui-stats-div_solo = Соло
ui-stats-div_div = Дивизион
ui-stats-filters = Фильтры
ui-stats-own_filters = Отдельные фильтры для этого графика
ui-stats-own_filters_tooltip = Сузить этот график, не меняя содержимое других панелей
ui-stats-mode_label = Режим:
ui-stats-clear = Очистить
ui-stats-pr = PR:
ui-stats-mc = MC:
ui-stats-best_frags = Лучший: { $ship } ({ $count ->
    [one] { $count } уничтожение
    [few] { $count } уничтожения
    [many] { $count } уничтожений
   *[other] { $count } уничтожения
})
ui-stats-max_damage = Макс. урон: { $ship } ({ $damage })
ui-stats-copy_markdown = Копировать как Markdown
ui-stats-copy_image = Копировать как изображение
ui-stats-copy_image_done = График скопирован в буфер обмена
ui-stats-copy_image_failed = Не удалось скопировать график
ui-stats-copy_csv = Копировать как CSV
ui-stats-remove_games_hint = Удалить все игры на { $ship } (Ctrl+Click для пропуска подтверждения)
ui-stats-settings = Настройки
ui-stats-options = Параметры
ui-stats-ships = Корабли
ui-stats-filter_ships = Фильтр кораблей графика
ui-stats-all_ships = Все
ui-stats-no_ships = Нет
ui-stats-search_ships = Поиск кораблей...
ui-stats-no_ship_matches = Нет кораблей, соответствующих запросу
ui-stats-search_statistics = Поиск показателей
ui-stats-no_statistic_matches = Нет показателей, соответствующих запросу
ui-stats-ship_selection_count = Выбрано { $selected } из { $total }
ui-stats-all_ships_tooltip = Выбрать все подходящие корабли
ui-stats-no_ships_tooltip = Снять выбор подходящих кораблей
ui-stats-limit_to_recent = Только последние бои
ui-stats-limit_to_recent_tooltip = На вкладках «Корабли» и «Графики» ограничение действует для каждого корабля, в «Обзоре» — для всех боёв.
ui-stats-division_label = Дивизион:
ui-stats-add_chart = Добавить график
ui-stats-clear_tooltip = Забыть все записанные бои
ui-stats-clear_failed = Не очищено: { $reason }
ui-stats-clear_ship_confirm = Нажмите ещё раз, чтобы забыть бои этого корабля
ui-stats-copy_table = Копировать эту таблицу
ui-stats-copied = Скопировано в буфер обмена
ui-stats-no_games = Нет записанных боёв для текущих фильтров
ui-stats-nothing_to_plot = Нет данных для графика при текущих фильтрах
ui-stats-column_ship = Корабль
ui-stats-column_battles = Бои
ui-stats-column_win_rate = Процент побед
ui-stats-column_avg_damage = Средний урон
ui-stats-column_avg_frags = Средние фраги
ui-stats-frags = { $count ->
    [one] { $count } фраг
    [few] { $count } фрага
    [many] { $count } фрагов
   *[other] { $count } фрага
}
ui-stats-statistic = Показатель
ui-stats-open_directory_stats = Статистика каталога
ui-stats-open_directory_stats_tooltip = Открыть статистику по всем реплеям в этом каталоге и его подкаталогах
ui-stats-directory_stats_unavailable = Игровые данные ещё загружаются
ui-stats-directory_reading_task = Чтение реплеев для статистики каталога...
ui-stats-directory_walking = Поиск реплеев...
ui-stats-directory_scanning = Проверка { $done } из { $total ->
    [one] { $total } реплея...
    [few] { $total } реплеев...
    [many] { $total } реплеев...
   *[other] { $total } реплеев...
}
ui-stats-directory_reading = Чтение { $done } из { $total ->
    [one] { $total } реплея...
    [few] { $total } реплеев...
    [many] { $total } реплеев...
   *[other] { $total } реплеев...
}
ui-stats-directory_reload = Перезагрузить
ui-stats-directory_reload_tooltip = Заново прочитать реплеи этого каталога
ui-stats-directory_skipped = { $count ->
    [one] Пропущен { $count } реплей
    [few] Пропущено { $count } реплея
    [many] Пропущено { $count } реплеев
   *[other] Пропущено { $count } реплея
}
ui-stats-directory_skipped_detail =
    Нечитаемые: { $unreadable }
    Нет игровых данных для их версии: { $missing_build }
    Нет результатов боя: { $no_results }
ui-stats-chart = График
ui-stats-combine_ships = Объединить корабли
ui-stats-running_average = Скользящее среднее
ui-stats-value_labels = Подписи значений
ui-stats-reset_view = Сбросить вид
ui-stats-keep_current_layout = Сохранить текущую раскладку
ui-stats-session_title = Статистика сессии
ui-stats-session_manual = Вручную
ui-stats-session_since_opened = С запуска приложения
ui-stats-session_manual_tooltip = Сессия содержит сыгранные бои и реплеи, которые вы задали или добавили из списка реплеев. Она сохраняется между запусками.
ui-stats-session_since_opened_tooltip = Сессия содержит все бои, начавшиеся в { $time } или позже, когда было запущено приложение. Ваша ручная сессия сохраняется и вернётся при переключении на «Вручную».
ui-stats-session_battles = { $count ->
    [one] { $count } бой
    [few] { $count } боя
    [many] { $count } боёв
   *[other] { $count } боя
}
ui-stats-session_chip_manual = Сессия: { $battles }
ui-stats-session_chip_since = Сессия с { $time }: { $battles }
ui-stats-session_empty_title = В этой сессии пока нет боёв
ui-stats-session_empty_manual = Сыгранные бои добавляются в сессию по их завершении. Также можно задать или добавить реплеи через меню списка реплеев.
ui-stats-session_empty_since = Бои, начавшиеся после { $time }, добавляются в сессию по их завершении.
ui-stats-session_set_add_unavailable = Сессия содержит бои с { $time }. Переключитесь на «Вручную», чтобы задать или добавить реплеи.
ui-stats-clear_unavailable_since = Сессия с запуска приложения не сохраняется. Переключитесь на «Вручную», чтобы очистить сохранённую сессию.
ui-stats-charts_save_failed = Настройки графиков не сохранены: { $reason }
ui-stats-layout_save_failed = Расположение графиков не сохранено: { $reason }
ui-stats-axis_game = Бой
ui-stats-chart_hover_game = Бой { $number }, { $time }
ui-stats-series_average = { $stat } (среднее)
ui-stats-series_running = { $stat } (скользящее)
ui-stats-series_more = +{ $count } ещё
ui-stats-mode_random = Случайные
ui-stats-mode_ranked = Ранговые
ui-stats-mode_other = Другие
ui-stats-win_rate_rated_only = Процент побед учитывает только случайные и ранговые бои
ui-stats-win_rate_breakdown_line = { $mode }: { $rate } (боёв: { $battles })
ui-stats-matches = Бои
ui-stats-column_time = Время
ui-stats-column_mode = Режим
ui-stats-column_result = Результат
ui-stats-keep_game = Учитывать этот бой в статистике
ui-stats-leave_out_game = Исключить этот бой из статистики
ui-stats-leave_out_ship = Исключить все бои на { $ship }
ui-stats-leave_out_ship_hint = Исключить { $ship } из статистики
ui-stats-keep_all_games = Учитывать все бои
ui-stats-keep_all_games_tooltip = Снова учитывать все вручную исключённые бои
ui-stats-left_out_only = Только исключённые
ui-stats-match_count = { $shown } из { $total ->
    [one] { $total } боя
    [few] { $total } боёв
    [many] { $total } боёв
   *[other] { $total } боёв
}, исключено: { $left_out }
ui-stats-no_matches = Нет боёв, соответствующих текущим фильтрам
ui-stats-search_matches = Поиск по кораблю...
ui-stats-ship_filter_tooltip = Выберите, какие корабли учитываются в статистике
ui-stats-keep_ships_tooltip = Учитывать все подходящие корабли
ui-stats-leave_out_ships_tooltip = Исключить все подходящие корабли из статистики
ui-stats-dates = Даты
ui-stats-all_dates = Все даты
ui-stats-clear_dates = Показать все даты
ui-stats-games_left_out = { $count ->
    [one] Исключён { $count } бой
    [few] Исключено { $count } боя
    [many] Исключено { $count } боёв
   *[other] Исключено { $count } боя
}
ui-stats-reset_filters = Сбросить фильтры
ui-stats-reset_filters_tooltip = Показать все бои: без лимита, периода, отряда, режима и исключённых кораблей и боёв

## ui.stats.table
ui-stats-table-min = Мин
ui-stats-table-max = Макс
ui-stats-table-total = Всего
ui-stats-table-average = Среднее

## ui.player_tracker
ui-player_tracker-clear_stats = Очистить статистику
ui-player_tracker-populate_from_replays = Заполнить данные из реплеев
ui-player_tracker-find_matches = Найти бои
ui-player_tracker-open_wows_numbers = Открыть на wows-numbers
ui-player_tracker-open_shipbuilds = Открыть сборки на shipbuilds
ui-player_tracker-subtab_players = Игроки
ui-player_tracker-filter_hint = Фильтр по игроку или клану...
ui-player_tracker-time_period = Период
ui-player_tracker-allies = Союзники
ui-player_tracker-enemies = Противники
ui-player_tracker-live_no_directory = Укажите каталог World of Warships, чтобы отслеживать бои
ui-player_tracker-live_checking = Проверка текущего боя...
ui-player_tracker-live_none = Нет текущего боя
ui-player_tracker-live_loading_build = Загрузка данных кораблей для этой версии; названия и классы появятся после загрузки.
ui-player_tracker-waiting_for_index = Ожидание индекса реплеев
ui-player_tracker-loading_clans = Загрузка кланов...
ui-player_tracker-loading_players = Загрузка игроков...
ui-player_tracker-index_failed = Не удалось прочитать индекс: { $reason }
ui-player_tracker-no_players = Нет проиндексированных игроков за этот период
ui-player_tracker-note_not_saved = Не сохранено: { $reason }
ui-player_tracker-subtab_current_match = Текущий бой
ui-player_tracker-encounters_hover = Встречен { $total ->
    [one] { $total } раз
    [few] { $total } раза
    [many] { $total } раз
   *[other] { $total } раза
} всего, за выбранный период: { $range }. Последняя встреча: { $last }.
ui-player_tracker-never_encountered = Ранее не встречался
ui-player_tracker-copy_wg_id = Нажмите, чтобы скопировать WG ID
ui-player_tracker-copied_wg_id = WG ID { $id } скопирован
ui-player_tracker-aliases_hover = Также известен как: { $names }
ui-player_tracker-notes_hint = Заметки об этом игроке
ui-player_tracker-arena_count = { $count ->
    [one] записан { $count } бой
    [few] записано { $count } боя
    [many] записано { $count } боёв
   *[other] записано { $count } боя
}
ui-player_tracker-last_encountered_exact = Последняя встреча: { $timestamp }
ui-player_tracker-subtab_clans = Кланы
ui-player_tracker-clan_no_data = Встречи с кланами ещё не записаны.
ui-player_tracker-find_clan_matches = Найти бои, в которых встречается этот тег. В индексе нет отдельного поля клана, поэтому совпадут и имена игроков, содержащие тег.
ui-player_tracker-clan_sightings_hover = Из них { $range } приходится на выбранный период.
ui-player_tracker-clan_member_matches = встречен в { $count ->
    [one] { $count } бою
    [few] { $count } боях
    [many] { $count } боях
   *[other] { $count } боях
}
ui-player_tracker-show_division_mates = Показывать напарников по дивизиону
ui-player_tracker-show_division_mates_hover = Учитывать бои, сыгранные в дивизионе. По умолчанию отключено, так как этих игроков вы встретили там по собственному приглашению. Остальные бои против тех же игроков учитываются всегда.
ui-player_tracker-seen_in_twitch_chat = Замечен в чате Twitch
ui-player_tracker-seen_in_twitch_chat_hover = Показывать только игроков, чьё имя совпало с логином из чата Twitch во время боя с вами.
ui-player_tracker-twitch_seen_hover = Замечен в чате Twitch как { $logins } в { $count ->
    [one] { $count } бою
    [few] { $count } боях
    [many] { $count } боях
   *[other] { $count } боях
} с вами
ui-player_tracker-win_rate_overall = Общая
ui-player_tracker-win_rate_ship = Корабль
ui-player_tracker-view_compact = Компактный
ui-player_tracker-view_detailed = Подробный
ui-player_tracker-ship_stats = Статистика корабля
ui-player_tracker-stats_resolving = Чтение состава боя...
ui-player_tracker-stats_fetching = Получение статистики игроков...
ui-player_tracker-stats_unavailable = Статистика недоступна: { $reason }
ui-player_tracker-stats_hidden = У этого игрока скрытый профиль
ui-player_tracker-stats_no_data = Нет статистики по этому игроку
ui-player_tracker-pr_overall_hover = PR аккаунта по всем кораблям
ui-player_tracker-pr_ship_hover = PR на этом корабле
ui-player_tracker-win_rate_hover_line = { $label }: { $rate } за { $count ->
    [one] { $battles } бой
    [few] { $battles } боя
    [many] { $battles } боёв
   *[other] { $battles } боя
}
ui-player_tracker-win_rate_hover_line_damage = { $label }: { $rate } за { $count ->
    [one] { $battles } бой
    [few] { $battles } боя
    [many] { $battles } боёв
   *[other] { $battles } боя
}, средний урон { $damage }
ui-player_tracker-team_average_win_rate = Средн. ПБ: { $rate }
ui-player_tracker-team_players = { $count ->
    [one] { $count } игрок
    [few] { $count } игрока
    [many] { $count } игроков
   *[other] { $count } игрока
}
ui-player_tracker-debug_replay_picker = Отладка: считать реплей текущим боем
ui-player_tracker-debug_replay_picker_hover = Загружает состав завершённого боя в эту вкладку, чтобы проверить получение статистики вне боя.
ui-player_tracker-debug_replay_none = Выберите реплей

## ui.player_tracker.column
ui-player_tracker-column-win_rate = ПБ
ui-player_tracker-column-personal_rating = PR
ui-player_tracker-column-battles = Бои
ui-player_tracker-column-avg_damage = Урон
ui-player_tracker-column-encounters = Встреч
ui-player_tracker-column-clan = Клан
ui-player_tracker-column-player_name = Имя игрока
ui-player_tracker-column-total_encounters = Всего встреч
ui-player_tracker-column-encounters_in_range = Встреч за период
ui-player_tracker-column-last_encountered = Последняя встреча
ui-player_tracker-column-members = Участники
ui-player_tracker-column-matches = Бои
ui-player_tracker-column-sightings = Встречи

## ui.player_tracker.period
ui-player_tracker-period-past_hour = За последний час
ui-player_tracker-period-past_six_hours = За последние 6 часов
ui-player_tracker-period-past_day = За последние 24 часа
ui-player_tracker-period-past_week = За последнюю неделю
ui-player_tracker-period-past_month = За последний месяц
ui-player_tracker-period-all_time = За всё время

## ui.twitch
ui-twitch-possible_name = Возможное имя на стриме: { $name }
ui-twitch-seen_minutes = Замечен: через { $minutes } минут после начала боя
ui-twitch-click_to_copy = Нажмите, чтобы скопировать
ui-twitch-copied = Скопировано: { $name }

## ui.renderer
ui-renderer-preparing_export = Подготовка экспорта...
ui-renderer-encoding = Кодирование
ui-renderer-muxing = Мультиплексирование

## ui.renderer.advantage
ui-renderer-advantage-breakdown = Разбор преимущества
ui-renderer-advantage-team_eliminated = Команда уничтожена
ui-renderer-advantage-score_projection = Прогноз очков
ui-renderer-advantage-fleet_power = Мощь флота
ui-renderer-advantage-strategic_threat = Стратегическая угроза
ui-renderer-advantage-total = Итого
ui-renderer-advantage-hp_incomplete = Данные HP/кораблей неполные. Показаны только факторы очков

## ui.renderer.context
ui-renderer-context-show_trail = Показать след
ui-renderer-context-disable_other_trails = Отключить все остальные следы
ui-renderer-context-ranges = Дальности
ui-renderer-context-detection = Обнаружение
ui-renderer-context-main_battery = Главный калибр
ui-renderer-context-secondary = Вспомогательный калибр
ui-renderer-context-torpedo = Торпеды
ui-renderer-context-radar = Радар
ui-renderer-context-hydro = Гидроакустика
ui-renderer-context-enable_all = Включить все
ui-renderer-context-disable_all = Отключить все
ui-renderer-context-disable_other_ranges = Отключить все остальные дальности
ui-renderer-context-enable_all_ranges = Включить дальности всех кораблей
ui-renderer-context-realtime_armor = Показывать броню в реальном времени

## ui.renderer.controls
ui-renderer-controls-jump_to_start = Перейти к началу
ui-renderer-controls-previous_event = Предыдущее событие (Shift+Left)
ui-renderer-controls-back_10s = Назад 10 сек (Left)
ui-renderer-controls-forward_10s = Вперёд 10 сек (Right)
ui-renderer-controls-next_event = Следующее событие (Shift+Right)
ui-renderer-controls-jump_to_end = Перейти к концу
ui-renderer-controls-speed = Скорость воспроизведения

## ui.renderer.settings
ui-renderer-settings-title = Настройки отображения
ui-renderer-settings-hp_bars = Полосы HP
ui-renderer-settings-armament = Вооружение
ui-renderer-settings-dead_ships = Уничтоженные корабли
ui-renderer-settings-dead_ship_names = Названия уничтоженных кораблей
ui-renderer-settings-player_names = Имена игроков
ui-renderer-settings-ship_ranges = Дальности кораблей
ui-renderer-settings-ship_names = Названия кораблей
ui-renderer-settings-camera_direction = Направление башен
ui-renderer-settings-heat_trail = Тепловой след
ui-renderer-settings-speed_trails = Следы скорости
ui-renderer-settings-dead_ship_trails = Следы уничтоженных кораблей
ui-renderer-settings-buildings = Здания
ui-renderer-settings-capture_points = Точки захвата
ui-renderer-settings-consumables = Снаряжение
ui-renderer-settings-planes = Самолёты
ui-renderer-settings-smoke = Дымовая завеса
ui-renderer-settings-torpedoes = Торпеды
ui-renderer-settings-tracers = Трассеры
ui-renderer-settings-battle_result = Результат боя
ui-renderer-settings-buff_counters = Счётчики усилений
ui-renderer-settings-chat_label = Чат
ui-renderer-settings-kill_feed = Лента убийств
ui-renderer-settings-score_label = Счёт
ui-renderer-settings-score_timers = Таймеры очков
ui-renderer-settings-team_advantage = Преимущество команды
ui-renderer-settings-team_rosters = Составы команд
ui-renderer-settings-stats_panel = Панель статистики
ui-renderer-settings-timer = Таймер
ui-renderer-settings-export_settings = Настройки экспорта
ui-renderer-settings-prefer_cpu = Предпочитать CPU-кодировщик
ui-renderer-settings-prefer_cpu_tooltip = Использовать программный (CPU) кодировщик вместо GPU для экспорта видео
ui-renderer-settings-include_pre_battle = Включать предбоевую фазу
ui-renderer-settings-include_pre_battle_tooltip = Включать появление кораблей и обратный отсчёт до начала боя в экспортируемое видео
ui-renderer-settings-export_timing = Тайминг видео
ui-renderer-settings-export_timing_length = Подогнать под длительность
ui-renderer-settings-export_timing_speed = Скорость
ui-renderer-settings-export_timing_result_speed = Воспроизводится примерно на { $speed }x
ui-renderer-settings-export_timing_result_length = Длительность видео: { $length }
ui-renderer-settings-codec = Кодек
ui-renderer-settings-codec_auto = Авто
ui-renderer-settings-save_defaults = Сохранить по умолчанию
ui-renderer-settings-save_defaults_done = Сохранено как настройки по умолчанию для рендерера
ui-renderer-settings-save_defaults_failed = Не удалось сохранить настройки по умолчанию
ui-renderer-settings-event_timeline = Шкала событий

## ui.unpacker
ui-unpacker-assets_bin = Assets.bin
ui-unpacker-loading_assets = Загрузка assets.bin...
ui-unpacker-load_failed = Не удалось загрузить: { $error }
ui-unpacker-no_game_data = Данные игры не загружены. Укажите каталог World of Warships в настройках.
ui-unpacker-filter_files = Фильтр файлов...
ui-unpacker-search_in_files = Поиск в файлах
ui-unpacker-search_hint = Поиск в файлах...
ui-unpacker-path_filter_hint = Фильтр пути (напр. *.xml)
ui-unpacker-queue_all_files = Добавить все файлы в очередь
ui-unpacker-view_as_json = Просмотреть как JSON
ui-unpacker-extract_as_json = Извлечь как JSON
ui-unpacker-view_contents = Просмотреть содержимое
ui-unpacker-version = Версия:
ui-unpacker-decode_prototypes = Декодировать прототипы как JSON
ui-unpacker-decode_prototypes_tooltip = Декодировать поддерживаемые прототипы Assets.bin (.visual, .model, .mfm) в JSON при извлечении
ui-unpacker-extraction_queue = Очередь извлечения
ui-unpacker-collapse_folders = Свернуть дерево папок
ui-unpacker-expand_folders = Развернуть дерево папок
ui-unpacker-clear_all = Очистить всё
ui-unpacker-browse = Обзор
ui-unpacker-output_dir_hint = Каталог вывода...
ui-unpacker-remove_from_queue = Удалить из очереди
ui-unpacker-queue_folder = Добавить всю папку в очередь
ui-unpacker-queue_file = Добавить файл в очередь
ui-unpacker-extraction_output = Извлечь в
ui-unpacker-queue_empty = Отметьте файл или папку в списке, чтобы добавить в очередь
ui-unpacker-packages = Пакеты
ui-unpacker-item_count = { $count ->
    [one] { $count } элемент
    [few] { $count } элемента
    [many] { $count } элементов
   *[other] { $count } элемента
}
ui-unpacker-search_title = Поиск: { $query }
ui-unpacker-search_in = в { $source }
ui-unpacker-image_decode_failed = Не удалось декодировать изображение: { $reason }
ui-unpacker-find = Найти
ui-unpacker-find_previous = Предыдущее совпадение
ui-unpacker-find_next = Следующее совпадение
ui-unpacker-find_close = Закрыть поиск
ui-unpacker-find_count = { $current } из { $total }
ui-unpacker-find_none = Нет совпадений
ui-unpacker-find_match_case = Учитывать регистр
ui-unpacker-queue = Очередь
ui-unpacker-reveal_in_tree = Показать в дереве пакетов
ui-unpacker-hit_offset = по смещению { $offset }
ui-unpacker-queued_count = В очереди: { $count }
ui-unpacker-counting_files = Подсчёт файлов...
ui-unpacker-extracting = Извлечение { $done } из { $total }
ui-unpacker-extract_failed = Ошибка извлечения: { $reason }
ui-unpacker-extracted = Извлечен{ $count ->
    [one] { $count } файл
    [few] о { $count } файла
    [many] о { $count } файлов
   *[other] о { $count } файла
}
ui-unpacker-cancelled = Отменено после { $count ->
    [one] { $count } файла
    [few] { $count } файлов
    [many] { $count } файлов
   *[other] { $count } файлов
}
ui-unpacker-extract = { $count ->
    [0] Извлечь
    [one] Извлечь { $count } элемент
    [few] Извлечь { $count } элемента
    [many] Извлечь { $count } элементов
   *[other] Извлечь { $count } элемента
}
ui-unpacker-choose_directory = Сначала выберите каталог для извлечения
ui-unpacker-dump_parameters = Экспорт GameParams
ui-unpacker-dump_parameters_tooltip = Экспортировать GameParams.data этой версии
ui-unpacker-load_a_build = Сначала загрузите версию игры
ui-unpacker-parameters_written = Параметры записаны
ui-unpacker-save_parameters_title = Сохранить параметры игры
ui-unpacker-writing = Запись...
ui-unpacker-prototype_written = Прототип записан
ui-unpacker-whole_tree = Всё дерево
ui-unpacker-base_only = Только базовые параметры

## ui.unpacker.column
ui-unpacker-column-name = Название
ui-unpacker-column-size = Размер
ui-unpacker-column-type_col = Тип

## ui.armor
ui-armor-load_settings = Загрузите данные игры в настройках, чтобы использовать просмотр брони.
ui-armor-load_failed = Не удалось загрузить данные корабля: { $error }
ui-armor-empty = Пусто
ui-armor-mirror_cameras = Зеркалировать камеры
ui-armor-sync_options = Синхронизация параметров
ui-armor-show_armor_by_default = Показывать броню по умолчанию
ui-armor-sync_tooltip = Синхронизировать видимость брони/корпуса и оверлеи отображения (центр корабля, ватерлиния, кольца камеры) между всеми панелями
ui-armor-common_settings = Общие настройки
ui-armor-tier_filter = Уровни с { $lowest } по { $highest }
ui-armor-fortifications = Укрепления
ui-armor-show_hidden_ships = Показать скрытые корабли
ui-armor-show_hidden_ships_tooltip = Показывать и корабли, которые список пропускает, например кланово арендованные
ui-armor-compare_split = Сравнить в новой панели
ui-armor-open_in_new_tab = Открыть в новой вкладке
ui-armor-export_model = Экспортировать модель корабля
ui-armor-export_disclaimer = Эти 3D-модели и текстуры являются интеллектуальной собственностью Wargaming, и любое использование этих моделей должно соответствовать законодательству вашей страны.
ui-armor-export_button = Экспорт
ui-armor-hull_toggle = Корпус
ui-armor-splash_tooltip = Переключить видимость зон сплэша
ui-armor-show_hidden = Показать скрытые
ui-armor-show_hidden_tooltip = Переключить видимость (возможно) скрытых панелей
ui-armor-gaps = Зазоры
ui-armor-gaps_tooltip = Подсветить граничные рёбра, где бронеплиты не соединяются (возможные зазоры)
ui-armor-display = Отображение
ui-armor-marker_opacity = Прозрачность маркеров
ui-armor-export_tooltip = Экспортировать модель корабля в файл OBJ
ui-armor-pen_check = Проверка пробития
ui-armor-trajectory = Траектория
ui-armor-trajectory_range = Дистанция
ui-armor-trajectory_clear = Очистить всё
ui-armor-arc_heading = Траектория { $number }
ui-armor-arc_summary = { $hits ->
    [one] { $hits } попадание
    [few] { $hits } попадания
    [many] { $hits } попаданий
   *[other] { $hits } попадания
}, { $armor } мм на { $range } км
ui-armor-arc_delete = Удалить эту траекторию
ui-armor-arc_isolate_plates = Изолировать плиты
ui-armor-arc_isolate_plates_tooltip = Показывать только бронеплиты, пересечённые этой траекторией
ui-armor-arc_isolate_zones = Изолировать зоны
ui-armor-arc_isolate_zones_tooltip = Показывать все части бронезон, в которые вошла эта траектория
ui-armor-arc_simulation_caveat = Эта симуляция основана на данных, полученных обратной разработкой, и может не совпадать с расчётом баллистики в игре.
ui-armor-arc_no_shell = Снаряд не выбран; пересечённые плиты показаны без него
ui-armor-arc_detonated_in = Взрыв внутри: { $zone }
ui-armor-arc_detonated_past_armor = Взрыв после прохождения всей брони
ui-armor-arc_ricochet = Рикошет от { $plate }
ui-armor-arc_shatter = Непробитие: { $plate }
ui-armor-arc_stopped = Остановлен на { $plate }
ui-armor-arc_overpen = Сквозное пробитие
ui-armor-arc_overpen_unarmed = Сквозное пробитие (взрыватель не взведён)
ui-armor-arc_plate_unknown = незаписанная плита
ui-armor-angle_good = менее 30 град.
ui-armor-angle_mid = 30-45 град.
ui-armor-angle_bad = более 45 град.
ui-armor-trajectory_ship = Корабль:
ui-armor-trajectory_select_ship = Выбрать корабль
ui-armor-trajectory_ship_tooltip = Выберите корабль, снаряд которого используется для траектории
ui-armor-trajectory_add_ship = Добавить стреляющий корабль
ui-armor-trajectory_add_ship_tooltip = Открыть проверку пробития, чтобы найти и добавить корабль
ui-armor-trajectory_shell = Снаряд:
ui-armor-trajectory_select_shell = Выбрать снаряд
ui-armor-trajectory_shell_tooltip = Выберите снаряд для расчёта траекторий
ui-armor-continue_ricochet = Продолжить после рикошета
ui-armor-splash_mode = Сплэш
ui-armor-splash_no_data = Нет данных о сплэше для этого корабля
ui-armor-splash_no_shells = Добавьте корабль со снарядами HE/SAP в список сравнения
ui-armor-splash_tooltip_mode = Нажмите на броню для визуализации объёма урона от сплэша HE
ui-armor-select_ship = Выберите корабль из списка
ui-armor-all_btn = Все
ui-armor-none_btn = Нет
ui-armor-opaque = Непрозрачный
ui-armor-upgrade = Модернизация:
ui-armor-camo_none = Стандартный (без камуфляжа)
ui-armor-camo_group_universal = Универсальные камуфляжи
ui-armor-camo_group_expendable = Расходуемые камуфляжи
ui-armor-camo_group_other = Прочие камуфляжи
ui-armor-show_armor_thickness = Показывать легенду толщины брони
ui-armor-plate_edges = Края плит
ui-armor-waterline = Ватерлиния
ui-armor-camera_rings = Кольца камеры
ui-armor-realtime_readout = { $health } HP  -  { $hits ->
    [one] { $hits } попадание
    [few] { $hits } попадания
    [many] { $hits } попаданий
   *[other] { $hits } попадания
}
ui-armor-show_camera_rings = Показывать кольца камеры
ui-armor-camera_fov = Поле зрения камеры
ui-armor-camera_height = Высота камеры
ui-armor-show_camera_zoom_path = Показывать траекторию приближения
ui-armor-zoom_path_regular_fov = Обычное поле зрения
ui-armor-zoom_path_max_fov = Макс. поле зрения
ui-armor-camera_perspective = Перспектива камеры
ui-armor-perspective_fov = Поле зрения вида
ui-armor-perspective_projection = Проекция
ui-armor-perspective_proj_game = Как в игре
ui-armor-perspective_proj_center = Через центр
ui-armor-ship_center = Центр корабля
ui-armor-grid = Сетка
ui-armor-length_marks = Отметки длины
ui-armor-length_readout = Длина { $length }, ширина { $beam }
ui-armor-length_metres = { $value } м
ui-armor-length_centimetres = { $value } см
ui-armor-filter_options = Фильтр настроек...
ui-armor-filter_zones = Фильтр зон...
ui-armor-filter_parts = Фильтр частей...
ui-armor-tree_matches = Совпадений: { $count }
ui-armor-tree_no_matches = Нет совпадений
ui-armor-no_matching_options = Нет настроек, подходящих под фильтр
ui-armor-opacity = Прозрачность
ui-armor-opaque_hull = Непрозрачный корпус
ui-armor-lighting = Освещение корпуса
ui-armor-lighting_enabled = Включить освещение
ui-armor-lighting_preset_ingame = Как в игре
ui-armor-lighting_preset_flat = Плоское
ui-armor-lighting_preset_studio = Студийное
ui-armor-lighting_flat = Плоское (рассеянное)
ui-armor-lighting_intensity = Интенсивность света
ui-armor-lighting_azimuth = Азимут источника
ui-armor-lighting_elevation = Высота источника
ui-armor-lighting_rim = Контровой свет
ui-armor-lighting_specular = Блик
ui-armor-lighting_shininess = Глянцевость
ui-armor-lighting_key_color = Цвет основного света
ui-armor-lighting_ambient_color = Цвет рассеянного света
ui-armor-zero_mm_plates = Плиты 0 мм
ui-armor-armor_opacity = Прозрачность брони
ui-armor-ballistic_disclaimer = Баллистические результаты основаны на обратной разработке/оценке работы симуляции
ui-armor-roll = Крен
ui-armor-reset_plates = Сбросить плиты
ui-armor-armor_options = Броня
ui-armor-no_ship_loaded = Корабль не загружен
ui-armor-lod = LOD:
ui-armor-lod_highest = 0 (наивысший)
ui-armor-camouflage = Камуфляж
ui-armor-ships = Корабли
ui-armor-compare = Сравнить
ui-armor-compare_tooltip = Открыть новую панель сравнения или щёлкните корабль правой кнопкой, чтобы сравнить его
ui-armor-mirror_cameras_tooltip = Перемещать камеры всех панелей одновременно
ui-armor-show_armor_by_default_tooltip = Открывать модели с видимой бронёй вместо корпуса
ui-armor-xray_highlight = Рентгеновская подсветка при наведении в окне просмотра
ui-armor-xray_always = Всегда
ui-armor-xray_while_held = При удержании { $key }
ui-armor-xray_off = Выкл.
ui-armor-stack_panes = Панели друг над другом
ui-armor-stack_panes_tooltip = Располагать панели сравнения друг над другом, а не рядом
ui-armor-viewports = Окна просмотра
ui-armor-options_panel = Панель параметров
ui-armor-options_pane = Панель { $number } из { $count }
ui-armor-splash_boxes = Зоны сплэша
ui-armor-cast_here_hint = Удерживая { $key }, щёлкните, чтобы выпустить снаряд сюда
ui-armor-trajectory_next_cast = Следующий выстрел
ui-armor-arc_pick_hint = Щёлкните по метке, чтобы изменить её траекторию.
ui-armor-arc_marker_tooltip = Траектория { $number }. Щёлкните, чтобы изменить. Правый щелчок, чтобы удалить.
ui-armor-zone_toggle_tooltip = Показывает или скрывает все плиты этой зоны. Удерживайте { $key }, чтобы показать только эту зону.
ui-armor-mode_solo_zone = только зона
ui-armor-mode_cast_trajectory = выпустить траекторию
ui-armor-loading_catalog = Загрузка каталога кораблей...
ui-armor-legend_collapse = Свернуть
ui-armor-legend_hide = Скрыть легенду
ui-armor-armor_thickness_title = Толщина брони
ui-armor-initializing = Инициализация 3D-окна...
ui-armor-init_failed = Не удалось инициализировать окно брони: { $reason }
ui-armor-loading_named_ship = Загрузка { $ship }...
ui-armor-options = Параметры

## ui.armor.compass
ui-armor-compass-north = С
ui-armor-compass-east = В
ui-armor-compass-south = Ю
ui-armor-compass-west = З

## ui.armor.realtime
ui-armor-realtime-incoming_fire = Входящий огонь
ui-armor-realtime-attacker_filter = Фильтр атакующих
ui-armor-realtime-all_enemies = Все враги
ui-armor-realtime-show_secondaries = Показать вспомогательное вооружение
ui-armor-realtime-seek_to_salvo = Перейти к залпу
ui-armor-realtime-no_data = Данные о снарядах больше недоступны
ui-armor-realtime-sim_agrees = Симуляция совпадает
ui-armor-realtime-sim_unchecked = { $count ->
    [one] Не удалось смоделировать { $count } снаряд
    [few] Не удалось смоделировать { $count } снаряда
    [many] Не удалось смоделировать { $count } снарядов
   *[other] Не удалось смоделировать { $count } снаряда
}
ui-armor-realtime-sim_rng_zone = Зона случайности ({ $angle } град. в { $from }-{ $to })
ui-armor-realtime-sim_disagrees = Симуляция: { $sim } / Сервер: { $server }
ui-armor-realtime-no_armor_hit = Бронеплиты не поражены
ui-armor-realtime-tracked = Отслежено { $salvos ->
    [one] { $salvos } залп
    [few] { $salvos } залпа
    [many] { $salvos } залпов
   *[other] { $salvos } залпа
}, { $shells ->
    [one] { $shells } снаряд
    [few] { $shells } снаряда
    [many] { $shells } снарядов
   *[other] { $shells } снаряда
}
ui-armor-realtime-nothing_landed = По этому кораблю ещё нет попаданий
ui-armor-realtime-shells = { $count }x
ui-armor-realtime-salvo_shell = { $caliber } мм { $shell }
ui-armor-realtime-salvo_range = { $km } км
ui-armor-realtime-unknown_attacker = Корабль { $id }
ui-armor-realtime-server_outcome = Сервер: { $outcome }
ui-armor-realtime-not_cast = Этот снаряд нельзя проследить через корпус: его полёт или положение корабля не записаны
ui-armor-realtime-pick_hint = Нажмите на залп или снаряд, чтобы проследить его через корпус

## ui.armor.plates
ui-armor-plates-heading = Пробитые плиты ({ $count })
ui-armor-plates-arrival = { $velocity } м/с при попадании, { $time } с полёта, угол падения { $fall } град.
ui-armor-plates-overmatch = Оверматч: бронепробитие { $penetration } мм при { $velocity } м/с
ui-armor-plates-penetrated = Пробивает: { $penetration } мм против { $effective } мм приведённой брони при { $velocity } м/с
ui-armor-plates-ricochet = Рикошет при { $angle } град.
ui-armor-plates-shatter = Разрушается: { $penetration } мм против { $effective } мм приведённой брони
ui-armor-plates-fuse_armed = Взрыватель взведён
ui-armor-plates-detonation = Взрыв внутри { $zone }, { $distance } м после плиты #{ $number }
ui-armor-plates-detonation_past_armor = Взрыв за всей бронёй, { $distance } м после плиты #{ $number }
ui-armor-plates-flat_penetrates = { $ammo } пробивает: { $penetration } мм против { $thickness } мм
ui-armor-plates-flat_shatters = { $ammo } не пробивает: { $penetration } мм против { $thickness } мм
ui-armor-plates-server_angle = сервер { $angle } град.

## advantage
advantage-absolute = Абсолютное
advantage-strong = Сильное
advantage-moderate = Умеренное
advantage-weak = Слабое

## ui.search
ui-search-edit_value = Изменить значение
ui-search-add_filter = Добавить фильтр
ui-search-pill_select = Выбрать
ui-search-pill_deselect = Снять выбор
ui-search-pill_group_all = Сгруппировать через «и»
ui-search-pill_group_any = Сгруппировать через «или»
ui-search-pill_ungroup = Разгруппировать
ui-search-pill_negate = Инвертировать
ui-search-pill_flip_connector = Сменить и/или
ui-search-pill_delete = Удалить
ui-search-match_count_truncated = боёв: { $count }+
ui-search-outcome_win = Победа
ui-search-outcome_loss = Поражение
ui-search-outcome_draw = Ничья
ui-search-outcome_unknown = Неизвестно
ui-search-bool_true = да
ui-search-bool_false = нет
ui-search-match_count = { $count ->
    [one] { $count } бой
    [few] { $count } боя
    [many] { $count } боёв
   *[other] { $count } боя
}
ui-search-row_actions_hint = Действия с реплеем (или щёлкните строку правой кнопкой)
ui-search-looking_up_values = Поиск значений...
ui-search-type_a_query = Введите запрос и нажмите Enter
ui-search-parse_failed = Не удалось разобрать запрос: { $reason }
ui-search-searching = Поиск...
ui-search-failed = Ошибка поиска: { $reason }
ui-search-no_matches = Совпадений нет
ui-search-game_mode_gap_hint = { $count ->
    [one] У { $count } проиндексированного боя не записан игровой режим. Выполните переиндексацию всех реплеев в настройках, чтобы заполнить его.
    [few] У { $count } проиндексированных боёв не записан игровой режим. Выполните переиндексацию всех реплеев в настройках, чтобы заполнить его.
    [many] У { $count } проиндексированных боёв не записан игровой режим. Выполните переиндексацию всех реплеев в настройках, чтобы заполнить его.
   *[other] У { $count } проиндексированных боёв не записан игровой режим. Выполните переиндексацию всех реплеев в настройках, чтобы заполнить его.
}
ui-search-open = Открыть
ui-search-open_missing = Файл реплея больше не существует
ui-search-free_text = содержит "{ $text }"
ui-search-quant_any = любой
ui-search-quant_none = ни один
ui-search-quant_ge = { $n } или более
ui-search-quant_eq = ровно { $n }
ui-search-quant_le = не более { $n }
ui-search-quant_gt = более { $n }
ui-search-quant_lt = менее { $n }
ui-search-quant_ne = не { $n }
ui-search-scope_self = Я
ui-search-scope_ally = Союзник
ui-search-scope_enemy = Противник
ui-search-scope_div = Дивизион

## ui.search.suggest
ui-search-suggest-preset_divmate_test_ship = Кто-то в моём дивизионе играл на тестовом корабле
ui-search-suggest-preset_no_enemy_cv = У противника нет авианосца
ui-search-suggest-preset_all_enemies_died = Все противники уничтожены
ui-search-suggest-preset_high_damage_enemies = 3 или более противников свыше 100к
ui-search-suggest-preset_stream_sniper = Есть стримснайпер
ui-search-suggest-preset_i_survived = Я выжил
ui-search-suggest-preset_i_disconnected = Я отключился
ui-search-suggest-my_damage = Мой урон
ui-search-suggest-enemy_ship = Корабль противника
ui-search-suggest-allied_ship = Корабль союзника
ui-search-suggest-player_in_match = Игрок был в бою
ui-search-suggest-someone_in_division = Кто-то в моём дивизионе
ui-search-suggest-scope_self = Мой
ui-search-suggest-scope_ally = Союзник
ui-search-suggest-scope_enemy = Противник
ui-search-suggest-scope_division = Дивизион
ui-search-suggest-scope_anyone = Любой игрок

## ui.search.field
ui-search-field-outcome = Результат
ui-search-field-map = Карта
ui-search-field-class = Класс
ui-search-field-tier = Уровень
ui-search-field-date = Дата
ui-search-field-player_present = Игрок
ui-search-field-group = Группа реплеев
ui-search-field-stat_damage = Урон
ui-search-field-stat_kills = Убийства
ui-search-field-stat_spotting = Засвет
ui-search-field-stat_potential = Потенциальный урон
ui-search-field-stat_received = Полученный урон
ui-search-field-stat_pr = PR
ui-search-field-rating = Рейтинг
ui-search-field-stat_survived = Выжил
ui-search-field-stat_disconnected = Отключился
ui-search-field-game_type = Тип игры
ui-search-field-game_mode = Игровой режим
ui-search-field-match_group = Группа боя
ui-search-field-build = Сборка игры
ui-search-field-results_available = Результаты доступны
ui-search-field-relation = Сторона
ui-search-field-division = Дивизион
ui-search-field-name = Имя
ui-search-field-clan = Клан
ui-search-field-realm = Регион
ui-search-field-ship = Корабль
ui-search-field-ship_index = Индекс корабля
ui-search-field-nation = Нация
ui-search-field-test_ship = Тестовый корабль
ui-search-field-stream_sniper = Стримснайпер
ui-search-field-sniper_login = Логин снайпера на Twitch

## ui.search.op
ui-search-op-contains = содержит
ui-search-op-equals = совпадает с
ui-search-op-not_equals = не совпадает с
ui-search-op-eq = =
ui-search-op-ne = !=
ui-search-op-gt = >
ui-search-op-ge = >=
ui-search-op-lt = <
ui-search-op-le = <=
ui-search-op-is = равно
ui-search-op-is_not = не равно
ui-search-op-is_set = задано
ui-search-op-is_not_set = не задано

## ui.search.value
ui-search-value-relation_self = Я
ui-search-value-relation_ally = Союзник
ui-search-value-relation_enemy = Противник
ui-search-value-division_mine = Мой
ui-search-value-division_any = Любой
ui-search-value-division_none = Нет
ui-search-value-class_air_carrier = Авианосец
ui-search-value-class_battleship = Линкор
ui-search-value-class_cruiser = Крейсер
ui-search-value-class_destroyer = Эсминец
ui-search-value-class_submarine = Подводная лодка
ui-search-value-class_auxiliary = Вспомогательный корабль
ui-search-value-rating_bad = Плохо
ui-search-value-rating_below_average = Ниже среднего
ui-search-value-rating_average = Средне
ui-search-value-rating_good = Хорошо
ui-search-value-rating_very_good = Очень хорошо
ui-search-value-rating_great = Отлично
ui-search-value-rating_unicum = Уникум
ui-search-value-rating_super_unicum = Супер уникум
ui-search-value-game_mode_test = Тест
ui-search-value-game_mode_standard = Стандартный бой
ui-search-value-game_mode_singlebase = Одна база
ui-search-value-game_mode_domination = Превосходство
ui-search-value-game_mode_tutorial = Обучение
ui-search-value-game_mode_megabase = Мегабаза
ui-search-value-game_mode_forts = Форты
ui-search-value-game_mode_standard_domination = Стандартное превосходство
ui-search-value-game_mode_epicenter = Эпицентр
ui-search-value-game_mode_assault_defense = Штурм и оборона
ui-search-value-game_mode_pve = PVE
ui-search-value-game_mode_arms_race = Гонка вооружений
ui-search-value-game_mode_epicenter_ring = Кольцо эпицентра
ui-search-value-game_mode_anti_standard = Анти-стандарт
ui-search-value-game_mode_attack_defense = Атака и оборона
ui-search-value-game_mode_torpedo_beat = Торпедный бит
ui-search-value-game_mode_team_battle_royale = Командная королевская битва
ui-search-value-game_mode_escape_to_portal = Побег к порталу
ui-search-value-game_mode_domination_asymm = Превосходство (асимм.)
ui-search-value-game_mode_key_battle = Ключевой бой
ui-search-value-game_mode_portal_2021 = Портал 2021
ui-search-value-game_mode_team_battle_royale_2021 = Командная королевская битва 2021
ui-search-value-game_mode_convoy_event = Конвой (событие)
ui-search-value-game_mode_convoy_airship = Конвой: дирижабль
ui-search-value-game_mode_two_teams_battle_royale = Королевская битва двух команд
ui-search-value-game_mode_pinata_event = Пиньята (событие)
ui-search-value-game_mode_respawns = Возрождения
ui-search-value-game_mode_respawns_sectors = Возрождения по секторам

## ui.search.column
ui-search-column-date = Дата
ui-search-column-map = Карта
ui-search-column-mode = Режим
ui-search-column-ship = Ваш корабль
ui-search-column-result = Результат
ui-search-column-damage = Урон
ui-search-column-kills = Убийства
ui-search-column-pr = PR
ui-search-column-mc = MC

## ui.settings.index
ui-settings-index-heading = Индекс реплеев
ui-settings-index-description = Доступная для поиска база всех реплеев в вашем каталоге
ui-settings-index-build = Построить индекс
ui-settings-index-rebuild = Переиндексировать всё
ui-settings-index-rebuild_hint = Заново прочитать все реплеи, чтобы исправить записи, неверно разобранные ранее. «Построить индекс» читает только недостающее.
ui-settings-index-progress = Прочитано { $done } из { $total }, ошибок: { $failed }
ui-settings-index-progress_skipped = Прочитано { $done } из { $total }, уже в индексе: { $skipped }, ошибок: { $failed }
ui-settings-index-built = Проиндексирован{ $indexed ->
    [one] { $indexed } реплей
    [few] о { $indexed } реплея
    [many] о { $indexed } реплеев
   *[other] о { $indexed } реплея
}, не удалось прочитать: { $failed }
ui-settings-index-built_with_skipped = Проиндексирован{ $indexed ->
    [one] { $indexed } реплей
    [few] о { $indexed } реплея
    [many] о { $indexed } реплеев
   *[other] о { $indexed } реплея
}, уже в индексе: { $skipped }, не удалось прочитать: { $failed }

## ui.settings
ui-settings-loading = Загрузка настроек...
ui-settings-load_failed = Не удалось загрузить настройки
ui-settings-not_set = (не задано)

## ui.settings.panel
ui-settings-panel-mcp = Сервер MCP
ui-settings-panel-open = Открыть настройки
ui-settings-panel-search_placeholder = Поиск настроек
ui-settings-panel-count = { $count ->
    [one] { $count } настройка
    [few] { $count } настройки
    [many] { $count } настроек
   *[other] { $count } настройки
}
ui-settings-panel-no_matches = Нет настроек, соответствующих запросу
ui-settings-panel-close = Закрыть настройки
ui-settings-panel-in_error = { $name }: параметр требует внимания
ui-settings-panel-general = Общие
ui-settings-panel-appearance = Внешний вид
ui-settings-panel-game = World of Warships
ui-settings-panel-replays = Реплеи
ui-settings-panel-collaboration = Совместная работа
ui-settings-panel-twitch = Twitch
ui-settings-panel-language_description = Язык интерфейса
ui-settings-panel-data_sharing_description = Что отправляется на ShipBuilds.com после ранговых и случайных боёв
ui-settings-panel-code_integrity_description = Можно ли загружать в это приложение код, не подписанный Microsoft
ui-settings-panel-check_for_updates_description = Проверять наличие новой версии при каждом запуске приложения
ui-settings-panel-data_folder = Папка данных приложения
ui-settings-panel-theme_description = Цвета интерфейса. Наведите указатель на вариант для предпросмотра.
ui-settings-panel-zoom_description = Размер всего текста и элементов управления
ui-settings-panel-zoom = Масштаб
ui-settings-panel-zoom_shortcuts = { $zoom_in } увеличивает, { $zoom_out } уменьшает, { $reset } возвращает 1x
ui-settings-panel-zoom_changed = Масштаб: { $factor }x
ui-settings-panel-cache_directory = Каталог кэша
ui-settings-panel-cache_directory_description = Где хранятся кэшированные сборки
ui-settings-panel-cached_builds = Кэшированные сборки
ui-settings-panel-current_path_description = Последний открытый файл реплея
ui-settings-panel-show_raw_xp_description = Добавить в таблицу результатов столбец чистого опыта (XP)
ui-settings-panel-show_observed_damage_description = Добавить в таблицу результатов столбец наблюдаемого урона
ui-settings-panel-show_entity_id_description = Добавить в таблицу результатов столбец ID сущности
ui-settings-panel-show_heals_description = Добавить в таблицу результатов столбец лечения
ui-settings-panel-auto_export_description = Записывать данные каждого боя в файл в выбранном формате и каталоге
ui-settings-panel-display_name_description = Имя, которое видят другие участники сеанса
ui-settings-panel-get_token_description = Открыть страницу входа, выдающую токен чата
ui-settings-panel-token = Токен входа
ui-settings-panel-monitored_channel_description = Отслеживаемый канал. Оставьте пустым, чтобы отслеживать свой.
ui-settings-panel-armor_defaults = Сохранённые настройки
ui-settings-panel-armor_saved_description = Сохранено из параметров просмотра моделей

## ui.settings.licenses
ui-settings-licenses-title = О программе и лицензии
ui-settings-licenses-back = Назад к настройкам
ui-settings-licenses-third_party_intro = Это приложение содержит перечисленные ниже сторонние компоненты. Выберите запись, чтобы показать использующие её пакеты и полный текст лицензии.

## ui.settings.armor
ui-settings-armor-no_defaults = (нет сохранённых настроек)

## ui.app
ui-app-status_cache_checking = Проверка кэша игровых данных
ui-app-status_cache_validating = Проверка целостности кэша игровых данных
ui-app-status_cache_downloading = Загрузка игровых данных
ui-app-status_cache_planning = Запрос списка опубликованных игровых данных
ui-app-drop_to_load = Отпустите, чтобы загрузить { $file }
ui-app-drop_to_list = Отпустите, чтобы показать реплеи из { $folder }
ui-app-drop_to_list_replays = Отпустите, чтобы показать { $count ->
    [one] { $count } реплей
    [few] { $count } реплея
    [many] { $count } реплеев
   *[other] { $count } реплея
}
ui-app-drop_to_list_folders = Отпустите, чтобы показать реплеи из { $count ->
    [one] { $count } папки
    [few] { $count } папок
    [many] { $count } папок
   *[other] { $count } папок
}
ui-app-drop_to_open_and_list = Отпустите, чтобы открыть { $file } и показать реплеи из: { $folders }
ui-app-drop_to_list_replays_and_folders = Отпустите, чтобы показать { $count ->
    [one] { $count } реплей
    [few] { $count } реплея
    [many] { $count } реплеев
   *[other] { $count } реплея
} и реплеи из { $folders }
ui-app-drop_folder_count = { $count ->
    [one] { $count } папка
    [few] { $count } папки
    [many] { $count } папок
   *[other] { $count } папки
}
ui-app-reading_replay = Чтение { $replay }
ui-app-reading_replays = Чтение { $count ->
    [one] { $count } реплея
    [few] { $count } реплеев
    [many] { $count } реплеев
   *[other] { $count } реплеев
}
ui-app-status_constants_generating = Обновление игровых констант для { $version }
ui-app-worker_constants = Обработчик игровых констант
ui-app-worker_live_match = Отслеживание боя
ui-app-worker_replay_loader = Загрузчик реплеев
ui-app-worker_restarting = { $worker }: остановлен и перезапускается
ui-app-worker_stalled = { $worker }: не отвечает
ui-app-worker_stopped = { $worker }: остановлен
ui-app-status_indexing = Индексация реплеев
ui-app-status_of = { $done } из { $total }
ui-app-status_tasks = { $count ->
    [one] Выполняется { $count } задача
    [few] Выполняются { $count } задачи
    [many] Выполняется { $count } задач
   *[other] Выполняются { $count } задачи
}
ui-app-status_show_panel = Показать панель: { $panel }
ui-app-status_hide_panel = Скрыть панель: { $panel }
ui-app-panel_left = Левая
ui-app-panel_right = Правая
ui-app-panel_bottom = Нижняя
ui-app-debug_build = Отладочная сборка

## ui.palette
ui-palette-title = Команды
ui-palette-placeholder = Введите команду или запрос...
ui-palette-go_to = Перейти: { $tab }
ui-palette-advanced_search = Расширенный поиск...
ui-palette-set_theme = Тема: { $theme }
ui-palette-games_i_died_in = Бои, в которых я погиб
ui-palette-games_i_won = Бои, которые я выиграл
ui-palette-send_all_replays = Отправить все реплеи в ShipBuilds
ui-palette-send_all_replays_again = Отправить все реплеи в ShipBuilds (игнорировать журнал отправок)
ui-palette-search_players = Поиск игроков...
ui-palette-my_matches_in_ship = Мои бои на корабле...
ui-palette-open_ship_in_armor_viewer = Открыть корабль в просмотре моделей...
ui-palette-open_ship_in_shiptool = Открыть корабль в ShipTool...
ui-palette-players_title = Поиск игроков
ui-palette-my_ships_title = Мои бои на корабле
ui-palette-armor_ships_title = Открыть корабль в просмотре моделей
ui-palette-shiptool_ships_title = Открыть корабль в ShipTool
ui-palette-players_placeholder = Поиск игроков по имени...
ui-palette-my_ships_placeholder = Поиск кораблей, на которых вы играли...
ui-palette-armor_ships_placeholder = Поиск любого корабля по названию...
ui-palette-armor_ship_row = { $name } -- { $class } { $tier } уровня, { $nation }
ui-palette-back_hint = Esc: назад
ui-palette-ships_loading = Загрузка списка кораблей...
ui-palette-ships_unavailable = Нет списка кораблей: { $reason }
ui-palette-matches_found = { $count ->
    [one] { $count } совпадение
    [few] { $count } совпадения
    [many] { $count } совпадений
   *[other] { $count } совпадения
}
ui-palette-sample_stats = Отладка: показать пример данных в статистике
ui-palette-restore_stats = Отладка: показать записанные данные в статистике

## ui.replay.hover
ui-replay-hover-ship = Корабль
ui-replay-hover-map = Карта
ui-replay-hover-mode = Режим
ui-replay-hover-blank_mode = Пусто
ui-replay-hover-played = Сыграно
ui-replay-hover-damage = Урон
ui-replay-hover-kills = Уничтожено
ui-replay-hover-result = Результат
ui-replay-hover-division = Дивизион
ui-replay-hover-twitch = Чат Twitch

## ui.replay.renderer
ui-replay-renderer-export_video = Рендер в видео
ui-replay-renderer-export_clipboard = Рендер в буфер обмена
ui-replay-renderer-export_saved = Видео сохранено в { $path }
ui-replay-renderer-export_copied = Видео скопировано в буфер обмена
ui-replay-renderer-export_not_written = Не удалось отрендерить видео: { $reason }
ui-replay-renderer-pop_out = Открыть в отдельном окне
ui-replay-renderer-export_path_unusable = В это расположение невозможно записать
ui-replay-renderer-batch_started = Рендер { $count ->
    [one] { $count } реплея
    [few] { $count } реплеев
    [many] { $count } реплеев
   *[other] { $count } реплеев
} в { $dir }
ui-replay-renderer-batch_finished = Отрисован{ $written ->
    [one] { $written } реплей
    [few] о { $written } реплея
    [many] о { $written } реплеев
   *[other] о { $written } реплея
}; не удалось отрисовать: { $failed }
ui-replay-renderer-batch_all_written = Отрисован{ $written ->
    [one] { $written } реплей
    [few] о { $written } реплея
    [many] о { $written } реплеев
   *[other] о { $written } реплея
}
ui-replay-renderer-batch_copied = { $written ->
    [one] Отрисован { $written } реплей и помещён в буфер обмена
    [few] Отрисовано { $written } реплея и помещены в буфер обмена
    [many] Отрисовано { $written } реплеев и помещены в буфер обмена
   *[other] Отрисовано { $written } реплея и помещены в буфер обмена
}
ui-replay-renderer-batch_not_copied = Отрисовано в { $dir }, но буфер обмена их не принял: { $reason }
ui-replay-renderer-batch_progress = Рендер { $done } из { $total }: { $replay }
ui-replay-renderer-baking = Чтение реплея и обработка боя...
ui-replay-renderer-failed = Не удалось воспроизвести этот реплей
ui-replay-renderer-play = Воспроизвести
ui-replay-renderer-pause = Пауза

## ui.renderer.annotations
ui-renderer-annotations-title = Инструменты рисования
ui-renderer-annotations-undo = Отменить
ui-renderer-annotations-redo = Повторить
ui-renderer-annotations-ship_hint = Найти корабль
ui-renderer-annotations-no_ship = Корабль не выбран
ui-renderer-annotations-nib = Толщина линии
ui-renderer-annotations-thinner = Тоньше
ui-renderer-annotations-thicker = Толще
ui-renderer-annotations-shortcuts_title = Сочетания клавиш
ui-renderer-annotations-shortcuts_tools = Инструменты
ui-renderer-annotations-shortcuts_actions = Действия
ui-renderer-annotations-tool_select = Выделение
ui-renderer-annotations-tool_arrow = Стрелка
ui-renderer-annotations-tool_freehand = Карандаш
ui-renderer-annotations-tool_line = Линия
ui-renderer-annotations-tool_circle = Круг
ui-renderer-annotations-tool_rectangle = Прямоугольник
ui-renderer-annotations-tool_triangle = Треугольник
ui-renderer-annotations-tool_measurement = Измерение
ui-renderer-annotations-tool_eraser = Ластик
ui-renderer-annotations-action_undo = Отменить
ui-renderer-annotations-action_multi_select = Множественный выбор
ui-renderer-annotations-action_stroke_width = Толщина линии
ui-renderer-annotations-action_delete = Удалить
ui-renderer-annotations-action_cancel = Отмена / снять выбор
ui-renderer-annotations-toggle_fill = Нажмите ещё раз, чтобы переключить заливку
ui-renderer-annotations-clear_all = Очистить всё
ui-renderer-annotations-edit = Изменить рисунок
ui-renderer-annotations-size = Размер
ui-renderer-annotations-color = Цвет
ui-renderer-annotations-custom_ink = Свой цвет
ui-renderer-annotations-filled = Заливка
ui-renderer-annotations-locked = Аннотации заблокированы хостом

## ui.armor.cast
ui-armor-cast-prompt = { $key }+щелчок по корпусу добавляет точку попадания
ui-armor-cast-empty = Точек попадания пока нет
ui-armor-cast-miss_nothing_under_pointer = Под курсором нет бронеплиты
ui-armor-cast-miss_thickness_unknown = У плит этой модели нет известной толщины, по которой можно выстрелить
ui-armor-cast-miss_no_plate_on_path = Траектория снаряда под этим углом не пересекает ни одной бронеплиты

## ui.armor.pen
ui-armor-pen-search_hint = Поиск кораблей для сравнения...
ui-armor-pen-add_a_ship = Найдите и добавьте корабли выше, чтобы сравнить пробитие
ui-armor-pen-hover_a_plate = Наведите курсор на плиту корабля, чтобы проверить её
ui-armor-pen-awaiting_plate = наведите на плиту
ui-armor-pen-clear_all = Очистить всё
ui-armor-pen-remove = Убрать из сравнения
ui-armor-pen-firing_ship_tooltip = Использовать снаряды этого корабля для траекторий
ui-armor-pen-use_for_trajectory = Использовать для траектории
ui-armor-pen-no_shells = У { $ship } нет снарядов ГК
ui-armor-pen-ifhe = ИВ
ui-armor-pen-ifhe_tooltip = Инерционный взрыватель ОФ-снарядов увеличивает бронепробиваемость ОФ-снарядов на четверть
ui-armor-pen-penetrates = пробивает
ui-armor-pen-shatters = не пробивает
ui-armor-pen-angle_dependent = зависит от угла
ui-armor-pen-no_data = нет данных о пробитии

## ui.armor.splash
ui-armor-splash-hint = Нажмите на броню в режиме сплэша, чтобы увидеть, каких зон достигает разрыв
ui-armor-splash-half_extent = Половина размера разрыва: { $value } ед. модели
ui-armor-splash-triangles = Треугольников брони в зоне разрыва: { $total }, пробито: { $penetrated }
ui-armor-splash-no_zones = В этой точке не пересекается ни одна зона сплэша
ui-armor-splash-zones = Зоны в области разрыва
ui-armor-splash-direct_hit = Прямое попадание
ui-armor-splash-hp = { $value } HP
ui-armor-splash-section = Зоны сплэша

## ui.armor.export
ui-armor-export-hull = Корпус
ui-armor-export-contents = Содержимое
ui-armor-export-contents_mesh = Модель
ui-armor-export-contents_armor = Броня
ui-armor-export-contents_both = Модель и броня
ui-armor-export-lod = Детализация модели
ui-armor-export-lod_tooltip = 0 — максимальная детализация. У брони нет уровней детализации, на неё это не влияет.
ui-armor-export-res = Разрешение текстур
ui-armor-export-res_full = Полное
ui-armor-export-res_2048 = 2048 px
ui-armor-export-res_1024 = 1024 px
ui-armor-export-res_512 = 512 px
ui-armor-export-camos = Камуфляжи
ui-armor-export-camo_select_none = Снять выбор
ui-armor-export-camo_reset = Сбросить по умолчанию
ui-armor-export-loading_details = Загрузка...
ui-armor-export-size_estimate = Ожидаемый размер файла: { $size }
ui-armor-export-size_breakdown = Геометрия { $geometry } (точно), текстуры { $textures } (оценка)
ui-armor-export-size_incomplete = Не удалось оценить { $count ->
    [one] { $count } камуфляж
    [few] { $count } камуфляжа
    [many] { $count } камуфляжей
   *[other] { $count } камуфляжа
}; размер может быть неполным
ui-armor-export-exporting = Экспорт { $ship }...
ui-armor-export-already_running = Экспорт уже выполняется
ui-armor-export-exported = Экспортирован { $ship }
ui-armor-export-exported_with_size = Экспортирован { $ship } ({ $size })
ui-armor-export-vehicle_not_found = Корабль не найден
ui-armor-export-export_failed = Ошибка экспорта: { $error }

## ui.armor.outcome
ui-armor-outcome-penetration = Пробитие
ui-armor-outcome-citadel = Цитадель
ui-armor-outcome-ricochet = Рикошет
ui-armor-outcome-shatter = Непробитие
ui-armor-outcome-overpenetration = Сквозное пробитие
ui-armor-outcome-underwater = Под водой
ui-armor-outcome-unknown = Не распознано ({ $raw })
ui-armor-outcome-stopped = Застрял в броне

## ui.tactics
ui-tactics-a_peers_board = Доска, которой поделился другой участник сеанса
ui-tactics-map = Карта
ui-tactics-map_hint = Найти карту
ui-tactics-mode = Режим
ui-tactics-pick_a_map = Выберите карту для рисования
ui-tactics-drawing = Отрисовка карты...
ui-tactics-reset_view = Сбросить вид
ui-tactics-add_cap = Добавить точку
ui-tactics-add_cap_tooltip = Поставить точку захвата следующим щелчком по карте
ui-tactics-selected_cap = Точка { $letter }
ui-tactics-cap_radius = { $km } км
ui-tactics-cap_team = Команда
ui-tactics-cap_team_tooltip = Переключить команду выбранной точки захвата
ui-tactics-team_neutral = Нейтральная
ui-tactics-team_one = Команда 1 (зелёная)
ui-tactics-team_two = Команда 2 (красная)
ui-tactics-delete_cap = Удалить
ui-tactics-delete_cap_tooltip = Удалить выбранную точку захвата
ui-tactics-cap_replay_delete_tooltip = Точку захвата, загруженную из реплея, удалить нельзя
ui-tactics-cap_narrow_tooltip = Уменьшить радиус выбранной точки захвата
ui-tactics-cap_widen_tooltip = Увеличить радиус выбранной точки захвата
ui-tactics-undo_tooltip = Отменить последнее изменение доски ({ $shortcut })
ui-tactics-redo_tooltip = Повторить последнее отменённое изменение ({ $shortcut } или { $alternate })
ui-tactics-clear_caps = Удалить добавленные точки
ui-tactics-clear_added_caps_tooltip = Удалить точки захвата, добавленные пользователем
ui-tactics-preset_name = Название доски
ui-tactics-preset_save = Сохранить
ui-tactics-preset_delete = Удалить
ui-tactics-reset_board = Сбросить доску
ui-tactics-reset_board_tooltip = Восстановить точки выбранного режима и очистить рисунки
ui-tactics-preset_needs_a_name = Сначала задайте название доски
ui-tactics-preset_needs_a_map = Сначала выберите карту для доски
ui-tactics-preset_saved = Сохранено: { $name }
ui-tactics-preset_save_failed = Не удалось сохранить доску: { $error }
ui-tactics-preset_load_failed = Не удалось открыть доску: { $error }
ui-tactics-preset_delete_failed = Не удалось удалить доску: { $error }
ui-tactics-presets = Сохранённые доски
ui-tactics-preset_open = Открыть
ui-tactics-clear_drawing = Очистить рисунок
ui-tactics-scan_replays = Заполнить точки из реплеев
ui-tactics-scan_replays_tooltip = Прочитать реплеи, чтобы получить расположение точек захвата для сыгранных режимов
ui-tactics-scan_needs_game_data = Сначала загрузите версию игры
ui-tactics-scan_running = Чтение реплеев...
ui-tactics-scan_progress = Чтение реплеев... { $read } / { $total }
ui-tactics-scan_done = Прочитан{ $total ->
    [one] { $total } реплей
    [few] о { $total } реплея
    [many] о { $total } реплеев
   *[other] о { $total } реплея
}, найден{ $added ->
    [one] а { $added } новая раскладка
    [few] о { $added } новые раскладки
    [many] о { $added } новых раскладок
   *[other] о { $added } новой раскладки
}
ui-tactics-session_boards_full = В сессии уже открыт{ $limit ->
    [one] а { $limit } доска
    [few] о { $limit } доски
    [many] о { $limit } досок
   *[other] о { $limit } доски
}, поэтому эта доска остаётся на своей карте
ui-tactics-placing = Размещение: { $ship }
ui-tactics-side_friendly = Союзник
ui-tactics-side_enemy = Противник
ui-tactics-ink_white = Белый цвет
ui-tactics-ink_gray = Серый цвет
ui-tactics-ink_red = Красный цвет
ui-tactics-ink_orange = Оранжевый цвет
ui-tactics-ink_yellow = Жёлтый цвет
ui-tactics-ink_green = Зелёный цвет
ui-tactics-ink_blue = Синий цвет
ui-tactics-ink_purple = Фиолетовый цвет
ui-tactics-ink_pink = Розовый цвет
ui-tactics-no_map_art = В этой версии нет изображения для этой карты
ui-tactics-ship_config = Настройка корабля
ui-tactics-hull = Корпус
ui-tactics-hull_default = По умолчанию
ui-tactics-modifiers = Модификаторы
ui-tactics-concealment_expert = Мастер маскировки
ui-tactics-gun_range_mod = Мод. дальности ГК
ui-tactics-secondary_range_mod = Мод. дальности ПМК
ui-tactics-ship_ranges = Круги дальности
ui-tactics-shape = Фигура
ui-tactics-shape_size = Размер
ui-tactics-shape_size_km = Размер (км)
ui-tactics-shape_filled = Заливка
ui-tactics-shape_smaller = Уменьшить фигуру
ui-tactics-shape_larger = Увеличить фигуру

## ui.armor.sim
ui-armor-sim-overmatch_rules_out_ricochet = Овермэтч, рикошета нет
ui-armor-sim-always_ricochet_band = Рикошет: угол в зоне гарантированного рикошета
ui-armor-sim-ricochet_angle_too_low = Рикошет невозможен, угол слишком мал

## ui.launcher
ui-launcher-rescan = Повторный поиск
ui-launcher-add_install = Добавить установку...
ui-launcher-add_install_title = Каталог установки World of Warships
ui-launcher-no_installs = Установки не найдены. Добавьте существующую или установите игру ниже.
ui-launcher-managed_by_steam = управляется Steam
ui-launcher-checking_updates = проверка обновлений
ui-launcher-up_to_date = Актуальная версия
ui-launcher-update_available = Доступно обновление: { $parts }
ui-launcher-not_installed = не установлено: { $parts }
ui-launcher-check_failed = ошибка проверки обновлений: { $error }
ui-launcher-recheck = Проверить снова
ui-launcher-download = Загрузить / обновить...
ui-launcher-download_title = Куда установить версию
ui-launcher-download_hint = Новая установка занимает десятки ГБ; дельта-обновления пока не поддерживаются, поэтому изменённая часть загружается заново целиком.
ui-launcher-pause = Пауза
ui-launcher-cancel = Отмена
ui-launcher-resume = Продолжить
ui-launcher-paused = Загрузка приостановлена; загруженные файлы сохранены.
ui-launcher-installed = Установлено
ui-launcher-installed_skipped = Установлено (пропущен{ $count ->
    [one] а { $count } дельта-часть
    [few] о { $count } дельта-части
    [many] о { $count } дельта-частей
   *[other] о { $count } дельта-части
}: пока не поддерживаются, загружены, но не применены)
ui-launcher-download_failed = Ошибка загрузки: { $error }
ui-launcher-update_in_place = Обновление устанавливается в папку самой установки. Выберите { $path }, чтобы обновить её.
ui-launcher-cancelled = Загрузка отменена.
ui-launcher-launch_failed = Ошибка запуска: { $error }
ui-launcher-use_for_toolkit = Использовать в Toolkit
ui-launcher-install_heading = Установить игру
ui-launcher-install_hint = Новая установка занимает десятки ГБ.
ui-launcher-install_title = Куда установить игру
ui-launcher-install = Установить...
ui-launcher-checking_version = проверка доступной версии
ui-launcher-version = версия { $version }
ui-launcher-version_unreported = версия клиента не сообщена
ui-launcher-unavailable = недоступно: { $error }
ui-launcher-retry = Повторить

## ui.launcher.account
ui-launcher-account-sign_in = Войти
ui-launcher-account-add_title = Добавить учётную запись
ui-launcher-account-again_title = Войти снова
ui-launcher-account-memory_only = Вход выполнен только на этот сеанс. Он не сохраняется, поэтому после перезапуска потребуется войти снова.
ui-launcher-account-password_note = Ваш пароль передаётся только Wargaming и никогда не сохраняется.
ui-launcher-account-realm_fixed = Регион: { $realm }
ui-launcher-account-email = Эл. почта
ui-launcher-account-password = Пароль
ui-launcher-account-code = Код двухфакторной аутентификации
ui-launcher-account-code_required = Требуется код двухфакторной аутентификации.
ui-launcher-account-use_backup_code = Использовать резервный код
ui-launcher-account-submit_code = Отправить код
ui-launcher-account-captcha = Для этого входа требуется CAPTCHA, которую лаунчер пока не может показать. Войдите один раз через официальный Wargaming Game Center или wargaming.net в браузере, затем повторите попытку.

## ui.launcher.sign_in_error
ui-launcher-sign_in_error-wrong_credentials = Неверный адрес электронной почты или пароль.
ui-launcher-sign_in_error-two_factor_required = Для этого аккаунта требуется двухфакторное подтверждение. Подтвердите вход в Wargaming Auth или кодом из письма, затем войдите снова.
ui-launcher-sign_in_error-two_factor_invalid = Двухфакторный код неверен или устарел. Введите новый код.
ui-launcher-sign_in_error-captcha_required = Wargaming запрашивает CAPTCHA. Войдите один раз в официальном Game Center или на wargaming.net, затем повторите попытку.
ui-launcher-sign_in_error-rate_limited = Слишком много попыток входа. Подождите несколько минут и повторите попытку.
ui-launcher-sign_in_error-account_banned = Этот аккаунт заблокирован.
ui-launcher-sign_in_error-network_unreachable = Не удаётся связаться с Wargaming. Проверьте подключение к интернету.
ui-launcher-sign_in_error-service_unavailable = Служба входа Wargaming недоступна. Повторите попытку позже.
ui-launcher-sign_in_error-session_expired = Срок действия сохранённого входа истёк. Войдите снова.
ui-launcher-sign_in_error-keyring_locked = Системная связка ключей заблокирована. Разблокируйте её и войдите снова.
ui-launcher-sign_in_error-not_saved = Вход выполнен, но сохранить его не удалось. После перезапуска потребуется войти снова.
ui-launcher-sign_in_error-unexpected = Не удалось войти. Подробности в журнале.

## ui.launcher.realm
ui-launcher-realm-eu = Европа
ui-launcher-realm-na = Северная Америка
ui-launcher-realm-asia = Азия

## ui.launcher.runner
ui-launcher-runner-looking = Поиск Proton.
ui-launcher-runner-steam_proton = { $name } из Steam
ui-launcher-runner-umu = umu-run с { $name }
ui-launcher-runner-umu_default = umu-run с UMU-Proton
ui-launcher-runner-native = Игра запускается нативно.
ui-launcher-runner-first_run_prefix = При первом запуске создаётся префикс Wine, это занимает несколько секунд.
ui-launcher-runner-first_run_download = При первом запуске загружаются UMU-Proton и среда выполнения Steam, около 700 МБ.
ui-launcher-runner-choose = Выбрать папку Proton
ui-launcher-runner-automatic = Найти автоматически
ui-launcher-runner-detect = Искать снова
ui-launcher-runner-needed = Для запуска в Linux нужен Proton. Установите Steam с Proton или umu-launcher.

## ui.replay.damage_share
ui-replay-damage_share-mode = Урон как доля от общего у другого корабля
ui-replay-damage_share-dealt = { $pct } урона, нанесённого { $ship }, пришлось на { $other }.
ui-replay-damage_share-dealt_inverse = { $pct } урона, полученного { $other }, нанёс { $ship }.
ui-replay-damage_share-received = { $pct } урона, полученного { $ship }, нанёс { $other }.
ui-replay-damage_share-received_inverse = { $pct } урона, нанесённого { $other }, пришлось на { $ship }.
ui-replay-damage_share-hold = Удерживайте { $key } для инверсии.
ui-replay-damage_share-release = Отпустите { $key }, чтобы вернуться.

## ui.reindex
ui-reindex-context = { $count ->
    [1] Принудительно переиндексировать
    [one] Принудительно переиндексировать { $count } реплей
    [few] Принудительно переиндексировать { $count } реплея
    [many] Принудительно переиндексировать { $count } реплеев
   *[other] Принудительно переиндексировать { $count } реплея
}
ui-reindex-palette_by_version = Принудительная переиндексация реплеев: по версии
ui-reindex-palette_all = Принудительная переиндексация реплеев: все
ui-reindex-versions_title = Принудительная переиндексация реплеев: по версии
ui-reindex-versions_placeholder = Фильтр версий игры...
ui-reindex-version_row = { $version } -- { $count ->
    [one] { $count } реплей
    [few] { $count } реплея
    [many] { $count } реплеев
   *[other] { $count } реплея
}
ui-reindex-no_versions = Ни в одном реплее из списка не указана версия игры
ui-reindex-confirm_title = Принудительная переиндексация реплеев
ui-reindex-confirm_ok = Переиндексировать
ui-reindex-confirm_count = { $count ->
    [one] { $count } реплей будет прочитан заново, а его запись в индексе перезаписана.
    [few] { $count } реплея будут прочитаны заново, а их записи в индексе перезаписаны.
    [many] { $count } реплеев будут прочитаны заново, а их записи в индексе перезаписаны.
   *[other] { $count } реплея будут прочитаны заново, а их записи в индексе перезаписаны.
}
ui-reindex-confirm_range = Сыграны с { $oldest } по { $newest }.
ui-reindex-confirm_undated = { $count ->
    [one] У { $count } реплея для чтения нет читаемой даты боя, и он не входит в этот диапазон.
    [few] У { $count } реплеев для чтения нет читаемой даты боя, и они не входят в этот диапазон.
    [many] У { $count } реплеев для чтения нет читаемой даты боя, и они не входят в этот диапазон.
   *[other] У { $count } реплеев для чтения нет читаемой даты боя, и они не входят в этот диапазон.
}
ui-reindex-confirm_downloads = Сначала будут загружены игровые данные для этих версий:
ui-reindex-confirm_unobtainable = Для этих версий нельзя получить игровые данные, поэтому { $count ->
    [one] их { $count } реплей пропускается:
    [few] их { $count } реплея пропускаются:
    [many] их { $count } реплеев пропускаются:
   *[other] их { $count } реплея пропускаются:
}
ui-reindex-confirm_no_cache_dir = Не задан каталог кэша игровых данных, поэтому игровые данные для этих версий нельзя загрузить, и { $count ->
    [one] их { $count } реплей пропускается:
    [few] их { $count } реплея пропускаются:
    [many] их { $count } реплеев пропускаются:
   *[other] их { $count } реплея пропускаются:
}
ui-reindex-confirm_no_build = { $count ->
    [one] В { $count } реплее не указана сборка игры, он пропускается.
    [few] В { $count } реплеях не указана сборка игры, они пропускаются.
    [many] В { $count } реплеях не указана сборка игры, они пропускаются.
   *[other] В { $count } реплеях не указана сборка игры, они пропускаются.
}
ui-reindex-confirm_no_version = { $count ->
    [one] В { $count } реплее не указана читаемая версия игры, он пропускается.
    [few] В { $count } реплеях не указана читаемая версия игры, они пропускаются.
    [many] В { $count } реплеях не указана читаемая версия игры, они пропускаются.
   *[other] В { $count } реплеях не указана читаемая версия игры, они пропускаются.
}
ui-reindex-build_row = { $version } (сборка { $build }): { $count ->
    [one] { $count } реплей
    [few] { $count } реплея
    [many] { $count } реплеев
   *[other] { $count } реплея
}
ui-reindex-nothing_listed = В списке нет реплеев для переиндексации.
ui-reindex-nothing_readable = Ни один из этих реплеев нельзя переиндексировать: для них нет игровых данных (пропущено: { $count }).
ui-reindex-no_game_data = Реплеи нельзя прочитать, пока не указан каталог игры.
ui-reindex-busy_index = Индексация уже выполняется.
ui-reindex-busy_cache = Задача игровых данных уже выполняется. Повторите попытку после её завершения.
ui-reindex-planning = Проверка доступных игровых данных...
ui-reindex-no_index = Индекс реплеев не открыт, поэтому ничего не переиндексировано.
ui-reindex-done = Переиндексирован{ $indexed ->
    [one] { $indexed } реплей
    [few] о { $indexed } реплея
    [many] о { $indexed } реплеев
   *[other] о { $indexed } реплея
}; пропущено: { $skipped }, не удалось прочитать: { $failed }
ui-reindex-stopped = Переиндексация остановлена: переиндексировано { $indexed }, пропущено { $skipped }, не удалось прочитать { $failed }
ui-reindex-stopped_before_reading = Переиндексация остановлена до чтения какого-либо реплея.

## ui.index_folder
ui-index_folder-tooltip = Проиндексировать все реплеи из этого списка, которых ещё нет в индексе реплеев
ui-index_folder-confirm_title = Индексация реплеев
ui-index_folder-confirm_ok = Индексировать
ui-index_folder-confirm_count = { $count ->
    [one] { $count } реплей из этого списка ещё не в индексе реплеев и будет прочитан и добавлен в него.
    [few] { $count } реплея из этого списка ещё не в индексе реплеев и будут прочитаны и добавлены в него.
    [many] { $count } реплеев из этого списка ещё не в индексе реплеев и будут прочитаны и добавлены в него.
   *[other] { $count } реплея из этого списка ещё не в индексе реплеев и будут прочитаны и добавлены в него.
}
ui-index_folder-note_live = Проиндексированные реплеи добавляются в индекс реплеев, который используют Поиск и Отслеживание игроков.
ui-index_folder-note_folder = Проиндексированные реплеи добавляются в индекс реплеев под собственным источником этого каталога ({ $dir }). Поиск и список игроков в Отслеживании игроков их учитывают. Список кланов в Отслеживании игроков читает только реплеи самой игры.
ui-index_folder-nothing_listed = В этом списке нет реплеев для индексации.
ui-index_folder-nothing_missing = Все реплеи в этом списке уже проиндексированы.
ui-index_folder-unreadable_index = Не удалось прочитать индекс реплеев.
ui-index_folder-done = Проиндексирован{ $indexed ->
    [one] { $indexed } реплей
    [few] о { $indexed } реплея
    [many] о { $indexed } реплеев
   *[other] о { $indexed } реплея
}; пропущено: { $skipped }, не удалось прочитать: { $failed }
ui-index_folder-stopped = Индексация остановлена: проиндексировано { $indexed }, пропущено { $skipped }, не удалось прочитать { $failed }
ui-index_folder-stopped_before_reading = Индексация остановлена до чтения какого-либо реплея.

## ui.replay_folders
ui-replay_folders-install = Игра
ui-replay_folders-watched = Отслеживается: новые и изменённые реплеи появляются здесь
ui-replay_folders-unwatched = Не отслеживается: используйте «Обновить», чтобы перечитать
ui-replay_folders-unseen = { $count ->
    [one] { $count } новый или изменённый реплей с последнего просмотра
    [few] { $count } новых или изменённых реплея с последнего просмотра
    [many] { $count } новых или изменённых реплеев с последнего просмотра
   *[other] { $count } новых или изменённых реплея с последнего просмотра
}
ui-replay_folders-missing = Этот каталог больше не существует: { $dir }
ui-replay_folders-missing_hint = Он будет открываться здесь при каждом запуске, пока вы его не открепите.
ui-replay_folders-pin = Закрепить
ui-replay_folders-unpin = Открепить
ui-replay_folders-refresh = Обновить
ui-replay_folders-close = Закрыть
ui-replay_folders-pins_unreadable = Не удалось прочитать закреплённые каталоги реплеев.
ui-replay_folders-game_data_available = Для перечисленных реплеев доступны игровые данные.
ui-replay_folders-browse = Обзор...
ui-replay_folders-recent_title = Открыть каталог реплеев
ui-replay_folders-recent_placeholder = Фильтр недавних каталогов...
ui-replay_folders-recent_missing = Этот каталог больше не существует и удалён из списка недавних: { $dir }
ui-replay_folders-checking = Поиск { $dir }...
ui-replay_folders-indexing = Индексация { $dir }
ui-replay_folders-indexing_in_background = Этот каталог ещё индексируется
ui-replay_folders-search_while_indexing = Этот каталог ещё индексируется, поэтому результаты пока неполные. Повторите поиск после завершения.
ui-replay_folders-source_failed = Не удаётся проиндексировать реплеи этого каталога: { $error }
ui-replay_folders-restart_watch = Перезапустить отслеживание
ui-replay_folders-watch_stopped = Отслеживание { $folder } остановлено: { $cause }.
ui-replay_folders-watch_restarting = Перезапуск через { $seconds } с (попытка { $attempt } из { $attempts }).
ui-replay_folders-watch_gave_up = Перезапуск не удался { $attempts ->
    [one] { $attempts } раз
    [few] { $attempts } раза
    [many] { $attempts } раз
   *[other] { $attempts } раза
}, новые попытки не выполняются.
ui-replay_folders-watch_restart_hint = Нажмите на красную точку или выберите «Перезапустить отслеживание» в меню вкладки, чтобы перезапустить его сейчас.
ui-replay_folders-watch_resumed = Отслеживание { $folder } возобновлено.
ui-replay_folders-watch_cause_backend = файловая система сообщила об ошибке: { $error }
ui-replay_folders-watch_cause_gone = папка была перемещена или удалена
ui-replay_folders-watch_cause_panicked = внутренняя ошибка: { $error }
ui-replay_folders-watch_cause_exited = отслеживание неожиданно завершилось
ui-replay_folders-dropped = Перетащенные
ui-replay_folders-dropped_numbered = Перетащенные { $number }
ui-replay_folders-dropped_count = { $count ->
    [one] { $count } перетащенный реплей
    [few] { $count } перетащенных реплея
    [many] { $count } перетащенных реплеев
   *[other] { $count } перетащенных реплея
}
ui-replay_folders-dropped_no_source = Перетащенные реплеи индексируются вместе со своими каталогами. Откройте каталог, чтобы искать по нему или проиндексировать его.
ui-replay_folders-unreadable_replay = Не удалось прочитать: { $error }

## ui.models
ui-models-heading = Модели
ui-models-loading = Загрузка моделей...
ui-models-unavailable = Модели недоступны
ui-models-search = Поиск кораблей и моделей...
ui-models-open = Открыть
ui-models-export = Экспортировать модель
ui-models-copy_path = Копировать путь к модели
ui-models-path_copied = Скопировано: { $path }
ui-models-skin_replaces = Заменяет { $model }, когда установлен { $exterior }
ui-models-geometry_absent = Нет в файлах игры
ui-models-geometry_none = У этой модели нет геометрии
ui-models-hardpoints_tooltip = Показать точки крепления
ui-models-common = Общие модели

## ui.models.group
ui-models-group-gameplay = Игровые
ui-models-group-building = Здания
ui-models-group-character = Персонажи
ui-models-group-location = Локации
ui-models-group-port = Порты
ui-models-group-style = Стили
ui-models-group-other = Прочие модели

## ui.models.building
ui-models-building-weapon = Вооружение зданий
ui-models-building-unassigned = Неназначенные здания
ui-models-building-unspecified = Здания без типа

## ui.models.misc
ui-models-misc-crane = Краны
ui-models-misc-anchor = Якоря
ui-models-misc-deck_fitting = Палубное оборудование
ui-models-misc-instrument = Приборы
ui-models-misc-propeller = Гребные винты
ui-models-misc-searchlight = Прожекторы
ui-models-misc-boat = Шлюпки
ui-models-misc-flag = Флаги
ui-models-misc-insignia = Знаки различия
ui-models-misc-railing = Леера
ui-models-misc-hatch = Люки
ui-models-misc-ventilation = Вентиляция
ui-models-misc-antenna = Антенны
ui-models-misc-weapon = Вооружение
ui-models-misc-stowage = Хранение
ui-models-misc-other = Прочее

## ui.models.variant
ui-models-variant-dead = Обломки
ui-models-variant-skin = Облик

## ui.models.kind
ui-models-kind-ship = Корабли
ui-models-kind-gun = Орудия
ui-models-kind-director = Командно-дальномерные посты
ui-models-kind-finder = Дальномеры
ui-models-kind-radar = Радары
ui-models-kind-catapult = Катапульты
ui-models-kind-aircraft = Самолёты
ui-models-kind-projectile = Боеприпасы
ui-models-kind-misc = Разное
ui-models-kind-cap = Точки захвата
ui-models-kind-other = Прочее

## ui.models.sub_kind
ui-models-sub_kind-battleship = Линкоры
ui-models-sub_kind-cruiser = Крейсеры
ui-models-sub_kind-destroyer = Эсминцы
ui-models-sub_kind-aircraft_carrier = Авианосцы
ui-models-sub_kind-submarine = Подводные лодки
ui-models-sub_kind-auxiliary = Вспомогательные суда
ui-models-sub_kind-main_battery = Главный калибр
ui-models-sub_kind-secondary_battery = Вспомогательный калибр
ui-models-sub_kind-anti_aircraft = ПВО
ui-models-sub_kind-torpedo_tubes = Торпедные аппараты
ui-models-sub_kind-depth_charge_launcher = Бомбомёты
ui-models-sub_kind-missile_launcher = Ракетные установки
ui-models-sub_kind-search_radar = Обзорные радары
ui-models-sub_kind-fire_control_radar = Радары управления огнём
ui-models-sub_kind-fighter = Истребители
ui-models-sub_kind-bomber = Бомбардировщики
ui-models-sub_kind-dive_bomber = Пикирующие бомбардировщики
ui-models-sub_kind-scout = Разведчики
ui-models-sub_kind-torpedo = Торпеды
ui-models-sub_kind-bomb = Бомбы
ui-models-sub_kind-rocket = Ракеты
ui-models-sub_kind-depth_charge = Глубинные бомбы
ui-models-sub_kind-mine = Мины
ui-models-sub_kind-missile = Управляемые ракеты
ui-models-sub_kind-jato = Стартовые ускорители
ui-models-sub_kind-shell = Артиллерийские снаряды

## ui.models.part
ui-models-part-hull = Корпус
ui-models-part-main_battery = Главный калибр
ui-models-part-secondaries = Вспомогательный калибр
ui-models-part-anti_aircraft = ПВО
ui-models-part-torpedoes = Торпеды
ui-models-part-directors = КДП
ui-models-part-finders = Дальномеры
ui-models-part-radars = Радары
ui-models-part-depth_charges = Глубинные бомбы
ui-models-part-aircraft = Самолёты
ui-models-part-other = Прочее
ui-models-part-misc = Разное

## ui.model_view
ui-model_view-thickness_from_building = Толщина по { $building }
ui-model_view-thickness_from = Толщина по { $ship } ({ $hp })
ui-model_view-thickness_unknown = Толщина неизвестна: ни один корабль не несёт эту модель
ui-model_view-thickness_other_ships = Другие корабли
ui-model_view-export_title = Экспорт модели
ui-model_view-export_tooltip = Экспортировать модель в файл GLB
ui-model_view-part_model = Модель: { $model }
ui-model_view-part_hardpoint = Точка крепления: { $hardpoint }

## ui.model_view.part_group
ui-model_view-part_group-hull = Корпус
ui-model_view-part_group-main_battery = Главный калибр
ui-model_view-part_group-secondary_battery = Вспомогательный калибр
ui-model_view-part_group-aa_guns = ПВО
ui-model_view-part_group-torpedoes = Торпеды
ui-model_view-part_group-depth_charges = Глубинные бомбы
ui-model_view-part_group-fire_control = Управление огнём
ui-model_view-part_group-radar = Радары
ui-model_view-part_group-missiles = Ракетные установки
ui-model_view-part_group-decorations = Украшения
ui-model_view-part_group-misc = Разное
ui-model_view-part_group-armor = Броня
ui-model_view-part_group-other = Прочее

## ui.scene_problem
ui-scene_problem-cannot_show = Невозможно показать { $name }
ui-scene_problem-geometry_absent = Геометрии этой модели нет в файлах игры.
ui-scene_problem-model_unreadable = Не удалось прочитать данные этой модели. Подробности в журнале.
ui-scene_problem-ship_unreadable = Не удалось прочитать данные этого корабля. Подробности в журнале.
ui-scene_problem-no_meshes = У этой модели нет сеток.
ui-scene_problem-no_meshes_at_lod = У этой модели нет сеток на выбранном уровне детализации.
ui-scene_problem-no_visible_surface = У этой модели нет видимой поверхности для отрисовки.
ui-scene_problem-section_stand_in = У этой секции корпуса нет собственной геометрии. Корабль отрисовывает эту часть корпуса в другой секции.

## ui.armor.option_tabs
ui-armor-option_tabs-label = Вкладки параметров
ui-armor-option_tabs-ship = Корабль
ui-armor-option_tabs-layers = Слои
ui-armor-option_tabs-analysis = Анализ
ui-armor-option_tabs-configuration = Конфигурация
ui-armor-option_tabs-hull_parts = Части корпуса
ui-armor-option_tabs-tab_matches = { $tab }, совпадений: { $count }
ui-armor-option_tabs-empty = Ничто на этой вкладке не относится к показанной модели.
ui-armor-option_tabs-no_choices = У этой модели нет корпуса, модуля, камуфляжа или уровня детализации для выбора.

## ui.armor.hidden_ship
ui-armor-hidden_ship-clan_rental = Клановая аренда
ui-armor-hidden_ship-other_class = Не класс кораблей

## ui.armor.textures
ui-armor-textures-title = Текстуры
ui-armor-textures-hint = Игра читает файл из папки res_mods этой сборки вместо упакованного файла по тому же пути. Щёлкните текстуру правой кнопкой, чтобы скопировать этот путь.
ui-armor-textures-under_pointer = Под указателем: { $material }
ui-armor-textures-reading = Чтение текстур...
ui-armor-textures-summary = { $materials ->
    [one] { $materials } материал
    [few] { $materials } материала
    [many] { $materials } материалов
   *[other] { $materials } материала
}, { $textures ->
    [one] { $textures } текстура
    [few] { $textures } текстуры
    [many] { $textures } текстур
   *[other] { $textures } текстуры
}
ui-armor-textures-no_material = Нет материала
ui-armor-textures-unresolved = Путь отсутствует в assets.bin
ui-armor-textures-absent = Нет в установленных пакетах
ui-armor-textures-unreadable = Не удалось прочитать заголовок DDS
ui-armor-textures-unknown_format = Неизвестный формат
ui-armor-textures-details = { $format }, { $size }, { $count ->
    [one] { $count } файл
    [few] { $count } файла
    [many] { $count } файлов
   *[other] { $count } файла
}, { $bytes }
ui-armor-textures-role_albedo = Альбедо
ui-armor-textures-role_normal = Нормали
ui-armor-textures-role_metal_gloss = Металличность-глянец
ui-armor-textures-role_ambient_occlusion = Фоновое затенение
ui-armor-textures-role_detail = Детализация
ui-armor-textures-role_name_matched = Альбедо, найдено по имени файла
ui-armor-textures-copy_path = Копировать путь
ui-armor-textures-copy_overrides = { $count ->
    [1] Копировать путь res_mods
    [one] Копировать { $count } путь res_mods
    [few] Копировать { $count } пути res_mods
    [many] Копировать { $count } путей res_mods
   *[other] Копировать { $count } пути res_mods
}
ui-armor-textures-paths_copied = Скопирован{ $count ->
    [one] { $count } путь
    [few] о { $count } пути
    [many] о { $count } путей
   *[other] о { $count } пути
}
ui-armor-textures-no_build = Игровые данные не загружены, поэтому папка модов сборки неизвестна.
ui-armor-textures-show_in_unpacker = Показать в распаковщике ресурсов
ui-armor-textures-show_textures = Показать текстуры
ui-armor-textures-extract = Извлечь файлы...
ui-armor-textures-extract_title = Извлечь файлы текстур в
ui-armor-textures-extracting = Извлечение файлов текстур...
ui-armor-textures-extracted = Извлечен{ $count ->
    [one] { $count } файл
    [few] о { $count } файла
    [many] о { $count } файлов
   *[other] о { $count } файла
} в { $folder }
ui-armor-textures-extract_failed = Не удалось извлечь файлы текстур: { $reason }
ui-armor-textures-replace_title = Заменить существующие файлы?
ui-armor-textures-replace_description = { $count ->
    [one] { $count } из этих файлов уже есть в { $folder }. Извлечение заменит его.
    [few] { $count } из этих файлов уже есть в { $folder }. Извлечение заменит их.
    [many] { $count } из этих файлов уже есть в { $folder }. Извлечение заменит их.
   *[other] { $count } из этих файлов уже есть в { $folder }. Извлечение заменит их.
}
ui-armor-textures-replace = Заменить

## ui.settings.mcp
ui-settings-mcp-enabled = Включить сервер MCP
ui-settings-mcp-enabled_description = Предоставляет реплеи и игровые данные ИИ-ассистентам по MCP. Показывает переключатель в строке состояния.
ui-settings-mcp-start_on_launch = Запускать вместе с программой
ui-settings-mcp-start_on_launch_description = Запускает сервер при открытии WoWs Toolkit.
ui-settings-mcp-bind_address = Адрес прослушивания
ui-settings-mcp-bind_address_description = Петлевой IP-адрес, на котором слушает сервер, например 127.0.0.1 или ::1. Сервер доступен только с этого компьютера.
ui-settings-mcp-port = Порт
ui-settings-mcp-port_description = TCP-порт для прослушивания. Выбирается случайно при первом включении и сохраняется.
ui-settings-mcp-port_unset = Выбирается при первом включении
ui-settings-mcp-address_not_loopback = { $input } не является петлевым адресом. Сервер слушает только на этом компьютере, например на 127.0.0.1.
ui-settings-mcp-address_invalid = { $input } не является IP-адресом
ui-settings-mcp-port_invalid = { $input } не является портом от 1 до 65535
ui-settings-mcp-port_in_use = Этот порт занят другой программой. Выберите другой порт.

## ui.mcp
ui-mcp-started = Сервер MCP работает по адресу { $url }
ui-mcp-copy_url = Копировать URL
ui-mcp-start_failed = Не удалось запустить сервер MCP: { $reason }
ui-mcp-port_in_use = порт { $port } на { $address } занят другой программой
ui-mcp-no_watchdog = супервизор фоновых задач не запущен
ui-mcp-status_running = Сервер MCP работает по адресу { $url }. Нажмите, чтобы остановить.
ui-mcp-status_starting = Сервер MCP запускается на { $address }
ui-mcp-status_failed = Сервер MCP не запустился. Нажмите, чтобы повторить.
ui-mcp-status_stopped = Сервер MCP остановлен. Нажмите, чтобы запустить.

## ui.launcher.opt_in
ui-launcher-opt_in-setting = Включить лаунчер игры
ui-launcher-opt_in-description = Играйте в World of Warships из Toolkit в Linux, войдя в свою учётную запись Wargaming.
ui-launcher-opt_in-title = Включить лаунчер игры?
ui-launcher-opt_in-notice_account = Используя функцию лаунчера, вы признаёте, что этот сценарий не поддерживается разработчиком игры. Вы вводите свой пароль непосредственно в это приложение, которое, если вы ему не доверяете, может быть использовано для кражи вашей учётной записи (хотя разработчик приложения не желает этого делать).
ui-launcher-opt_in-notice_reports = Кроме того, вы признаёте, что при использовании этой функции не будете сообщать разработчику игры об ошибках графики/стабильности, если только вы действительно не уверены, что это кроссплатформенная проблема, которая затрагивает НЕ ТОЛЬКО Linux.
ui-launcher-opt_in-continue = Продолжить
ui-launcher-opt_in-cancel = Отмена

## ui.launcher.keyring
ui-launcher-keyring-unencrypted = Ваш вход сохраняется без шифрования в папке данных приложения, потому что нет ни одного поставщика Secret Service (например, GNOME Keyring, KeePassXC или KDE Wallet).
ui-launcher-keyring-how = Как сохранить шифрование
ui-launcher-keyring-how_detail = Установите и запустите поставщика Secret Service, например GNOME Keyring, KeePassXC или KDE Wallet, затем перезапустите Toolkit. Сохранённые входы будут перенесены в него.
ui-launcher-keyring-unlock_hint = Системная связка ключей может попросить разблокировку, чтобы Toolkit мог прочитать сохранённый вход.

## ui.launcher.play
ui-launcher-play-play = Играть
ui-launcher-play-play_through_steam = Играть через Steam
ui-launcher-play-set_up_game = Настроить игру
ui-launcher-play-set_up_runner = Настроить Proton
ui-launcher-play-sign_in_again = Войти снова
ui-launcher-play-update = Обновить { $size }
ui-launcher-play-updating = Обновление { $percent }%
ui-launcher-play-updating_unknown = Обновление
ui-launcher-play-paused = Приостановлено
ui-launcher-play-starting = Запуск...
ui-launcher-play-running = Запущено
ui-launcher-play-in_battle = В бою
ui-launcher-play-menu = Учётные записи и параметры игры

## ui.launcher.popover
ui-launcher-popover-game = World of Warships
ui-launcher-popover-no_install = Установка игры не задана.
ui-launcher-popover-update_available = Доступно обновление
ui-launcher-popover-check_failed = Не удалось проверить обновления
ui-launcher-popover-accounts = Учётные записи
ui-launcher-popover-no_accounts = Учётных записей пока нет. Добавьте учётную запись, чтобы играть с входом.
ui-launcher-popover-close_to_switch = Закройте игру, чтобы сменить учётную запись.
ui-launcher-popover-default = По умолчанию
ui-launcher-popover-ready = Готово
ui-launcher-popover-restoring = Проверка...
ui-launcher-popover-not_saved = Не сохранено
ui-launcher-popover-unencrypted = Сохранено без шифрования
ui-launcher-popover-keyring_locked = Связка ключей заблокирована: разблокируйте её, чтобы восстановить сохранённые входы
ui-launcher-popover-forget = Удалить
ui-launcher-popover-sign_out = Выйти
ui-launcher-popover-expires_in = Истекает через { $duration }
ui-launcher-popover-expired = Сеанс истёк
ui-launcher-popover-expiry_unknown = Срок неизвестен
ui-launcher-popover-make_default = Сделать основной
ui-launcher-popover-account_actions = Действия с учётной записью
ui-launcher-popover-play_anyway = Всё равно играть
ui-launcher-popover-play_safe_mode = Играть в безопасном режиме
ui-launcher-popover-add_account = Добавить учётную запись...
ui-launcher-popover-game_settings = Настройки игры...
ui-launcher-popover-installs = Установки и настройка...
ui-launcher-popover-open_replay = Открыть реплей в игре...

## ui.launcher.palette
ui-launcher-palette-play = Играть
ui-launcher-palette-play_as = Играть как { $nickname } ({ $realm })
ui-launcher-palette-game_settings = Настройки игры
ui-launcher-palette-add_account = Добавить учётную запись
ui-launcher-palette-update = Обновить World of Warships
ui-launcher-palette-open_replay = Открыть реплей в игре

## ui.launcher.installs
ui-launcher-installs-toolkit = Каталог игры Toolkit
ui-launcher-installs-played = Используется учётной записью по умолчанию
ui-launcher-installs-play_from = Играть отсюда
ui-launcher-installs-update = Обновить

## ui.launcher.wgc_login
ui-launcher-wgc_login-label = Использовать вход Wargaming
ui-launcher-wgc_login-about = Запускает эту установку Steam через вход Toolkit вместо Steam. Настройки и реплеи хранятся отдельно от запусков через Steam. Кошелёк Steam, достижения и время в игре в этом режиме не работают.
ui-launcher-wgc_login-steam_accounts = Учётным записям, созданным через Steam, нужны адрес эл. почты и пароль. Сначала добавьте их в управлении учётной записью Wargaming.
ui-launcher-wgc_login-steam_accounts_link = Как добавить эл. почту и пароль
ui-launcher-wgc_login-ready = Файлы Game Center готовы
ui-launcher-wgc_login-fetchable = При включении будут загружены файлы Game Center от WG для этой версии ({ $size }).
ui-launcher-wgc_login-downloading = Загрузка файлов Game Center: { $percent }%
ui-launcher-wgc_login-downloading_unknown = Загрузка файлов Game Center
ui-launcher-wgc_login-checking = Поиск файлов Game Center для этой версии...
ui-launcher-wgc_login-needs_sign_in = Сначала добавьте учётную запись Wargaming в Toolkit.
ui-launcher-wgc_login-unavailable = Недоступно для этой версии: { $error }

## ui.launcher.replay
ui-launcher-replay-no_install = Нет установки с версией игры { $build }. Сохраните или установите копию этой версии, чтобы посмотреть этот реплей.

## ui.launcher.setup
ui-launcher-setup-title = Настройка World of Warships
ui-launcher-setup-game = Игра
ui-launcher-setup-proton = Proton
ui-launcher-setup-accounts = Учётные записи
ui-launcher-setup-replays = Реплеи
ui-launcher-setup-game_question = Какую копию игры должен использовать Toolkit?
ui-launcher-setup-none_found = На этом компьютере установка не найдена.
ui-launcher-setup-choose_folder = Выбрать папку...
ui-launcher-setup-install_new = Установить новую копию...
ui-launcher-setup-proton_explain = В Linux игра запускается через Proton: из Steam, если Steam установлен, иначе через umu-run.
ui-launcher-setup-proton_ready = Proton найден.
ui-launcher-setup-accounts_explain = Войдите в учётные записи Wargaming, за которые вы играете. Кнопка Играть входит в игру с учётной записью по умолчанию.
ui-launcher-setup-replays_explain = Когда игра хранит столько реплеев, она удаляет самые старые.
ui-launcher-setup-skip = Пропустить настройку
ui-launcher-setup-back = Назад
ui-launcher-setup-next = Далее
ui-launcher-setup-done = Готово

## ui.launcher.game_settings
ui-launcher-game_settings-title = Настройки игры
ui-launcher-game_settings-build = версия { $build }
ui-launcher-game_settings-display = Экран
ui-launcher-game_settings-graphics = Графика
ui-launcher-game_settings-sound = Звук
ui-launcher-game_settings-replays = Реплеи
ui-launcher-game_settings-locked = Игра запущена. При выходе она перезаписывает эти файлы, поэтому до этого они доступны только для чтения.
ui-launcher-game_settings-footer = Записывается в файлы настроек игры. Сохраняется резервная копия.
ui-launcher-game_settings-cancel = Отмена
ui-launcher-game_settings-save = Сохранить
ui-launcher-game_settings-saved = Настройки игры сохранены.
ui-launcher-game_settings-no_install = Установка игры не задана.
ui-launcher-game_settings-window_mode = Режим окна
ui-launcher-game_settings-windowed = В окне
ui-launcher-game_settings-borderless = Без рамки
ui-launcher-game_settings-fullscreen = Полноэкранный
ui-launcher-game_settings-monitor = Монитор
ui-launcher-game_settings-monitor_name = Имя монитора
ui-launcher-game_settings-resolution = Полноэкранное разрешение
ui-launcher-game_settings-width = Ширина
ui-launcher-game_settings-height = Высота
ui-launcher-game_settings-refresh = Частота обновления
ui-launcher-game_settings-display_rate = Частота отображения
ui-launcher-game_settings-vsync = Вертикальная синхронизация
ui-launcher-game_settings-ui_scale = Масштаб интерфейса
ui-launcher-game_settings-battle_fps = Ограничение частоты кадров в бою
ui-launcher-game_settings-port_fps = Ограничение частоты кадров в порту
ui-launcher-game_settings-unlimited = Без ограничений
ui-launcher-game_settings-skip_intro = Пропускать вступительное видео
ui-launcher-game_settings-master = Общая громкость
ui-launcher-game_settings-music_port = Музыка в порту
ui-launcher-game_settings-music_battle = Музыка в бою
ui-launcher-game_settings-preset = Предустановка
ui-launcher-game_settings-antialiasing = Сглаживание
ui-launcher-game_settings-textures = Качество текстур
ui-launcher-game_settings-filtering = Фильтрация текстур
ui-launcher-game_settings-shadows = Качество теней
ui-launcher-game_settings-graphics_note = Изменяйте графику в игре. Игра сверяет её с вашим оборудованием.
ui-launcher-game_settings-replays_kept = Сколько реплеев хранить
ui-launcher-game_settings-replays_kept_zero = Храните хотя бы один реплей.
ui-launcher-game_settings-versioned = Сортировать реплеи по версии игры
ui-launcher-game_settings-replays_note = Записывается в res_mods этой версии. Безопасный режим его не использует.
ui-launcher-game_settings-absent = Не задано в файлах игры
ui-launcher-game_settings-unrecognized = Неизвестное значение: { $value }
ui-launcher-game_settings-not_a_number = { $field }: введите число.

## ui.settings.launcher
ui-settings-launcher-installs = Установки игры
ui-settings-launcher-installs_description = Установки игры, которые лаунчер запускает и обновляет.
ui-settings-launcher-runner = Proton (Linux)
ui-settings-launcher-runner_description = Как игра запускается в Linux. Proton Experimental или новейший Proton из Steam находится автоматически, иначе используется umu-run.
ui-settings-launcher-launch_args = Аргументы запуска
ui-settings-launcher-launch_args_description = Дополнительные аргументы командной строки для игры, по одному в строке.
ui-settings-launcher-environment = Окружение
ui-settings-launcher-environment_description = Переменные окружения для игры, KEY=VALUE, по одной в строке.
ui-settings-launcher-safe_mode_args = Аргументы безопасного режима
ui-settings-launcher-safe_mode_args_description = Аргументы, добавляемые при запуске игры в безопасном режиме, по одному в строке.
