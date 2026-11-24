# You are encouraged to suggest edits to this file on GitHub by either filing
# an issue, editing the file directly and creating a pull request, or just
# mentioning it in the Discord.
## meta
meta-language_name = Português (Brasil)

## dialog
dialog-continue_in_english = Continuar em inglês
dialog-continue_in_language = Continuar em português
dialog-select_language = Selecionar idioma
dialog-machine_translation_warning = A maioria das traduções foi gerada por máquina e pode conter imprecisões.

## ui.tabs
ui-tabs-unpacker = Desempacotador de recursos
ui-tabs-settings = Configurações
ui-tabs-replay_parser = Inspetor de replays
ui-tabs-player_tracker = Rastreador de jogadores
ui-tabs-armor_viewer = Visualizador de blindagem
ui-tabs-model_viewer = Visualizador de modelos
ui-tabs-stats = Estatísticas
ui-tabs-directory_stats = Estatísticas de { $dir }
ui-tabs-close_directory_stats = Fechar estatísticas do diretório
ui-tabs-search = Pesquisa
ui-tabs-search_these_replays_unavailable = Estes replays ainda não foram indexados
ui-tabs-zoom_in_panel = Ampliar
ui-tabs-zoom_out_panel = Reduzir
ui-tabs-close_panel = Fechar
ui-tabs-close_tab = Fechar
ui-tabs-close_other_tabs = Fechar outras
ui-tabs-close_tabs_to_the_right = Fechar à direita

## ui.buttons
ui-buttons-yes = Sim
ui-buttons-no = Não
ui-buttons-send_replays = Enviar replays
ui-buttons-send_build_data = Enviar apenas dados de build
ui-buttons-send_nothing = Não enviar nada
ui-buttons-switch_to_replays = Mudar para replays
ui-buttons-keep_current = Manter configuração atual
ui-buttons-stop = Parar
ui-buttons-copy = Copiar
ui-buttons-cancel = Cancelar
ui-buttons-continue_ = Continuar
ui-buttons-install_update = Instalar atualização
ui-buttons-view_release = Ver lançamento
ui-buttons-clear_settings = Limpar configurações
ui-buttons-more_options = Mais opções
ui-buttons-create_issue = Criar issue
ui-buttons-discord = Discord
ui-buttons-github = GitHub
ui-buttons-view_github = Ver projeto no GitHub
ui-buttons-choose = Escolher...
ui-buttons-reset = Redefinir
ui-buttons-select_all = Selecionar tudo
ui-buttons-download = Baixar
ui-buttons-retry = Tentar novamente
ui-buttons-refresh_persisted_data_confirm = Atualizar
ui-buttons-clear = Limpar

## ui.menu
ui-menu-file = Arquivo
ui-menu-check_updates = Verificar atualizações
ui-menu-about = Sobre
ui-menu-quit = Sair

## ui.windows
ui-windows-build_consent = Consentimento de coleta de builds
ui-windows-replay_migration = Mudar para compartilhamento de replays
ui-windows-crash_detected = Falha do aplicativo detectada
ui-windows-update_available = Atualização disponível
ui-windows-network_warning = Aviso de rede
ui-windows-download_game_data = Baixar dados do jogo
ui-windows-error = Erro
ui-windows-software_encode = Sem codificador de vídeo por GPU
ui-windows-about = Sobre
ui-windows-refresh_persisted_data = Atualizar dados persistidos dos replays
ui-windows-tactics_board = Quadro tático
ui-windows-minimize = Minimizar
ui-windows-maximize = Maximizar
ui-windows-restore = Restaurar
ui-windows-close = Fechar

## ui.dialogs
ui-dialogs-build_consent_message = Você gostaria de enviar informações de builds de jogadores de batalhas ranqueadas e aleatórias para o desenvolvedor? Essa coleta de dados ajuda a comunidade a analisar builds de jogadores em massa. Você pode cancelar a qualquer momento nas configurações.
ui-dialogs-replay_migration_message = O Toolkit agora pode compartilhar replays completos, o que permite uma análise de matchmaking muito mais rica do que apenas os dados de build. Batalhas em que você joga com um navio de teste não são compartilhadas. Deseja mudar para o compartilhamento de replays brutos?
ui-dialogs-crash_message = Parece que o WoWs Toolkit travou na última vez que foi executado. Se você deseja relatar esse problema, publique uma issue no GitHub ou entre no servidor do Discord e forneça as informações abaixo.
ui-dialogs-crash_clear_settings = Se por algum motivo os dados persistidos pelo aplicativo puderem ser responsáveis, você pode tentar limpar as configurações pressionando o botão abaixo. Isso limpará todas as configurações. Os jogadores rastreados são mantidos. Seus replays e dados do WoWs estarão seguros.
ui-dialogs-update_message = A versão { $tag } do WoWs Toolkit está disponível
ui-dialogs-update_notes = Notas da versão
ui-dialogs-update_asset_pending = O download para Windows ainda não está disponível. Veja a versão para o status da compilação.
ui-dialogs-update_windows_only = Atualização disponível, mas apenas o Windows é suportado no momento.
ui-dialogs-p2p_warning = Sessões colaborativas usam rede P2P. Outros usuários na sessão podem ver seu endereço IP.
ui-dialogs-software_encode_message = Nenhum codificador de vídeo por GPU compatível foi encontrado, então esta exportação será codificada por software. Isso é muito mais lento, e uma batalha longa pode levar vários minutos.
ui-dialogs-download_game_data_intro = Alguns replays precisam de dados do jogo que não estão instalados. Escolha as builds a baixar.
ui-dialogs-download_build_row = Versão { $version } (build { $build })
ui-dialogs-download_replays_needing = necessária para { $count ->
    [one] { $count } replay
   *[other] { $count } replays
}
ui-dialogs-download_availability_exact = publicada
ui-dialogs-download_availability_nearest = sem correspondência exata, a mais próxima é { $version } (build { $build })
ui-dialogs-download_availability_unpublished = nunca publicada
ui-dialogs-download_availability_unreachable = publicada, mas não foi possível ler sua lista de arquivos
ui-dialogs-download_plan_pending = Calculando o que precisa ser baixado...
ui-dialogs-download_objects_to_fetch = Objetos a baixar para as builds marcadas: { $count }
ui-dialogs-download_plan_failed = Não foi possível calcular o que precisa ser baixado
ui-dialogs-download_plan_no_cache_dir = Nenhum diretório de cache de dados do jogo foi definido. Escolha um nas Configurações e tente novamente.
ui-dialogs-refresh_persisted_data_message = Reanalisa todos os replays do seu diretório de replays e regrava os dados armazenados. Entradas gravadas com um mapeamento de dados de replay que não corresponde à build do jogo são mantidas como estão, exceto que o Rating pessoal e o modo de jogo são recalculados a partir dos valores esperados atuais para todos os replays. Isso pode demorar.
ui-dialogs-file_dialog_failed = Não foi possível abrir a caixa de diálogo de arquivos: { $reason }

## ui.labels
ui-labels-error_occurred = Ocorreu um erro:
ui-labels-made_by = Feito por landaire.
ui-labels-credits = Agradecimentos a Trackpad, TTaro, lkolbly por suas contribuições.
ui-labels-pr_credits = Dados e fórmula de cálculo do rating pessoal (PR) fornecidos por WoWs Numbers.
ui-labels-suppress_warning = Não mostrar novamente
ui-labels-more_info = Mais informações
ui-labels-app_version = Versão { $version }
ui-labels-project_license = O WoWs Toolkit é distribuído sob a licença { $license }.
ui-labels-third_party_notices = Avisos de terceiros
ui-labels-notices_search_hint = Pesquisar avisos
ui-labels-no_matching_notices = Nenhum aviso corresponde à pesquisa.

## ui.messages
ui-messages-bug_report_info_copied = Informações para o relatório de bug copiadas.
ui-messages-update_downloading = Baixando a atualização: { $read } de { $total }
ui-messages-update_downloading_unknown = Baixando a atualização: { $read } até agora
ui-messages-app_up_to_date = Aplicativo atualizado
ui-messages-checking_app_updates = Verificando atualizações do aplicativo...
ui-messages-update_starting = Baixando a atualização...
ui-messages-update_check_failed = Falha ao verificar atualizações do aplicativo
ui-messages-game_data_loaded = Dados do jogo carregados com sucesso
ui-messages-no_replays_detected = Nenhum replay detectado. Seu diretório do WoWs está configurado corretamente?
ui-messages-game_data_builds_downloaded = Dados do jogo baixados para { $count ->
    [one] { $count } build
   *[other] { $count } builds
}
ui-messages-game_data_download_failed = Não foi possível baixar os dados do jogo
ui-messages-game_data_up_to_date = Os dados do jogo estão atualizados
ui-messages-game_data_updates_available = { $count ->
    [one] { $count } build em cache tem dados mais recentes disponíveis
   *[other] { $count } builds em cache têm dados mais recentes disponíveis
}
ui-messages-game_data_cache_valid = O cache de dados do jogo é válido e corresponde ao repositório remoto
ui-messages-game_data_cache_invalid = { $count ->
    [one] { $count } build em cache está desatualizada ou corrompida
   *[other] { $count } builds em cache estão desatualizadas ou corrompidas
}
ui-messages-replay_load_failed = Falha ao carregar o replay
ui-messages-twitch_token_invalid = O token do Twitch é inválido ou expirou. Atualize-o nas Configurações.
ui-messages-wows_dir_invalid = O diretório do World of Warships não é válido. Verifique o caminho nas Configurações.
ui-messages-wows_dir_not_set = O diretório do World of Warships não está definido
ui-messages-constants_version_mismatch =
    A versão do arquivo de mapeamento de dados de replay não corresponde à versão do jogo.
    Os resultados pós-batalha podem não ser precisos. Aguarde enquanto os mantenedores do projeto atualizam o mapeamento no servidor.
ui-messages-session_started = Sessão iniciada
ui-messages-user_joined = { $name } entrou
ui-messages-user_timeout = { $name } expirou por inatividade
ui-messages-user_left = { $name } saiu
ui-messages-session_ended = Sessão encerrada
ui-messages-session_error = Erro de sessão: { $msg }
ui-messages-connected_to_session = Conectado à sessão
ui-messages-host_opened_replay = O anfitrião abriu um replay: { $name }
ui-messages-host_closed_replay = O anfitrião fechou um replay
ui-messages-session_rejected = Sessão rejeitada: { $reason }
ui-messages-drop_not_a_replay = Isso não é um arquivo de replay
ui-messages-drop_not_replays = Esses não são arquivos de replay
ui-messages-drop_skipped = { $count ->
    [one] { $count } arquivo solto foi ignorado por não ser um replay nem uma pasta
   *[other] { $count } arquivos soltos foram ignorados por não serem replays nem pastas
}
ui-messages-log_copied = { $bytes } KB de { $file } copiados para a área de transferência
ui-messages-log_copy_failed = Não foi possível ler um arquivo de log: { $error }
ui-messages-log_not_found = Nenhum arquivo de log encontrado em { $dir }

## stat
stat-damage = Dano
stat-spotting_damage = Dano de avistamento
stat-frags = Abates
stat-raw_xp = XP bruto
stat-base_xp = XP base
stat-win_rate = Taxa de vitória
stat-personal_rating = Rating pessoal
stat-match_contribution = Contribuição na batalha
stat-capture_points = Pontos de captura
stat-planes_shot_down = Aviões abatidos
stat-anti_air_damage = Dano antiaéreo
stat-survival_rate = Taxa de sobrevivência
stat-torpedoes_spotted = Torpedos detectados
stat-avg_pr = PR médio
stat-avg_prefix = Méd. { $name }

## chart
chart-combined = Combinado
chart-combined_suffix = (Combinado)
chart-rolling_average_suffix = (Média móvel)

## confirm
confirm-open_in_game = Isso iniciará o World of Warships. Continuar?
confirm-clear_all_session_stats = Isso limpará todas as estatísticas de sessão. Continuar?
confirm-set_as_session_stats = Isso substituirá suas estatísticas de sessão atuais. Continuar?

## error
error-file_watcher_creation = Falha ao criar observador de arquivos: { $error }
error-replay_dir_watch = Falha ao monitorar diretório de replays: { $error }

## ui.settings.app
ui-settings-app-check_for_updates = Verificar atualizações ao iniciar
ui-settings-app-enable_logging = Ativar registro de logs (entra em vigor ao reiniciar)
ui-settings-app-enable_logging_tooltip = Gravar logs de depuração em um arquivo no diretório de dados do aplicativo. Útil para solução de problemas.
ui-settings-app-data_sharing_mode = Compartilhamento de dados (batalhas ranqueadas e aleatórias)
ui-settings-app-theme = Tema
ui-settings-app-theme_system = Sistema
ui-settings-app-theme_dark = Escuro
ui-settings-app-theme_light = Claro
ui-settings-app-code_integrity = Bloquear código não assinado (entra em vigor ao reiniciar)
ui-settings-app-code_integrity_tooltip = Recusa carregar código que não tenha sido assinado pela Microsoft, o que mantém overlays, ferramentas de captura e utilitários de áudio fora deste aplicativo. Eles são uma causa comum de falhas e travamentos durante a renderização. Os drivers de vídeo são assinados pelo Windows e continuam funcionando. Isso não pode ser desativado depois que a sessão começa, portanto a alteração só vale na próxima inicialização. A opção Automático ignora o bloqueio quando você tem um método de entrada de terceiros instalado, que de outra forma seria bloqueado. Observe que as demais proteções continuam ativas independentemente desta configuração; execute com --no-hardening para desativar todas elas.
ui-settings-app-code_integrity_automatic = Automático
ui-settings-app-code_integrity_always = Sempre
ui-settings-app-code_integrity_never = Nunca
ui-settings-app-language = Idioma
ui-settings-app-open_data_dir_tooltip = Abrir a pasta de dados do aplicativo (configurações, logs, dados em cache) no seu gerenciador de arquivos.
ui-settings-app-proxy_url = Proxy HTTP
ui-settings-app-proxy_url_hint_live = host:porta, ou deixe vazio para usar as configurações de proxy do sistema

## ui.settings.wows
ui-settings-wows-description = Caminho para sua instalação do World of Warships
ui-settings-wows-directory_hint = Diretório do World of Warships
ui-settings-wows-browse = Procurar...

## ui.settings.wows.cache
ui-settings-wows-cache-heading = Cache de dados do jogo
ui-settings-wows-cache-description = Dados do jogo extraídos, para que replays de builds antigas ainda abram
ui-settings-wows-cache-auto_dump = Guardar dados do jogo para compatibilidade com replays antigos
ui-settings-wows-cache-browse = Procurar
ui-settings-wows-cache-open_folder = Abrir pasta
ui-settings-wows-cache-delete_old = Eliminar versões antigas
ui-settings-wows-cache-stats = Cache de dados: { $size } em { $count ->
    [one] { $count } versão
   *[other] { $count } versões
}
ui-settings-wows-cache-check_updates = Verificar atualizações
ui-settings-wows-cache-actions = Ações de cache
ui-settings-wows-cache-actions_tooltip = Verificar atualizações ou validar builds em cache
ui-settings-wows-cache-update_all = Atualizar tudo
ui-settings-wows-cache-updates_available = { $count ->
    [one] { $count } build em cache tem dados mais recentes disponíveis
   *[other] { $count } builds em cache têm dados mais recentes disponíveis
}
ui-settings-wows-cache-validate = Validar cache
ui-settings-wows-cache-repair = Reparar
ui-settings-wows-cache-repair_needed = { $count ->
    [one] { $count } build em cache está desatualizada ou corrompida
   *[other] { $count } builds em cache estão desatualizadas ou corrompidas
}

## ui.settings.replay
ui-settings-replay-show_raw_xp = Mostrar XP bruto
ui-settings-replay-show_entity_id = Mostrar ID de entidade
ui-settings-replay-show_observed_damage = Mostrar dano observado
ui-settings-replay-enable_previews = Mostrar prévia do minimapa ao passar o mouse
ui-settings-replay-enable_previews_tooltip = Passar o mouse sobre um replay faz com que ele seja lido e reproduzido em segundo plano para montar uma prévia do minimapa em loop. Desative isso em armazenamento lento.
ui-settings-replay-auto_export_data = Exportação automática de dados
ui-settings-replay-export_path_hint = Diretório de exportação automática de dados
ui-settings-replay-export_path_missing = O diretório não existe. Não é possível exportar dados para ele.
ui-settings-replay-index_all_replays = Indexar todos os replays
ui-settings-replay-show_heals = Mostrar reparos
ui-settings-replay-current_path = Caminho atual dos replays

## ui.settings.session
ui-settings-session-display_name = Nome de exibição
ui-settings-session-suppress_ip_warning = Não mostrar aviso de endereço IP P2P
ui-settings-session-ip_warning_tooltip = Sessões colaborativas usam rede P2P. Outros usuários na sessão podem ver seu endereço IP.
ui-settings-session-disable_auto_open = Desativar abertura automática de janelas de sessão
ui-settings-session-auto_open_tooltip = Quando ativado, janelas de sessão (replays, quadros táticos) não abrirão automaticamente. Você ainda pode abri-las no menu da sessão.

## ui.settings.twitch
ui-settings-twitch-get_token = Obter token de login
ui-settings-twitch-get_token_tooltip = Usamos a página de login do Chatterino pois ela fornece um token com as permissões necessárias (basicamente um token de moderador com permissões de chat), eliminando a necessidade do desenvolvedor do WoWs Toolkit hospedar sua própria página de login que teria o mesmo resultado.
ui-settings-twitch-paste_token_no_token = Colar token (nenhum token definido)
ui-settings-twitch-paste_token_tooltip = Lê a credencial da área de transferência
ui-settings-twitch-paste_token_valid = Colar token (token válido)
ui-settings-twitch-paste_token_unvalidated = Verificando token
ui-settings-twitch-paste_token_invalid = Colar token (token inválido / expirado)
ui-settings-twitch-monitored_channel = Canal monitorado (padrão: próprio)
ui-settings-twitch-signed_in_as = Conectado como { $who }

## ui.replay
ui-replay-copy_bug_report_info = Copiar informações para o relatório de bug
ui-replay-match_age = há { $age }
ui-replay-listing_caption = Replays
ui-replay-loading_game_data_version = Carregando dados do jogo para { $version }...
ui-replay-spectator = Espectador
ui-replay-loading = Carregando...
ui-replay-preview_loading = Montando prévia...
ui-replay-preview_no_game_data = Sem dados do jogo para a versão de cliente { $value }
ui-replay-preview_unreadable_because = Não foi possível ler este replay: { $value }
ui-replay-preview_unwalkable = Não foi possível percorrer a batalha deste replay: { $value }
ui-replay-preview_unknown_version = Este replay informa uma versão de cliente ilegível: { $value }
ui-replay-preview_no_map = Sem dados de mapa para { $value }
ui-replay-preview_render_failed = Não foi possível desenhar a prévia: { $value }
ui-replay-incomplete_results = Resultados de partida incompletos
ui-replay-incomplete_results_tooltip = O replay ainda não possui resultados de fim de partida. Os dados serão recarregados automaticamente quando a partida terminar e os resultados forem adicionados ao replay.
ui-replay-no_chat = Nenhuma mensagem de chat foi enviada neste replay
ui-replay-chat_copy_all = Copiar tudo
ui-replay-chat_save_to_file = Salvar em arquivo
ui-replay-chat_save_title = Salvar chat da partida
ui-replay-export_results_title = Exportar resultados
ui-replay-results_exported = Resultados exportados
ui-replay-chat_saved = Chat salvo em { $path }
ui-replay-chat_save_failed = O chat não foi salvo: { $error }
ui-replay-team_damage = Dano ao time:{" "}
ui-replay-team_share = { $pct } do total do time ({ $team })
ui-replay-hide_my_stats = Ocultar minhas estatísticas de navios de teste
ui-replay-open_manually = Abrir arquivo de replay manualmente...
ui-replay-open_directory = Abrir diretório de replays...
ui-replay-search_these_replays = Pesquisar estes replays
ui-replay-import_constants = Importar arquivo constants.json...
ui-replay-refresh_persisted_data = Atualizar dados persistidos dos replays...
ui-replay-contributing = Contribuindo { $done } de { $total ->
    [one] { $total } replay
   *[other] { $total } replays
}
ui-replay-contributed = { $sent } de { $total ->
    [one] { $total } replay contribuído
   *[other] { $total } replays contribuídos
}
ui-replay-contribute_nothing_shared = Escolha o que compartilhar nas Configurações primeiro
ui-replay-contribute_none_listed = Nenhum replay listado para contribuir
ui-replay-contribute_not_ready = Os replays podem ser contribuídos quando os dados do jogo e as configurações forem carregados
ui-replay-contribute_already_running = Uma contribuição já está em andamento
ui-replay-auto_export_failed = Não foi possível gravar uma batalha: { $error }
ui-replay-auto_exported = A batalha foi exportada para { $file }
ui-replay-opened_new_battle = Aberta a nova batalha: { $battle }
ui-replay-new_battle_listed = Nova batalha: { $battle }
ui-replay-results_arrived = Os resultados da batalha { $battle } chegaram
ui-replay-constants_written = { $count ->
    [one] O mapeamento de resultados de { $count } build foi baixado
   *[other] Os mapeamentos de resultados de { $count } builds foram baixados
}
ui-replay-constants_import_title = Importar um constants.json
ui-replay-constants_imported = O mapeamento de resultados da build { $build } foi importado
ui-replay-constants_import_failed = Esse mapeamento não foi importado: { $reason }
ui-replay-constants_no_build = Carregue uma build do jogo primeiro
ui-replay-constants_latest_written = O mapeamento de resultados mais recente foi baixado
ui-replay-constants_check_failed = Não foi possível verificar os mapeamentos de resultados: { $reason }
ui-replay-constants_build_label = build { $build }
ui-replay-constants_generating = Atualizando as constantes do jogo para { $version }...
ui-replay-constants_generated = As constantes do jogo para { $version } foram geradas em { $seconds } s
ui-replay-constants_generation_failed = Não foi possível atualizar as constantes do jogo para { $version }: { $reason }. { $fallback }
ui-replay-constants_fallback_kept = Mantendo as constantes já salvas para ela.
ui-replay-constants_fallback_published = Usando as constantes publicadas para a build { $build }.
ui-replay-constants_fallback_embedded = Usando as constantes integradas.
ui-replay-copy_latest_log = Copiar último log para a área de transferência
ui-replay-load_alt_perspective = Carregar perspectiva do outro time...
ui-replay-load_alt_perspective_failed = Falha ao mesclar o replay: { $error }
ui-replay-load_alt_perspective_limit = Esta batalha já está sendo lida a partir de { $limit ->
    [one] { $limit } outra gravação
   *[other] { $limit } outras gravações
}
ui-replay-consumable_charges_remaining = { $remaining } restantes de { $total ->
    [one] { $total } carga
   *[other] { $total } cargas
}
ui-replay-consumable_charges_unlimited = { $used } usadas (cargas ilimitadas)
ui-replay-consumable_header_remaining = Restantes
ui-replay-consumable_header_total = Total
ui-replay-consumable_charges_infinite = ∞
ui-replay-autoload_latest = Carregar automaticamente o replay mais recente
ui-replay-autoload_latest_tooltip = Abrir uma partida assim que o jogo terminar de gravá-la
ui-replay-anonymize_names = Anonimizar nomes de jogadores
ui-replay-anonymize_names_tooltip = Mostrar nomes gerados em vez dos nomes dos jogadores e das tags de clã. As exportações mantêm os nomes reais.
ui-replay-group_win_rate_all_modes = Contar todos os modos de jogo na taxa de vitórias dos grupos
ui-replay-group_win_rate_all_modes_tooltip = Por padrão, os cabeçalhos de grupo contam apenas batalhas aleatórias e ranqueadas. Quando ativado, todos os modos de jogo contam e a taxa de vitórias não é colorida, pois as cores são calibradas para batalhas aleatórias.
ui-replay-select_replay = Selecione um replay
ui-replay-drop_hint = Solte um replay aqui para abri-lo, ou uma pasta para listar seus replays
ui-replay-scanning = Verificando replays...
ui-replay-scanning_count = Lendo replay { $done } de { $total }
ui-replay-no_replays_in = Nenhum replay em { $folder }
ui-replay-drop_folder_hint = Solte uma pasta na janela para listá-la em uma aba própria
ui-replay-new_tag = Novo
ui-replay-loading_game_data = Carregando dados do jogo...
ui-replay-game_data_failed = Falha ao carregar os dados do jogo: { $error }
ui-replay-group_label = Agrupamento de replays
ui-replay-column_filters = Filtros de coluna
ui-replay-row_elements = Elementos da linha
ui-replay-row_element_personal_rating = Rating pessoal
ui-replay-row_element_match_contribution = Contribuição na partida
ui-replay-row_element_division = Divisão
ui-replay-row_element_twitch = Chat da Twitch
ui-replay-row_element_damage = Dano
ui-replay-row_element_kills = Eliminações
ui-replay-row_element_time = Horário da batalha
ui-replay-listing_options = Opções da lista
ui-replay-open = Abrir
ui-replay-open_file = Abrir arquivo de replay...
ui-replay-check_game_data = Verificar dados do jogo
ui-replay-check_game_data_tooltip = Verificar os dados do jogo necessários para os replays deste diretório
ui-replay-close_side_panel = Fechar painel
ui-replay-nda = NDA
ui-replay-chat = Chat
ui-replay-game_chat = Chat do jogo
ui-replay-save_to_file = Salvar em arquivo
ui-replay-chat_copied = Chat copiado
ui-replay-copy_message = Copiar mensagem
ui-replay-actions = Ações
ui-replay-row_actions_hint = Ações do jogador (ou clique com o botão direito na linha)
ui-replay-section_match = Partida
ui-replay-section_export = Exportar
ui-replay-section_debug = Depuração
ui-replay-timeline = Linha do tempo da partida
ui-replay-row_not_indexed = não indexado
ui-replay-hover_survived = Sobreviveu
ui-replay-hover_sunk = Afundado
ui-replay-hover_not_indexed = Não indexado
ui-replay-row_kills = { $count ->
    [one] { $count } eliminação
   *[other] { $count } eliminações
}
ui-replay-row_survived = vivo
ui-replay-row_sunk = afundado
ui-replay-row_division = Divisão: { $members }
ui-replay-export_results_json = Resultados: JSON
ui-replay-export_results_cbor = Resultados: CBOR
ui-replay-export_results_csv = Resultados: CSV
ui-replay-timeline_filter_all = Todos
ui-replay-timeline_filter_none = Nenhum
ui-replay-timeline_search_hint = Pesquisar navio ou jogador
ui-replay-timeline_no_events = Nenhum evento significativo detectado.
ui-replay-timeline_no_matches = Nenhum evento corresponde ao filtro atual.
ui-replay-timeline_parsing = Analisando eventos...
ui-replay-timeline_failed = Falha ao extrair a linha do tempo da partida: { $error }
ui-replay-timeline_retry = Tentar novamente
ui-replay-timeline_export_tooltip = Aplica-se aos eventos visíveis no momento após a filtragem.
ui-replay-timeline_copied = Linha do tempo copiada
ui-replay-timeline_event_copied = Evento copiado
ui-replay-timeline_saved = Linha do tempo salva em { $path }
ui-replay-timeline_save_failed = A linha do tempo não foi salva: { $error }
ui-replay-timeline_kind_health_lost = Dano pesado
ui-replay-timeline_kind_death = Destruído
ui-replay-timeline_kind_cap_contested = Ponto contestado
ui-replay-timeline_kind_cap_flipped = Ponto capturado
ui-replay-timeline_kind_cap_being_captured = Ponto sendo capturado
ui-replay-timeline_kind_radar = Radar usado
ui-replay-timeline_kind_advantage = Mudança de vantagem
ui-replay-timeline_kind_disconnected = Desconectado

## ui.replay.context
ui-replay-context-open_in_new_tab = Abrir em nova aba
ui-replay-context-copy_cell = Copiar célula
ui-replay-context-copy_row = Copiar linha
ui-replay-context-copy_table = Copiar tabela
ui-replay-context-pin_column = Fixar coluna
ui-replay-context-unpin_column = Desafixar coluna
ui-replay-context-copied = Copiado
ui-replay-context-copy_path = Copiar caminho
ui-replay-context-copy_replays = { $count ->
    [one] Copiar replay
   *[other] Copiar { $count } replays
}
ui-replay-context-copy_paths = { $count ->
    [one] Copiar caminho
   *[other] Copiar { $count } caminhos
}
ui-replay-context-show_in_explorer = Mostrar no explorador de arquivos
ui-replay-context-show_replay_controls = Mostrar controles de replay
ui-replay-context-open_in_game = Abrir no jogo
ui-replay-context-render_replay = Renderizar replay
ui-replay-render = { ui-replay-context-render_replay }
ui-replay-context-set_session_stats = { $count ->
    [one] Definir como estatísticas de sessão ({ $count } replay)
   *[other] Definir como estatísticas de sessão ({ $count } replays)
}
ui-replay-context-add_session_stats = { $count ->
    [one] Adicionar às estatísticas de sessão ({ $count } replay)
   *[other] Adicionar às estatísticas de sessão ({ $count } replays)
}
ui-replay-context-render_to_video = { $count ->
    [one] Exportar como vídeo
   *[other] Exportar { $count } replays como vídeo
}
ui-replay-context-render_to_clipboard = { $count ->
    [one] Exportar para a área de transferência
   *[other] Exportar { $count } replays para a área de transferência
}

## ui.replay.column
ui-replay-column-player_name = Nome do jogador
ui-replay-column-base_xp = XP base
ui-replay-column-ship_name = Nome do navio
ui-replay-column-hits = Acertos
ui-replay-column-heals = Reparos
ui-replay-column-heals_tooltip = Número de ativações da Equipe de Reparos observadas para este jogador. Pode ser impreciso para navios não renderizados na tela (ou seja, navios inimigos que nunca foram avistados).
ui-replay-column-heals_no_repair_tooltip = Este navio não possui o consumível Equipe de Reparos.
ui-replay-column-kills = Eliminações
ui-replay-column-observed_damage = Dano observado
ui-replay-column-actual_damage = Dano real
ui-replay-column-potential_damage = Dano potencial
ui-replay-column-time_lived = Tempo de vida
ui-replay-column-fires = Incêndios
ui-replay-column-floods = Alagamentos
ui-replay-column-citadels = Cidadelas
ui-replay-column-crits = Críticos
ui-replay-column-received_damage = Dano recebido
ui-replay-column-distance_traveled = Distância percorrida
ui-replay-column-skills = Habilidades
ui-replay-column-personal_rating = PR
ui-replay-column-match_contribution = CB

## ui.rating
ui-rating-personal_rating = Rating pessoal
ui-rating-match_contribution = Contribuição na batalha
ui-rating-versus_average = vs. média
ui-rating-damage = Dano
ui-rating-kills = Eliminações
ui-rating-wins = Vitória
ui-rating-team_damage_share = Parcela do dano da equipe
ui-rating-tanking = Fogo atraído
ui-rating-spotting = Avistamento
ui-rating-objectives = Objetivos
ui-rating-anti_air = Antiaéreo
ui-rating-torpedo_warnings = Alertas de torpedo
ui-rating-critical_bursts = Rajadas críticas
ui-rating-unrated_early_exit = Sem CB: a gravação terminou antes da batalha
ui-rating-mc_unrated_battle_type = Sem CB: a contribuição na batalha se aplica apenas a batalhas aleatórias e ranqueadas

## ui.rating.detail
ui-rating-detail-versus_expected = { $actual } vs. { $expected } esperado
ui-rating-detail-wins = { $actual } vencidas vs. { $expected } esperadas
ui-rating-detail-weighted_damage = { $actual } vs. { $expected } esperado, dano a contratorpedeiros e submarinos x1,2
ui-rating-detail-team_damage_share = { $share }% do dano da equipe aos inimigos, { $even }% é uma parcela igual
ui-rating-detail-tanking = { $potential } de dano potencial, { $class }x a média da classe, { $team }x uma parcela igual da equipe
ui-rating-detail-spotting = { $damage } de dano de avistamento (média da classe { $baseline }), { $ships } de HP de navios avistados (média { $ships_baseline })
ui-rating-detail-objectives = Pontos: { $capture } capturando, { $block } bloqueando, { $reset } reiniciando, { $battle_end } mantidos no fim, { $hold } mantendo (contados a um quarto)
ui-rating-detail-anti_air_planes = { $strike ->
    [one] { $strike } avião de ataque
   *[other] { $strike } aviões de ataque
}, { $support ->
    [one] { $support } caça ou avião de reconhecimento
   *[other] { $support } caças e aviões de reconhecimento
}
ui-rating-detail-anti_air_damage = { $damage } de dano antiaéreo
ui-rating-detail-torpedo_warnings = { $threatening ->
    [one] { $threatening } torpedo ameaçador detectado
   *[other] { $threatening } torpedos ameaçadores detectados
}
ui-rating-detail-critical_bursts = { $count ->
    [one] { $count } rajada crítica
   *[other] { $count } rajadas críticas
}: { $bursts }. Visíveis apenas para navios próximos do jogador que gravou
ui-rating-detail-critical_burst = { $share }% de { $ship } em { $seconds } s
ui-rating-detail-critical_burst_at_once = { $share }% de { $ship } de uma vez

## ui.personal_rating
ui-personal_rating-unrated_hover = O rating pessoal se aplica apenas a batalhas aleatórias e ranqueadas.

## ui.rating.basis
ui-rating-basis-static = Estático
ui-rating-basis-dynamic = Dinâmico
ui-rating-basis-static_explained = Estático: medido em relação a uma constante fixa ou a uma referência da classe
ui-rating-basis-dynamic_explained = Dinâmico: medido em relação aos valores esperados do wows-numbers ou aos outros jogadores desta batalha

## ui.replay.group
ui-replay-group-date = Data
ui-replay-group-ship = Navio
ui-replay-group-none = Nenhum

## ui.replay.player
ui-replay-player-hidden_profile = O jogador tem um perfil oculto
ui-replay-player-never_connected = O jogador nunca se conectou à partida

## ui.replay.build
ui-replay-build-open_shiptool = Abrir build no ShipTool
ui-replay-build-open_in_ship_builder = Abrir build no WoWs SB
ui-replay-build-copy_link = Copiar link da build
ui-replay-build-copy_short_link = Copiar link curto da build
ui-replay-build-link_copied = Link da build copiado
ui-replay-build-open_wows_numbers = Abrir página do WoWs Numbers
ui-replay-build-not_spotted = Este navio nunca foi avistado. Informações da build indisponíveis.

## ui.replay.sections
ui-replay-sections-achievements = Conquistas
ui-replay-sections-ribbons = Fitas
ui-replay-sections-modules = Módulos:
ui-replay-sections-modules_none = Sem módulos
ui-replay-sections-signals = Sinalizadores:
ui-replay-sections-loadout = Configuração:
ui-replay-sections-loadout_none = Sem configuração
ui-replay-sections-captain_skills = Habilidades do capitão:
ui-replay-sections-captain_skills_none = Sem habilidades do capitão
ui-replay-sections-consumables = Consumíveis:
ui-replay-sections-fire_chance = Chance efetiva de incêndio
ui-replay-sections-fire_chance_expected = esperado
ui-replay-sections-fire_chance_fires = { $fires ->
    [one] { $fires } incêndio
   *[other] { $fires } incêndios
}
ui-replay-sections-fire_chance_eligible_hits = { $hits ->
    [one] { $hits } acerto que poderia ter causado um
   *[other] { $hits } acertos que poderiam ter causado um
}
ui-replay-sections-fire_chance_ships = em { $ships ->
    [one] { $ships } navio alvo
   *[other] { $ships } navios alvo
}
ui-replay-sections-fire_chance_expected_fires = esperados { $fires } incêndios
ui-replay-sections-fire_chance_per_ship = Por navio alvo
ui-replay-sections-fire_chance_no_eligible_hits = nenhum acerto que poderia ter causado um incêndio
ui-replay-sections-fire_chance_formula = Fórmula da chance de incêndio do atacante
ui-replay-sections-fire_chance_formula_base = burnProb base
ui-replay-sections-fire_chance_formula_clamped = limitado
ui-replay-sections-fire_chance_breakdown = Detalhamento
ui-replay-sections-fire_chance_shells_fired = { $count ->
    [one] projétil HE disparado
   *[other] projéteis HE disparados
}
ui-replay-sections-fire_chance_he_hits = { $count ->
    [one] acerto HE em um navio
   *[other] acertos HE em um navio
}
ui-replay-sections-fire_chance_no_target_ship = { $count ->
    [one] acerto HE não atribuível a um navio alvo
   *[other] acertos HE não atribuíveis a um navio alvo
}
ui-replay-sections-fire_chance_eligible = poderiam ter causado um incêndio
ui-replay-sections-fire_chance_ribbons = { $count ->
    [one] fita SetFire
   *[other] fitas SetFire
}
ui-replay-sections-fire_chance_ribbons_credited = creditadas a um projétil
ui-replay-sections-fire_chance_ribbons_uncredited = não creditadas
ui-replay-sections-fire_chance_unattributed_no_burn_state = nenhuma mudança de estado de incêndio para corresponder
ui-replay-sections-fire_chance_unattributed_already_credited = um incêndio anterior já usou o acerto próximo
ui-replay-sections-fire_chance_unattributed_secondary_contest = uma das nossas próprias secundárias poderia igualmente tê-lo causado
ui-replay-sections-fire_chance_unattributed_hit_contest = todo acerto nosso nas proximidades era ambíguo com outro
ui-replay-sections-fire_chance_unattributed_all_excluded = todo acerto nosso nas proximidades já havia sido excluído
ui-replay-sections-fire_chance_unattributed_none_could_burn = nenhum acerto nosso nas proximidades poderia ter causado um incêndio
ui-replay-sections-fire_chance_unattributed_no_hit = nenhum acerto nosso ocorreu dentro da janela
ui-replay-sections-fire_chance_not_applicable = vítima já destruída, não aplicável
ui-replay-sections-fire_chance_copied = Detalhamento da chance de incêndio copiado
ui-replay-sections-fire_chance_click_to_copy = Clique para copiar este detalhamento
ui-replay-sections-fire_chance_exclusion_already_burning = seção já em chamas
ui-replay-sections-fire_chance_exclusion_victim_build_unknown = build da vítima desconhecida, as zonas de incêndio podem estar mescladas
ui-replay-sections-fire_chance_exclusion_damage_control_active = Equipe de Controle de Avarias ativa
ui-replay-sections-fire_chance_exclusion_damage_control_unknown = estado do controle de avarias desconhecido
ui-replay-sections-fire_chance_exclusion_observation_gap = lacuna de observação
ui-replay-sections-fire_chance_exclusion_consumable_unreliable = modelo de ativação de consumíveis não confiável
ui-replay-sections-fire_chance_exclusion_victim_fate_unknown = destino da vítima desconhecido
ui-replay-sections-fire_chance_exclusion_hit_type_does_not_roll = este tipo de acerto não sorteia incêndio
ui-replay-sections-fire_chance_exclusion_no_geometry = sem geometria das seções de incêndio
ui-replay-sections-fire_chance_exclusion_impact_unplaceable = não foi possível posicionar o impacto no navio ao qual ele foi associado
ui-replay-sections-fire_chance_exclusion_victim_pose_unknown = posição da vítima desconhecida no momento do impacto
ui-replay-sections-fire_chance_exclusion_ambiguous_hit = ambíguo com outro acerto nosso
ui-replay-sections-damage_events = Eventos de dano

## ui.replay.debug
ui-replay-debug-view_raw_metadata = Ver metadados brutos do jogador
ui-replay-debug-raw_metadata = Metadados brutos
ui-replay-debug-raw_results = Resultados brutos
ui-replay-debug-mapped_results = Resultados mapeados
ui-replay-debug-player_metadata = Metadados do jogador
ui-replay-debug-search = Pesquisar
ui-replay-debug-previous_match = Resultado anterior (Shift+Enter)
ui-replay-debug-next_match = Próximo resultado (Enter)

## ui.replay.controls
ui-replay-controls-window_title = Controles de replay
ui-replay-controls-unavailable = Controles não disponíveis (commands.scheme.xml não encontrado nos arquivos do jogo).

## ui.collab
ui-collab-session = Sessão
ui-collab-starting = Iniciando sessão...
ui-collab-session_active = Sessão ativa
ui-collab-connected_to_session = Conectado à sessão
ui-collab-session_token = Token da sessão:
ui-collab-toggle_visibility = Alternar visibilidade do token
ui-collab-copy_token = Copiar token
ui-collab-token_copied = Token copiado para a área de transferência
ui-collab-copy_web_link = Copiar link web
ui-collab-web_link_copied = Link web copiado para a área de transferência
ui-collab-copy_localhost_link = Copiar link localhost
ui-collab-localhost_copied = Link localhost copiado para a área de transferência
ui-collab-connected_count = { $count } conectados
ui-collab-promote_cohost = Promover a coanfitrião
ui-collab-permissions = Permissões
ui-collab-lock_annotations = Bloquear anotações
ui-collab-lock_settings = Bloquear configurações
ui-collab-reset_overrides = Redefinir substituições do cliente
ui-collab-reset_overrides_tooltip = Redefinir todas as alterações de configuração de exibição do cliente
ui-collab-leave = Sair
ui-collab-role_host = Anfitrião
ui-collab-role_cohost = Coanfitrião
ui-collab-you = (você)
ui-collab-display_name = Nome de exibição:
ui-collab-display_name_hint = Seu nome...
ui-collab-enter_display_name = Insira um nome de exibição primeiro
ui-collab-host_session = Hospedar uma sessão
ui-collab-start_session = Iniciar sessão
ui-collab-join_session = Entrar em uma sessão
ui-collab-paste_and_join = Colar token e entrar
ui-collab-clipboard_empty = A área de transferência está vazia
ui-collab-invalid_token = Token inválido: { $error }
ui-collab-no_runtime = Não foi possível iniciar a sessão: sem runtime
ui-collab-shared_windows = Janelas compartilhadas
ui-collab-watching = Assistindo
ui-collab-waiting_for_frames = Aguardando a batalha chegar
ui-collab-open = Abrir
ui-collab-open_for_everyone = Abrir para todos
ui-collab-tactics_board = Quadro tático

## battle
battle-victory = VITÓRIA
battle-defeat = DERROTA
battle-draw = EMPATE
battle-battle_starts_in = A BATALHA COMEÇA EM

## finish_type
finish_type-unknown = Batalha encerrada
finish_type-extermination = Todos os navios inimigos destruídos
finish_type-base_captured = Base capturada
finish_type-timeout = Tempo esgotado
finish_type-failure = Missão fracassada
finish_type-technical = Finalização técnica
finish_type-score = Limite de pontuação atingido
finish_type-score_on_timeout = Vantagem de pontuação no limite de tempo
finish_type-pve_success = Missão cumprida
finish_type-pve_failed = Missão fracassada
finish_type-score_zero = Pontuação esgotada
finish_type-score_excess = Limite de pontuação excedido

## ui.stats
ui-stats-overview = Visão geral
ui-stats-sample_notice = Dados de exemplo, não batalhas registradas
ui-stats-div_all = Todas
ui-stats-div_solo = Solo
ui-stats-div_div = Div
ui-stats-filters = Filtros
ui-stats-own_filters = Filtrar este gráfico separadamente
ui-stats-own_filters_tooltip = Restringe este gráfico sem alterar o que os outros painéis mostram
ui-stats-mode_label = Modo:
ui-stats-clear = Limpar
ui-stats-pr = PR:
ui-stats-mc = CB:
ui-stats-best_frags = Melhor: { $ship } ({ $count ->
    [one] { $count } eliminação
   *[other] { $count } eliminações
})
ui-stats-max_damage = Dano máx: { $ship } ({ $damage })
ui-stats-copy_markdown = Copiar como Markdown
ui-stats-copy_image = Copiar como imagem
ui-stats-copy_image_done = Gráfico copiado para a área de transferência
ui-stats-copy_image_failed = Não foi possível copiar o gráfico
ui-stats-copy_csv = Copiar como CSV
ui-stats-remove_games_hint = Remover todas as partidas de { $ship } (Ctrl+Click para pular confirmação)
ui-stats-settings = Configurações
ui-stats-options = Opções
ui-stats-ships = Navios
ui-stats-filter_ships = Filtrar navios do gráfico
ui-stats-all_ships = Todos
ui-stats-no_ships = Nenhum
ui-stats-search_ships = Pesquisar navios...
ui-stats-no_ship_matches = Nenhum navio corresponde a esta pesquisa
ui-stats-search_statistics = Pesquisar estatísticas
ui-stats-no_statistic_matches = Nenhuma estatística corresponde a esta pesquisa
ui-stats-ship_selection_count = { $selected } de { $total } selecionados
ui-stats-all_ships_tooltip = Selecionar todos os navios correspondentes
ui-stats-no_ships_tooltip = Limpar a seleção dos navios correspondentes
ui-stats-limit_to_recent = Limitar a partidas recentes
ui-stats-limit_to_recent_tooltip = O limite se aplica por navio em Navios e Gráficos, e a todas as partidas em Visão geral.
ui-stats-division_label = Divisão:
ui-stats-add_chart = Adicionar gráfico
ui-stats-clear_tooltip = Esquecer todas as partidas registradas
ui-stats-clear_failed = Não foi limpo: { $reason }
ui-stats-clear_ship_confirm = Pressione novamente para esquecer as partidas deste navio
ui-stats-copy_table = Copiar esta tabela
ui-stats-copied = Copiado para a área de transferência
ui-stats-no_games = Nenhuma partida registrada para os filtros atuais
ui-stats-nothing_to_plot = Nada para exibir com os filtros atuais
ui-stats-column_ship = Navio
ui-stats-column_battles = Batalhas
ui-stats-column_win_rate = Taxa de vitórias
ui-stats-column_avg_damage = Dano médio
ui-stats-column_avg_frags = Abates médios
ui-stats-frags = { $count ->
    [one] { $count } abate
   *[other] { $count } abates
}
ui-stats-statistic = Estatística
ui-stats-open_directory_stats = Estatísticas do diretório
ui-stats-open_directory_stats_tooltip = Abrir estatísticas de todos os replays deste diretório e de seus subdiretórios
ui-stats-directory_stats_unavailable = Os dados do jogo ainda estão carregando
ui-stats-directory_reading_task = Lendo replays para as estatísticas do diretório...
ui-stats-directory_walking = Procurando replays...
ui-stats-directory_scanning = Verificando { $done } de { $total ->
    [one] { $total } replay...
   *[other] { $total } replays...
}
ui-stats-directory_reading = Lendo { $done } de { $total ->
    [one] { $total } replay...
   *[other] { $total } replays...
}
ui-stats-directory_reload = Recarregar
ui-stats-directory_reload_tooltip = Ler novamente os replays deste diretório
ui-stats-directory_skipped = { $count ->
    [one] { $count } replay ignorado
   *[other] { $count } replays ignorados
}
ui-stats-directory_skipped_detail =
    Ilegíveis: { $unreadable }
    Sem dados do jogo para a build: { $missing_build }
    Sem resultados da batalha: { $no_results }
ui-stats-chart = Gráfico
ui-stats-combine_ships = Combinar navios
ui-stats-running_average = Média móvel
ui-stats-value_labels = Rótulos de valores
ui-stats-reset_view = Redefinir visualização
ui-stats-keep_current_layout = Manter layout atual
ui-stats-session_title = Estatísticas de sessão
ui-stats-session_manual = Manual
ui-stats-session_since_opened = Desde que o app foi aberto
ui-stats-session_manual_tooltip = A sessão contém as batalhas que você joga e os replays que você define ou adiciona pela lista de replays. Ela é mantida entre execuções.
ui-stats-session_since_opened_tooltip = A sessão contém todas as batalhas iniciadas às { $time } ou depois, quando o app foi aberto. Sua sessão manual é mantida e volta quando você muda para Manual.
ui-stats-session_battles = { $count ->
    [one] { $count } batalha
   *[other] { $count } batalhas
}
ui-stats-session_chip_manual = Sessão: { $battles }
ui-stats-session_chip_since = Sessão desde { $time }: { $battles }
ui-stats-session_empty_title = Ainda não há batalhas nesta sessão
ui-stats-session_empty_manual = As batalhas que você joga entram na sessão quando terminam. Você também pode definir ou adicionar replays pelo menu da lista de replays.
ui-stats-session_empty_since = As batalhas que começarem depois das { $time } entram na sessão quando terminam.
ui-stats-session_set_add_unavailable = A sessão contém batalhas desde as { $time }. Mude para Manual para definir ou adicionar replays.
ui-stats-clear_unavailable_since = Uma sessão desde a abertura do app não é salva. Mude para Manual para limpar a sessão salva.
ui-stats-charts_save_failed = Configurações dos gráficos não salvas: { $reason }
ui-stats-layout_save_failed = Layout dos gráficos não salvo: { $reason }
ui-stats-axis_game = Partida
ui-stats-chart_hover_game = Partida { $number }, { $time }
ui-stats-series_average = { $stat } (média)
ui-stats-series_running = { $stat } (móvel)
ui-stats-series_more = +{ $count } mais
ui-stats-mode_random = Aleatórias
ui-stats-mode_ranked = Ranqueadas
ui-stats-mode_other = Outras
ui-stats-win_rate_rated_only = A taxa de vitórias conta apenas batalhas aleatórias e ranqueadas
ui-stats-win_rate_breakdown_line = { $mode }: { $rate } (batalhas: { $battles })
ui-stats-matches = Partidas
ui-stats-column_time = Hora
ui-stats-column_mode = Modo
ui-stats-column_result = Resultado
ui-stats-keep_game = Contar esta partida nas estatísticas
ui-stats-leave_out_game = Excluir esta partida das estatísticas
ui-stats-leave_out_ship = Excluir todas as partidas com { $ship }
ui-stats-leave_out_ship_hint = Excluir { $ship } das estatísticas
ui-stats-keep_all_games = Contar todas as partidas
ui-stats-keep_all_games_tooltip = Voltar a contar todas as partidas excluídas manualmente
ui-stats-left_out_only = Apenas excluídas
ui-stats-match_count = { $shown } de { $total ->
    [one] { $total } partida
   *[other] { $total } partidas
}, { $left_out } excluídas
ui-stats-no_matches = Nenhuma partida corresponde aos filtros atuais
ui-stats-search_matches = Pesquisar por navio...
ui-stats-ship_filter_tooltip = Escolha quais navios contam nas estatísticas
ui-stats-keep_ships_tooltip = Contar todos os navios correspondentes
ui-stats-leave_out_ships_tooltip = Excluir todos os navios correspondentes das estatísticas
ui-stats-dates = Datas
ui-stats-all_dates = Todas as datas
ui-stats-clear_dates = Mostrar todas as datas
ui-stats-games_left_out = { $count ->
    [one] { $count } partida excluída
   *[other] { $count } partidas excluídas
}
ui-stats-reset_filters = Redefinir filtros
ui-stats-reset_filters_tooltip = Mostrar todas as partidas: sem limite, período, divisão, modo ou navios e partidas excluídos

## ui.stats.table
ui-stats-table-min = Mín
ui-stats-table-max = Máx
ui-stats-table-total = Total
ui-stats-table-average = Média

## ui.player_tracker
ui-player_tracker-clear_stats = Limpar estatísticas
ui-player_tracker-populate_from_replays = Preencher dados a partir de replays
ui-player_tracker-find_matches = Encontrar partidas
ui-player_tracker-open_wows_numbers = Abrir no wows-numbers
ui-player_tracker-open_shipbuilds = Abrir builds no shipbuilds
ui-player_tracker-subtab_players = Jogadores
ui-player_tracker-filter_hint = Filtrar por jogador ou clã...
ui-player_tracker-time_period = Período
ui-player_tracker-allies = Aliados
ui-player_tracker-enemies = Inimigos
ui-player_tracker-live_no_directory = Defina o diretório do World of Warships para monitorar batalhas
ui-player_tracker-live_checking = Verificando se há uma batalha em andamento...
ui-player_tracker-live_none = Nenhuma batalha em andamento
ui-player_tracker-live_loading_build = Carregando os dados de navios desta build; nomes e classes aparecerão quando terminar.
ui-player_tracker-waiting_for_index = Aguardando o índice de replays
ui-player_tracker-loading_clans = Carregando clãs...
ui-player_tracker-loading_players = Carregando jogadores...
ui-player_tracker-index_failed = Não foi possível ler o índice: { $reason }
ui-player_tracker-no_players = Nenhum jogador indexado neste período
ui-player_tracker-note_not_saved = Não salvo: { $reason }
ui-player_tracker-subtab_current_match = Partida atual
ui-player_tracker-encounters_hover = Visto { $total ->
    [one] { $total } vez
   *[other] { $total } vezes
} no total, { $range } no período selecionado. Visto pela última vez { $last }.
ui-player_tracker-never_encountered = Nunca visto antes
ui-player_tracker-copy_wg_id = Clique para copiar o WG ID
ui-player_tracker-copied_wg_id = WG ID { $id } copiado
ui-player_tracker-aliases_hover = Também conhecido como: { $names }
ui-player_tracker-notes_hint = Notas sobre este jogador
ui-player_tracker-arena_count = { $count ->
    [one] { $count } partida registrada
   *[other] { $count } partidas registradas
}
ui-player_tracker-last_encountered_exact = Último encontro { $timestamp }
ui-player_tracker-subtab_clans = Clãs
ui-player_tracker-clan_no_data = Nenhum encontro com clãs registrado até agora.
ui-player_tracker-find_clan_matches = Encontrar partidas que mencionem esta tag. O índice não tem um campo exclusivo de clã, então nomes de jogadores que contenham a tag também correspondem.
ui-player_tracker-clan_sightings_hover = { $range } destes estão no período selecionado.
ui-player_tracker-clan_member_matches = visto em { $count ->
    [one] { $count } partida
   *[other] { $count } partidas
}
ui-player_tracker-show_division_mates = Mostrar companheiros de divisão
ui-player_tracker-show_division_mates_hover = Conta as partidas que você jogou em divisão. Desativado por padrão, já que você encontrou esses jogadores ali porque os convidou. Outras partidas contra os mesmos jogadores são sempre contadas.
ui-player_tracker-seen_in_twitch_chat = Visto no chat da Twitch
ui-player_tracker-seen_in_twitch_chat_hover = Mostra apenas os jogadores cujo nome correspondeu a um usuário do chat da Twitch durante uma partida com você.
ui-player_tracker-twitch_seen_hover = Visto no chat da Twitch como { $logins } em { $count ->
    [one] { $count } partida
   *[other] { $count } partidas
} com você
ui-player_tracker-win_rate_overall = Geral
ui-player_tracker-win_rate_ship = Navio
ui-player_tracker-view_compact = Compacto
ui-player_tracker-view_detailed = Detalhado
ui-player_tracker-ship_stats = Estatísticas do navio
ui-player_tracker-stats_resolving = Lendo a lista de jogadores da partida...
ui-player_tracker-stats_fetching = Obtendo estatísticas dos jogadores...
ui-player_tracker-stats_unavailable = Estatísticas indisponíveis: { $reason }
ui-player_tracker-stats_hidden = O perfil deste jogador é privado
ui-player_tracker-stats_no_data = Sem estatísticas para este jogador
ui-player_tracker-pr_overall_hover = PR da conta em todos os navios
ui-player_tracker-pr_ship_hover = PR neste navio
ui-player_tracker-win_rate_hover_line = { $label }: { $rate } em { $count ->
    [one] { $battles } batalha
   *[other] { $battles } batalhas
}
ui-player_tracker-win_rate_hover_line_damage = { $label }: { $rate } em { $count ->
    [one] { $battles } batalha
   *[other] { $battles } batalhas
}, { $damage } de dano médio
ui-player_tracker-team_average_win_rate = WR méd.: { $rate }
ui-player_tracker-team_players = { $count ->
    [one] { $count } jogador
   *[other] { $count } jogadores
}
ui-player_tracker-debug_replay_picker = Depuração: tratar um replay como a partida atual
ui-player_tracker-debug_replay_picker_hover = Carrega a lista de jogadores de um replay finalizado nesta aba, para que o caminho das estatísticas possa ser exercitado fora de uma batalha.
ui-player_tracker-debug_replay_none = Escolher um replay

## ui.player_tracker.column
ui-player_tracker-column-win_rate = WR
ui-player_tracker-column-personal_rating = PR
ui-player_tracker-column-battles = Batalhas
ui-player_tracker-column-avg_damage = Dano
ui-player_tracker-column-encounters = Visto
ui-player_tracker-column-clan = Clã
ui-player_tracker-column-player_name = Nome do jogador
ui-player_tracker-column-total_encounters = Total de encontros
ui-player_tracker-column-encounters_in_range = Encontros no período
ui-player_tracker-column-last_encountered = Último encontro
ui-player_tracker-column-members = Membros
ui-player_tracker-column-matches = Partidas
ui-player_tracker-column-sightings = Avistamentos

## ui.player_tracker.period
ui-player_tracker-period-past_hour = Última hora
ui-player_tracker-period-past_six_hours = Últimas 6 horas
ui-player_tracker-period-past_day = Últimas 24 horas
ui-player_tracker-period-past_week = Última semana
ui-player_tracker-period-past_month = Último mês
ui-player_tracker-period-all_time = Todo o período

## ui.twitch
ui-twitch-possible_name = Possível nome de stream: { $name }
ui-twitch-seen_minutes = Visto: { $minutes } minutos após o início da partida
ui-twitch-click_to_copy = Clique para copiar
ui-twitch-copied = { $name } copiado

## ui.renderer
ui-renderer-preparing_export = Preparando exportação...
ui-renderer-encoding = Codificando
ui-renderer-muxing = Multiplexando

## ui.renderer.advantage
ui-renderer-advantage-breakdown = Detalhamento da vantagem
ui-renderer-advantage-team_eliminated = Um time foi eliminado
ui-renderer-advantage-score_projection = Projeção de pontuação
ui-renderer-advantage-fleet_power = Poder da frota
ui-renderer-advantage-strategic_threat = Ameaça estratégica
ui-renderer-advantage-total = Total
ui-renderer-advantage-hp_incomplete = Dados de HP/navios incompletos. Apenas fatores de pontuação exibidos

## ui.renderer.context
ui-renderer-context-show_trail = Mostrar rastro
ui-renderer-context-disable_other_trails = Desativar todos os outros rastros
ui-renderer-context-ranges = Alcances
ui-renderer-context-detection = Detecção
ui-renderer-context-main_battery = Bateria principal
ui-renderer-context-secondary = Secundária
ui-renderer-context-torpedo = Torpedo
ui-renderer-context-radar = Radar
ui-renderer-context-hydro = Hidroacústica
ui-renderer-context-enable_all = Ativar todos
ui-renderer-context-disable_all = Desativar todos
ui-renderer-context-disable_other_ranges = Desativar todos os outros alcances
ui-renderer-context-enable_all_ranges = Ativar alcances de todos os navios
ui-renderer-context-realtime_armor = Mostrar blindagem em tempo real

## ui.renderer.controls
ui-renderer-controls-jump_to_start = Ir para o início
ui-renderer-controls-previous_event = Evento anterior (Shift+Left)
ui-renderer-controls-back_10s = Voltar 10s (Left)
ui-renderer-controls-forward_10s = Avançar 10s (Right)
ui-renderer-controls-next_event = Próximo evento (Shift+Right)
ui-renderer-controls-jump_to_end = Ir para o final
ui-renderer-controls-speed = Velocidade de reprodução

## ui.renderer.settings
ui-renderer-settings-title = Configurações de exibição
ui-renderer-settings-hp_bars = Barras de HP
ui-renderer-settings-armament = Armamento
ui-renderer-settings-dead_ships = Navios destruídos
ui-renderer-settings-dead_ship_names = Nomes de navios destruídos
ui-renderer-settings-player_names = Nomes dos jogadores
ui-renderer-settings-ship_ranges = Alcances dos navios
ui-renderer-settings-ship_names = Nomes dos navios
ui-renderer-settings-camera_direction = Direção das torres
ui-renderer-settings-heat_trail = Rastro térmico
ui-renderer-settings-speed_trails = Rastros de velocidade
ui-renderer-settings-dead_ship_trails = Rastros de navios destruídos
ui-renderer-settings-buildings = Edifícios
ui-renderer-settings-capture_points = Pontos de captura
ui-renderer-settings-consumables = Consumíveis
ui-renderer-settings-planes = Aviões
ui-renderer-settings-smoke = Fumaça
ui-renderer-settings-torpedoes = Torpedos
ui-renderer-settings-tracers = Traçantes
ui-renderer-settings-battle_result = Resultado da batalha
ui-renderer-settings-buff_counters = Contadores de buff
ui-renderer-settings-chat_label = Chat
ui-renderer-settings-kill_feed = Feed de eliminações
ui-renderer-settings-score_label = Pontuação
ui-renderer-settings-score_timers = Temporizadores de pontuação
ui-renderer-settings-team_advantage = Vantagem do time
ui-renderer-settings-team_rosters = Listas das equipes
ui-renderer-settings-stats_panel = Painel de estatísticas
ui-renderer-settings-timer = Temporizador
ui-renderer-settings-export_settings = Configurações de exportação
ui-renderer-settings-prefer_cpu = Preferir codificador CPU
ui-renderer-settings-prefer_cpu_tooltip = Usar codificador por software (CPU) em vez de GPU para exportação de vídeo
ui-renderer-settings-include_pre_battle = Incluir fase pré-batalha
ui-renderer-settings-include_pre_battle_tooltip = Incluir o surgimento e a contagem regressiva antes do início da batalha no vídeo exportado
ui-renderer-settings-export_timing = Tempo do vídeo
ui-renderer-settings-export_timing_length = Ajustar à duração
ui-renderer-settings-export_timing_speed = Velocidade
ui-renderer-settings-export_timing_result_speed = Reproduz a cerca de { $speed }x
ui-renderer-settings-export_timing_result_length = O vídeo dura { $length }
ui-renderer-settings-codec = Codec
ui-renderer-settings-codec_auto = Automático
ui-renderer-settings-save_defaults = Salvar padrões
ui-renderer-settings-save_defaults_done = Salvo como padrão ao abrir o renderizador
ui-renderer-settings-save_defaults_failed = Não foi possível salvar os padrões
ui-renderer-settings-event_timeline = Linha do tempo de eventos

## ui.unpacker
ui-unpacker-assets_bin = Assets.bin
ui-unpacker-loading_assets = Carregando assets.bin...
ui-unpacker-load_failed = Falha ao carregar: { $error }
ui-unpacker-no_game_data = Nenhum dado do jogo carregado. Defina o diretório do World of Warships nas Configurações.
ui-unpacker-filter_files = Filtrar arquivos...
ui-unpacker-search_in_files = Pesquisar em arquivos
ui-unpacker-search_hint = Pesquisar em arquivos...
ui-unpacker-path_filter_hint = Filtro de caminho (ex: *.xml)
ui-unpacker-queue_all_files = Enfileirar todos os arquivos
ui-unpacker-view_as_json = Ver como JSON
ui-unpacker-extract_as_json = Extrair como JSON
ui-unpacker-view_contents = Ver conteúdo
ui-unpacker-version = Versão:
ui-unpacker-decode_prototypes = Decodificar protótipos como JSON
ui-unpacker-decode_prototypes_tooltip = Decodificar protótipos suportados do Assets.bin (.visual, .model, .mfm) para JSON na extração
ui-unpacker-extraction_queue = Fila de extração
ui-unpacker-collapse_folders = Recolher árvore de pastas
ui-unpacker-expand_folders = Expandir árvore de pastas
ui-unpacker-clear_all = Limpar tudo
ui-unpacker-browse = Procurar
ui-unpacker-output_dir_hint = Diretório de saída...
ui-unpacker-remove_from_queue = Remover da fila
ui-unpacker-queue_folder = Enfileirar pasta inteira
ui-unpacker-queue_file = Adicionar este arquivo à fila
ui-unpacker-extraction_output = Extrair para
ui-unpacker-queue_empty = Marque um arquivo ou pasta na lista para adicioná-lo à fila
ui-unpacker-packages = Pacotes
ui-unpacker-item_count = { $count ->
    [one] { $count } item
   *[other] { $count } itens
}
ui-unpacker-search_title = Pesquisa: { $query }
ui-unpacker-search_in = em { $source }
ui-unpacker-image_decode_failed = Não foi possível decodificar esta imagem: { $reason }
ui-unpacker-find = Localizar
ui-unpacker-find_previous = Resultado anterior
ui-unpacker-find_next = Próximo resultado
ui-unpacker-find_close = Fechar busca
ui-unpacker-find_count = { $current } de { $total }
ui-unpacker-find_none = Nenhum resultado
ui-unpacker-find_match_case = Diferenciar maiúsculas
ui-unpacker-queue = Fila
ui-unpacker-reveal_in_tree = Mostrar na árvore de pacotes
ui-unpacker-hit_offset = em { $offset }
ui-unpacker-queued_count = { $count } na fila
ui-unpacker-counting_files = Contando arquivos...
ui-unpacker-extracting = Extraindo { $done } de { $total }
ui-unpacker-extract_failed = Falha na extração: { $reason }
ui-unpacker-extracted = { $count ->
    [one] { $count } arquivo extraído
   *[other] { $count } arquivos extraídos
}
ui-unpacker-cancelled = Cancelado após { $count ->
    [one] { $count } arquivo
   *[other] { $count } arquivos
}
ui-unpacker-extract = { $count ->
    [0] Extrair
    [one] Extrair { $count } item
   *[other] Extrair { $count } itens
}
ui-unpacker-choose_directory = Escolha primeiro um diretório para extrair
ui-unpacker-dump_parameters = Exportar GameParams
ui-unpacker-dump_parameters_tooltip = Exportar o GameParams.data desta build
ui-unpacker-load_a_build = Carregue uma build primeiro
ui-unpacker-parameters_written = Parâmetros gravados
ui-unpacker-save_parameters_title = Salvar parâmetros do jogo
ui-unpacker-writing = Gravando...
ui-unpacker-prototype_written = Protótipo gravado
ui-unpacker-whole_tree = Árvore inteira
ui-unpacker-base_only = Apenas parâmetros base

## ui.unpacker.column
ui-unpacker-column-name = Nome
ui-unpacker-column-size = Tamanho
ui-unpacker-column-type_col = Tipo

## ui.armor
ui-armor-load_settings = Carregue os dados do jogo nas Configurações para usar o Visualizador de blindagem.
ui-armor-load_failed = Falha ao carregar dados do navio: { $error }
ui-armor-empty = Vazio
ui-armor-mirror_cameras = Espelhar câmeras
ui-armor-sync_options = Sincronizar opções
ui-armor-show_armor_by_default = Mostrar blindagem por padrão
ui-armor-sync_tooltip = Manter a visibilidade da blindagem/casco e as sobreposições de exibição (centro do navio, linha d'água, anéis da câmera) sincronizadas em todos os painéis
ui-armor-common_settings = Configurações comuns
ui-armor-tier_filter = Níveis { $lowest } a { $highest }
ui-armor-fortifications = Fortificações
ui-armor-show_hidden_ships = Mostrar navios ocultos
ui-armor-show_hidden_ships_tooltip = Listar também os navios que a lista omite, como os alugados pelo clã
ui-armor-compare_split = Comparar em novo painel
ui-armor-open_in_new_tab = Abrir em nova aba
ui-armor-export_model = Exportar modelo do navio
ui-armor-export_disclaimer = Estes modelos 3D e texturas são propriedade intelectual da Wargaming e qualquer uso destes modelos deve estar em conformidade com as leis locais.
ui-armor-export_button = Exportar
ui-armor-hull_toggle = Casco
ui-armor-splash_tooltip = Alternar visibilidade da caixa de respingo
ui-armor-show_hidden = Mostrar ocultos
ui-armor-show_hidden_tooltip = Alternar visibilidade de painéis (possivelmente) ocultos
ui-armor-gaps = Lacunas
ui-armor-gaps_tooltip = Destacar bordas onde os painéis de blindagem não se conectam (lacunas potenciais)
ui-armor-display = Exibição
ui-armor-marker_opacity = Opacidade dos marcadores
ui-armor-export_tooltip = Exportar modelo do navio para arquivo OBJ
ui-armor-pen_check = Verificação de penetração
ui-armor-trajectory = Trajetória
ui-armor-trajectory_range = Distância
ui-armor-trajectory_clear = Limpar tudo
ui-armor-arc_heading = Arco { $number }
ui-armor-arc_summary = { $hits ->
    [one] { $hits } acerto
   *[other] { $hits } acertos
}, { $armor } mm a { $range } km
ui-armor-arc_delete = Excluir este arco
ui-armor-arc_isolate_plates = Isolar placas
ui-armor-arc_isolate_plates_tooltip = Mostrar apenas as placas de blindagem atravessadas por este arco
ui-armor-arc_isolate_zones = Isolar zonas
ui-armor-arc_isolate_zones_tooltip = Mostrar todas as partes das zonas de blindagem em que este arco entrou
ui-armor-arc_simulation_caveat = Esta simulação se baseia em dados obtidos por engenharia reversa e pode não corresponder à forma como o jogo calcula a balística.
ui-armor-arc_no_shell = Nenhum projétil disparado; as placas atravessadas são mostradas sem ele
ui-armor-arc_detonated_in = Detona dentro de { $zone }
ui-armor-arc_detonated_past_armor = Detona após toda a blindagem
ui-armor-arc_ricochet = Ricochete em { $plate }
ui-armor-arc_shatter = Estilhaçamento em { $plate }
ui-armor-arc_stopped = Parado em { $plate }
ui-armor-arc_overpen = Sobrepenetração
ui-armor-arc_overpen_unarmed = Sobrepenetração (espoleta não armada)
ui-armor-arc_plate_unknown = uma placa não registrada
ui-armor-angle_good = abaixo de 30 graus
ui-armor-angle_mid = 30 a 45 graus
ui-armor-angle_bad = acima de 45 graus
ui-armor-trajectory_ship = Navio:
ui-armor-trajectory_select_ship = Selecionar navio
ui-armor-trajectory_ship_tooltip = Escolha o navio que fornece o projétil da trajetória
ui-armor-trajectory_add_ship = Adicionar navio atirador
ui-armor-trajectory_add_ship_tooltip = Abrir a Verificação de penetração para procurar e adicionar um navio
ui-armor-trajectory_shell = Projétil:
ui-armor-trajectory_select_shell = Selecionar projétil
ui-armor-trajectory_shell_tooltip = Escolha o projétil usado nas trajetórias
ui-armor-continue_ricochet = Continuar após ricochete
ui-armor-splash_mode = Respingo
ui-armor-splash_no_data = Sem dados de respingo para este navio
ui-armor-splash_no_shells = Adicione um navio com projéteis HE/SAP à lista de comparação
ui-armor-splash_tooltip_mode = Clique na blindagem para visualizar o volume de dano de respingo HE
ui-armor-select_ship = Selecione um navio da lista
ui-armor-all_btn = Todos
ui-armor-none_btn = Nenhum
ui-armor-opaque = Opaco
ui-armor-upgrade = Melhoria:
ui-armor-camo_none = Padrão (nenhuma)
ui-armor-camo_group_universal = Camuflagens universais
ui-armor-camo_group_expendable = Camuflagens descartáveis
ui-armor-camo_group_other = Outras camuflagens
ui-armor-show_armor_thickness = Mostrar legenda de espessura da blindagem
ui-armor-plate_edges = Bordas das placas
ui-armor-waterline = Linha d'água
ui-armor-camera_rings = Anéis da câmera
ui-armor-realtime_readout = { $health } HP  -  { $hits ->
    [one] { $hits } acerto
   *[other] { $hits } acertos
}
ui-armor-show_camera_rings = Mostrar anéis da câmera
ui-armor-camera_fov = FOV da câmera
ui-armor-camera_height = Altura da câmera
ui-armor-show_camera_zoom_path = Mostrar caminho de zoom
ui-armor-zoom_path_regular_fov = FOV normal
ui-armor-zoom_path_max_fov = FOV máximo
ui-armor-camera_perspective = Perspectiva da câmera
ui-armor-perspective_fov = FOV de visão
ui-armor-perspective_projection = Projeção
ui-armor-perspective_proj_game = Jogo
ui-armor-perspective_proj_center = Pelo centro
ui-armor-ship_center = Centro do navio
ui-armor-grid = Grade
ui-armor-length_marks = Marcas de comprimento
ui-armor-length_readout = Comprimento { $length }, boca { $beam }
ui-armor-length_metres = { $value } m
ui-armor-length_centimetres = { $value } cm
ui-armor-filter_options = Filtrar opções...
ui-armor-filter_zones = Filtrar zonas...
ui-armor-filter_parts = Filtrar peças...
ui-armor-tree_matches = Correspondências: { $count }
ui-armor-tree_no_matches = Nenhuma correspondência
ui-armor-no_matching_options = Nenhuma opção corresponde ao filtro
ui-armor-opacity = Opacidade
ui-armor-opaque_hull = Casco opaco
ui-armor-lighting = Iluminação do casco
ui-armor-lighting_enabled = Ativar iluminação
ui-armor-lighting_preset_ingame = No jogo
ui-armor-lighting_preset_flat = Plana
ui-armor-lighting_preset_studio = Estúdio
ui-armor-lighting_flat = Plana (ambiente)
ui-armor-lighting_intensity = Intensidade da luz
ui-armor-lighting_azimuth = Azimute da luz
ui-armor-lighting_elevation = Elevação da luz
ui-armor-lighting_rim = Contorno
ui-armor-lighting_specular = Especular
ui-armor-lighting_shininess = Brilho
ui-armor-lighting_key_color = Cor principal
ui-armor-lighting_ambient_color = Cor ambiente
ui-armor-zero_mm_plates = Placas 0 mm
ui-armor-armor_opacity = Opacidade da blindagem
ui-armor-ballistic_disclaimer = Os resultados balísticos são baseados em engenharia reversa/estimativas de como a simulação funciona
ui-armor-roll = Rolagem
ui-armor-reset_plates = Redefinir placas
ui-armor-armor_options = Blindagem
ui-armor-no_ship_loaded = Nenhum navio carregado
ui-armor-lod = LOD:
ui-armor-lod_highest = 0 (máximo)
ui-armor-camouflage = Camuflagem
ui-armor-ships = Navios
ui-armor-compare = Comparar
ui-armor-compare_tooltip = Abrir um novo painel de comparação, ou clique com o botão direito em um navio para compará-lo
ui-armor-mirror_cameras_tooltip = Mover a câmera de todos os painéis em conjunto
ui-armor-show_armor_by_default_tooltip = Abrir os modelos com a blindagem visível em vez do casco
ui-armor-xray_highlight = Destaque de raio X ao passar o cursor na visualização
ui-armor-xray_always = Sempre
ui-armor-xray_while_held = Ao segurar { $key }
ui-armor-xray_off = Desligado
ui-armor-stack_panes = Empilhar painéis
ui-armor-stack_panes_tooltip = Dispor os painéis de comparação um acima do outro em vez de lado a lado
ui-armor-viewports = Visualizações
ui-armor-options_panel = Painel de opções
ui-armor-options_pane = Painel { $number } de { $count }
ui-armor-splash_boxes = Caixas de respingo
ui-armor-cast_here_hint = Segure { $key } e clique para disparar um projétil aqui
ui-armor-trajectory_next_cast = Próximo disparo
ui-armor-arc_pick_hint = Clique em um marcador para editar seu arco.
ui-armor-arc_marker_tooltip = Arco { $number }. Clique para editá-lo. Clique com o botão direito para removê-lo.
ui-armor-zone_toggle_tooltip = Mostra ou oculta todas as placas desta zona. Segure { $key } para mostrar apenas esta zona.
ui-armor-mode_solo_zone = apenas zona
ui-armor-mode_cast_trajectory = disparar trajetória
ui-armor-loading_catalog = Carregando catálogo de navios...
ui-armor-legend_collapse = Recolher
ui-armor-legend_hide = Ocultar legenda
ui-armor-armor_thickness_title = Espessura da blindagem
ui-armor-initializing = Inicializando a visualização 3D...
ui-armor-init_failed = Falha ao inicializar a visualização de blindagem: { $reason }
ui-armor-loading_named_ship = Carregando { $ship }...
ui-armor-options = Opções

## ui.armor.compass
ui-armor-compass-north = N
ui-armor-compass-east = L
ui-armor-compass-south = S
ui-armor-compass-west = O

## ui.armor.realtime
ui-armor-realtime-incoming_fire = Fogo recebido
ui-armor-realtime-attacker_filter = Filtro de atacantes
ui-armor-realtime-all_enemies = Todos os inimigos
ui-armor-realtime-show_secondaries = Mostrar armamento secundário
ui-armor-realtime-seek_to_salvo = Ir para a salva
ui-armor-realtime-no_data = Os dados de projéteis não estão mais disponíveis
ui-armor-realtime-sim_agrees = Simulação concorda
ui-armor-realtime-sim_unchecked = { $count ->
    [one] { $count } projétil não pôde ser simulado
   *[other] { $count } projéteis não puderam ser simulados
}
ui-armor-realtime-sim_rng_zone = Zona de RNG ({ $angle } graus em { $from }-{ $to })
ui-armor-realtime-sim_disagrees = Sim.: { $sim } / Servidor: { $server }
ui-armor-realtime-no_armor_hit = Nenhuma placa de blindagem atingida
ui-armor-realtime-tracked = { $salvos ->
    [one] { $salvos } salva
   *[other] { $salvos } salvas
}, { $shells ->
    [one] { $shells } projétil registrado
   *[other] { $shells } projéteis registrados
}
ui-armor-realtime-nothing_landed = Nada atingiu este navio ainda
ui-armor-realtime-shells = { $count }x
ui-armor-realtime-salvo_shell = { $caliber } mm { $shell }
ui-armor-realtime-salvo_range = { $km } km
ui-armor-realtime-unknown_attacker = Navio { $id }
ui-armor-realtime-server_outcome = Servidor: { $outcome }
ui-armor-realtime-not_cast = Não é possível seguir este projétil pelo casco: o voo ou a posição do navio não foi registrado
ui-armor-realtime-pick_hint = Clique em uma salva ou projétil para segui-lo pelo casco

## ui.armor.plates
ui-armor-plates-heading = Placas atingidas ({ $count })
ui-armor-plates-arrival = { $velocity } m/s no impacto, { $time } s de voo, caindo a { $fall } deg
ui-armor-plates-overmatch = Sobrecalibre: { $penetration } mm de penetração a { $velocity } m/s
ui-armor-plates-penetrated = Penetra: { $penetration } mm contra { $effective } mm efetivos a { $velocity } m/s
ui-armor-plates-ricochet = Ricochete a { $angle } deg
ui-armor-plates-shatter = Estilhaça: { $penetration } mm contra { $effective } mm efetivos
ui-armor-plates-fuse_armed = Espoleta armada
ui-armor-plates-detonation = Detona dentro de { $zone }, { $distance } m após a placa #{ $number }
ui-armor-plates-detonation_past_armor = Detona além de toda a blindagem, { $distance } m após a placa #{ $number }
ui-armor-plates-flat_penetrates = { $ammo } penetra: { $penetration } mm contra { $thickness } mm
ui-armor-plates-flat_shatters = { $ammo } estilhaça: { $penetration } mm contra { $thickness } mm
ui-armor-plates-server_angle = servidor { $angle } deg

## advantage
advantage-absolute = Absoluta
advantage-strong = Forte
advantage-moderate = Moderada
advantage-weak = Fraca

## ui.search
ui-search-edit_value = Editar valor
ui-search-add_filter = Adicionar filtro
ui-search-pill_select = Selecionar
ui-search-pill_deselect = Desmarcar
ui-search-pill_group_all = Agrupar com e
ui-search-pill_group_any = Agrupar com ou
ui-search-pill_ungroup = Desagrupar
ui-search-pill_negate = Negar
ui-search-pill_flip_connector = Inverter e/ou
ui-search-pill_delete = Excluir
ui-search-match_count_truncated = { $count }+ partidas
ui-search-outcome_win = Vitória
ui-search-outcome_loss = Derrota
ui-search-outcome_draw = Empate
ui-search-outcome_unknown = Desconhecido
ui-search-bool_true = verdadeiro
ui-search-bool_false = falso
ui-search-match_count = { $count ->
    [one] { $count } partida
   *[other] { $count } partidas
}
ui-search-row_actions_hint = Ações do replay (ou clique com o botão direito na linha)
ui-search-looking_up_values = Buscando valores...
ui-search-type_a_query = Digite uma consulta e pressione Enter
ui-search-parse_failed = Não foi possível interpretar a consulta: { $reason }
ui-search-searching = Pesquisando...
ui-search-failed = A pesquisa falhou: { $reason }
ui-search-no_matches = Nenhum resultado
ui-search-game_mode_gap_hint = { $count ->
    [one] { $count } partida indexada não tem o modo de jogo registrado. Use Reindexar todos os replays nas Configurações para preenchê-lo.
   *[other] { $count } partidas indexadas não têm o modo de jogo registrado. Use Reindexar todos os replays nas Configurações para preenchê-los.
}
ui-search-open = Abrir
ui-search-open_missing = O arquivo de replay não existe mais
ui-search-free_text = contém "{ $text }"
ui-search-quant_any = qualquer
ui-search-quant_none = nenhum
ui-search-quant_ge = { $n } ou mais
ui-search-quant_eq = exatamente { $n }
ui-search-quant_le = no máximo { $n }
ui-search-quant_gt = mais de { $n }
ui-search-quant_lt = menos de { $n }
ui-search-quant_ne = diferente de { $n }
ui-search-scope_self = Próprio
ui-search-scope_ally = Aliado
ui-search-scope_enemy = Inimigo
ui-search-scope_div = Div

## ui.search.suggest
ui-search-suggest-preset_divmate_test_ship = Alguém na minha divisão jogou com um navio de teste
ui-search-suggest-preset_no_enemy_cv = Nenhum porta-aviões inimigo
ui-search-suggest-preset_all_enemies_died = Todos os inimigos morreram
ui-search-suggest-preset_high_damage_enemies = 3 ou mais inimigos acima de 100k
ui-search-suggest-preset_stream_sniper = Contém um stream sniper
ui-search-suggest-preset_i_survived = Eu sobrevivi
ui-search-suggest-preset_i_disconnected = Eu desconectei
ui-search-suggest-my_damage = Meu dano
ui-search-suggest-enemy_ship = Navio inimigo
ui-search-suggest-allied_ship = Navio aliado
ui-search-suggest-player_in_match = O jogador estava na partida
ui-search-suggest-someone_in_division = Alguém na minha divisão
ui-search-suggest-scope_self = Meu
ui-search-suggest-scope_ally = Aliado
ui-search-suggest-scope_enemy = Inimigo
ui-search-suggest-scope_division = Divisão
ui-search-suggest-scope_anyone = Qualquer jogador

## ui.search.field
ui-search-field-outcome = Resultado
ui-search-field-map = Mapa
ui-search-field-class = Classe
ui-search-field-tier = Nível
ui-search-field-date = Data
ui-search-field-player_present = Jogador
ui-search-field-group = Grupo de replays
ui-search-field-stat_damage = Dano
ui-search-field-stat_kills = Eliminações
ui-search-field-stat_spotting = Avistamento
ui-search-field-stat_potential = Dano potencial
ui-search-field-stat_received = Dano recebido
ui-search-field-stat_pr = PR
ui-search-field-rating = Classificação
ui-search-field-stat_survived = Sobreviveu
ui-search-field-stat_disconnected = Desconectou
ui-search-field-game_type = Tipo de jogo
ui-search-field-game_mode = Modo de jogo
ui-search-field-match_group = Grupo da partida
ui-search-field-build = Build
ui-search-field-results_available = Resultados disponíveis
ui-search-field-relation = Relação
ui-search-field-division = Divisão
ui-search-field-name = Nome
ui-search-field-clan = Clã
ui-search-field-realm = Servidor
ui-search-field-ship = Navio
ui-search-field-ship_index = Índice do navio
ui-search-field-nation = Nação
ui-search-field-test_ship = Navio de teste
ui-search-field-stream_sniper = Stream sniper
ui-search-field-sniper_login = Login do sniper no Twitch

## ui.search.op
ui-search-op-contains = contém
ui-search-op-equals = é igual a
ui-search-op-not_equals = não é igual a
ui-search-op-eq = =
ui-search-op-ne = !=
ui-search-op-gt = >
ui-search-op-ge = >=
ui-search-op-lt = <
ui-search-op-le = <=
ui-search-op-is = é
ui-search-op-is_not = não é
ui-search-op-is_set = está definido
ui-search-op-is_not_set = não está definido

## ui.search.value
ui-search-value-relation_self = Próprio
ui-search-value-relation_ally = Aliado
ui-search-value-relation_enemy = Inimigo
ui-search-value-division_mine = Minha
ui-search-value-division_any = Qualquer
ui-search-value-division_none = Nenhuma
ui-search-value-class_air_carrier = Porta-aviões
ui-search-value-class_battleship = Encouraçado
ui-search-value-class_cruiser = Cruzador
ui-search-value-class_destroyer = Contratorpedeiro
ui-search-value-class_submarine = Submarino
ui-search-value-class_auxiliary = Auxiliar
ui-search-value-rating_bad = Ruim
ui-search-value-rating_below_average = Abaixo da média
ui-search-value-rating_average = Média
ui-search-value-rating_good = Bom
ui-search-value-rating_very_good = Muito bom
ui-search-value-rating_great = Ótimo
ui-search-value-rating_unicum = Unicum
ui-search-value-rating_super_unicum = Super Unicum
ui-search-value-game_mode_test = Teste
ui-search-value-game_mode_standard = Padrão
ui-search-value-game_mode_singlebase = Base única
ui-search-value-game_mode_domination = Domínio
ui-search-value-game_mode_tutorial = Tutorial
ui-search-value-game_mode_megabase = Megabase
ui-search-value-game_mode_forts = Fortes
ui-search-value-game_mode_standard_domination = Domínio padrão
ui-search-value-game_mode_epicenter = Epicentro
ui-search-value-game_mode_assault_defense = Assalto e defesa
ui-search-value-game_mode_pve = PVE
ui-search-value-game_mode_arms_race = Corrida armamentista
ui-search-value-game_mode_epicenter_ring = Anel do epicentro
ui-search-value-game_mode_anti_standard = Padrão invertido
ui-search-value-game_mode_attack_defense = Ataque e defesa
ui-search-value-game_mode_torpedo_beat = Torpedo Beat
ui-search-value-game_mode_team_battle_royale = Battle Royale em equipe
ui-search-value-game_mode_escape_to_portal = Fuga para o portal
ui-search-value-game_mode_domination_asymm = Domínio assimétrico
ui-search-value-game_mode_key_battle = Batalha decisiva
ui-search-value-game_mode_portal_2021 = Portal 2021
ui-search-value-game_mode_team_battle_royale_2021 = Battle Royale em equipe 2021
ui-search-value-game_mode_convoy_event = Evento Comboio
ui-search-value-game_mode_convoy_airship = Comboio aéreo
ui-search-value-game_mode_two_teams_battle_royale = Battle Royale de dois times
ui-search-value-game_mode_pinata_event = Evento Pinhata
ui-search-value-game_mode_respawns = Ressurgimentos
ui-search-value-game_mode_respawns_sectors = Respawns por setor

## ui.search.column
ui-search-column-date = Data
ui-search-column-map = Mapa
ui-search-column-mode = Modo
ui-search-column-ship = Seu navio
ui-search-column-result = Resultado
ui-search-column-damage = Dano
ui-search-column-kills = Eliminações
ui-search-column-pr = PR
ui-search-column-mc = CB

## ui.settings.index
ui-settings-index-heading = Índice de replays
ui-settings-index-description = O registro pesquisável de todos os replays do seu diretório
ui-settings-index-build = Criar índice
ui-settings-index-rebuild = Reindexar tudo
ui-settings-index-rebuild_hint = Lê todos os replays novamente, para corrigir linhas que uma análise antiga errou. Criar índice lê apenas o que falta.
ui-settings-index-progress = { $done } de { $total } lidos, { $failed } não puderam ser lidos
ui-settings-index-progress_skipped = { $done } de { $total } lidos, { $skipped } já incluídos, { $failed } não puderam ser lidos
ui-settings-index-built = { $indexed ->
    [one] { $indexed } replay indexado
   *[other] { $indexed } replays indexados
}, { $failed } não puderam ser lidos
ui-settings-index-built_with_skipped = { $indexed ->
    [one] { $indexed } replay indexado
   *[other] { $indexed } replays indexados
}, { $skipped ->
    [one] { $skipped } já estava incluído
   *[other] { $skipped } já estavam incluídos
}, { $failed } não puderam ser lidos

## ui.settings
ui-settings-loading = Carregando configurações...
ui-settings-load_failed = Falha ao carregar as configurações
ui-settings-not_set = (não definido)

## ui.settings.panel
ui-settings-panel-mcp = Servidor MCP
ui-settings-panel-open = Abrir configurações
ui-settings-panel-search_placeholder = Pesquisar configurações
ui-settings-panel-count = { $count ->
    [one] { $count } configuração
   *[other] { $count } configurações
}
ui-settings-panel-no_matches = Nenhuma configuração corresponde à pesquisa
ui-settings-panel-close = Fechar configurações
ui-settings-panel-in_error = { $name }: uma configuração precisa de atenção
ui-settings-panel-general = Geral
ui-settings-panel-appearance = Aparência
ui-settings-panel-game = World of Warships
ui-settings-panel-replays = Replays
ui-settings-panel-collaboration = Colaboração
ui-settings-panel-twitch = Twitch
ui-settings-panel-language_description = Idioma da interface
ui-settings-panel-data_sharing_description = O que é enviado ao ShipBuilds.com após batalhas ranqueadas e aleatórias
ui-settings-panel-code_integrity_description = Se código não assinado pela Microsoft pode ser carregado neste aplicativo
ui-settings-panel-check_for_updates_description = Procurar uma versão mais recente sempre que o aplicativo for iniciado
ui-settings-panel-data_folder = Pasta de dados do aplicativo
ui-settings-panel-theme_description = Cores da interface. Aponte para uma opção para visualizá-la.
ui-settings-panel-zoom_description = Tamanho de todo o texto e controles
ui-settings-panel-zoom = Zoom
ui-settings-panel-zoom_shortcuts = { $zoom_in } aumenta, { $zoom_out } diminui, { $reset } volta para 1x
ui-settings-panel-zoom_changed = Zoom: { $factor }x
ui-settings-panel-cache_directory = Diretório de cache
ui-settings-panel-cache_directory_description = Onde as builds em cache são armazenadas
ui-settings-panel-cached_builds = Builds em cache
ui-settings-panel-current_path_description = O arquivo de replay aberto mais recentemente
ui-settings-panel-show_raw_xp_description = Adicionar uma coluna de XP bruta à tabela de resultados
ui-settings-panel-show_observed_damage_description = Adicionar uma coluna de dano observado à tabela de resultados
ui-settings-panel-show_entity_id_description = Adicionar uma coluna de ID de entidade à tabela de resultados
ui-settings-panel-show_heals_description = Adicionar uma coluna de curas à tabela de resultados
ui-settings-panel-auto_export_description = Gravar os dados de cada batalha em um arquivo no formato e diretório escolhidos
ui-settings-panel-display_name_description = O nome que os outros veem em uma sessão
ui-settings-panel-get_token_description = Abrir uma página de login que emite um token de chat
ui-settings-panel-token = Token de login
ui-settings-panel-monitored_channel_description = Canal a observar. Deixe vazio para observar o seu.
ui-settings-panel-armor_defaults = Padrões salvos
ui-settings-panel-armor_saved_description = Salvo a partir das opções do Visualizador de modelos

## ui.settings.licenses
ui-settings-licenses-title = Sobre e licenças
ui-settings-licenses-back = Voltar às configurações
ui-settings-licenses-third_party_intro = Este aplicativo contém os componentes de terceiros listados abaixo. Selecione uma entrada para mostrar os pacotes que a utilizam e o texto completo da licença.

## ui.settings.armor
ui-settings-armor-no_defaults = (nenhum padrão salvo)

## ui.app
ui-app-status_cache_checking = Verificando o cache de dados do jogo
ui-app-status_cache_validating = Validando o cache de dados do jogo
ui-app-status_cache_downloading = Baixando dados do jogo
ui-app-status_cache_planning = Consultando quais dados do jogo estão publicados
ui-app-drop_to_load = Solte para carregar { $file }
ui-app-drop_to_list = Solte para listar os replays em { $folder }
ui-app-drop_to_list_replays = Solte para listar { $count ->
    [one] { $count } replay
   *[other] { $count } replays
}
ui-app-drop_to_list_folders = Solte para listar os replays de { $count ->
    [one] { $count } pasta
   *[other] { $count } pastas
}
ui-app-drop_to_open_and_list = Solte para abrir { $file } e listar os replays de { $folders }
ui-app-drop_to_list_replays_and_folders = Solte para listar { $count ->
    [one] { $count } replay
   *[other] { $count } replays
} e os replays de { $folders }
ui-app-drop_folder_count = { $count ->
    [one] { $count } pasta
   *[other] { $count } pastas
}
ui-app-reading_replay = Lendo { $replay }
ui-app-reading_replays = Lendo { $count ->
    [one] { $count } replay
   *[other] { $count } replays
}
ui-app-status_constants_generating = Atualizando as constantes do jogo para { $version }
ui-app-worker_constants = O processo de constantes do jogo
ui-app-worker_live_match = O monitor de batalhas
ui-app-worker_replay_loader = O carregador de replays
ui-app-worker_restarting = { $worker } parou e está reiniciando
ui-app-worker_stalled = { $worker } não está respondendo
ui-app-worker_stopped = { $worker } parou
ui-app-status_indexing = Indexando replays
ui-app-status_of = { $done } de { $total }
ui-app-status_tasks = { $count ->
    [one] { $count } tarefa em execução
   *[other] { $count } tarefas em execução
}
ui-app-status_show_panel = Mostrar painel: { $panel }
ui-app-status_hide_panel = Ocultar painel: { $panel }
ui-app-panel_left = Esquerda
ui-app-panel_right = Direita
ui-app-panel_bottom = Inferior
ui-app-debug_build = Compilação de depuração

## ui.palette
ui-palette-title = Comandos
ui-palette-placeholder = Digite um comando ou pesquise...
ui-palette-go_to = Ir para: { $tab }
ui-palette-advanced_search = Pesquisa avançada...
ui-palette-set_theme = Tema: { $theme }
ui-palette-games_i_died_in = Partidas em que fui destruído
ui-palette-games_i_won = Partidas que venci
ui-palette-send_all_replays = Enviar todos os replays para ShipBuilds
ui-palette-send_all_replays_again = Enviar todos os replays para ShipBuilds (ignorar o registro)
ui-palette-search_players = Pesquisar jogadores...
ui-palette-my_matches_in_ship = Minhas partidas com o navio...
ui-palette-open_ship_in_armor_viewer = Abrir navio no Visualizador de modelos...
ui-palette-open_ship_in_shiptool = Abrir navio no ShipTool...
ui-palette-players_title = Pesquisar jogadores
ui-palette-my_ships_title = Minhas partidas com o navio
ui-palette-armor_ships_title = Abrir navio no Visualizador de modelos
ui-palette-shiptool_ships_title = Abrir navio no ShipTool
ui-palette-players_placeholder = Pesquisar jogadores por nome...
ui-palette-my_ships_placeholder = Pesquisar navios que você jogou...
ui-palette-armor_ships_placeholder = Pesquisar qualquer navio por nome...
ui-palette-armor_ship_row = { $name } -- { $class } nível { $tier }, { $nation }
ui-palette-back_hint = Esc: voltar
ui-palette-ships_loading = Carregando a lista de navios...
ui-palette-ships_unavailable = Sem lista de navios: { $reason }
ui-palette-matches_found = { $count ->
    [one] { $count } resultado
   *[other] { $count } resultados
}
ui-palette-sample_stats = Depuração: mostrar dados de exemplo em Estatísticas
ui-palette-restore_stats = Depuração: mostrar dados registrados em Estatísticas

## ui.replay.hover
ui-replay-hover-ship = Navio
ui-replay-hover-map = Mapa
ui-replay-hover-mode = Modo
ui-replay-hover-blank_mode = Vazio
ui-replay-hover-played = Jogado em
ui-replay-hover-damage = Dano
ui-replay-hover-kills = Eliminações
ui-replay-hover-result = Resultado
ui-replay-hover-division = Divisão
ui-replay-hover-twitch = Chat da Twitch

## ui.replay.renderer
ui-replay-renderer-export_video = Exportar como vídeo
ui-replay-renderer-export_clipboard = Exportar para a área de transferência
ui-replay-renderer-export_saved = Vídeo salvo em { $path }
ui-replay-renderer-export_copied = Vídeo copiado para a área de transferência
ui-replay-renderer-export_not_written = Não foi possível exportar o vídeo: { $reason }
ui-replay-renderer-pop_out = Abrir em janela própria
ui-replay-renderer-export_path_unusable = Não é possível gravar nesse local
ui-replay-renderer-batch_started = Renderizando { $count ->
    [one] { $count } replay
   *[other] { $count } replays
} em { $dir }
ui-replay-renderer-batch_finished = { $written ->
    [one] { $written } replay renderizado
   *[other] { $written } replays renderizados
}; { $failed } não puderam ser renderizados
ui-replay-renderer-batch_all_written = { $written ->
    [one] { $written } replay renderizado
   *[other] { $written } replays renderizados
}
ui-replay-renderer-batch_copied = { $written ->
    [one] { $written } replay renderizado e copiado para a área de transferência
   *[other] { $written } replays renderizados e copiados para a área de transferência
}
ui-replay-renderer-batch_not_copied = Renderizado em { $dir }, mas a área de transferência os recusou: { $reason }
ui-replay-renderer-batch_progress = Renderizando { $done } de { $total }: { $replay }
ui-replay-renderer-baking = Lendo o replay e percorrendo a batalha...
ui-replay-renderer-failed = Não foi possível reproduzir este replay
ui-replay-renderer-play = Reproduzir
ui-replay-renderer-pause = Pausar

## ui.renderer.annotations
ui-renderer-annotations-title = Ferramentas de desenho
ui-renderer-annotations-undo = Desfazer
ui-renderer-annotations-redo = Refazer
ui-renderer-annotations-ship_hint = Encontrar um navio
ui-renderer-annotations-no_ship = Nenhum navio escolhido
ui-renderer-annotations-nib = Espessura do traço
ui-renderer-annotations-thinner = Traço mais fino
ui-renderer-annotations-thicker = Traço mais grosso
ui-renderer-annotations-shortcuts_title = Atalhos de teclado
ui-renderer-annotations-shortcuts_tools = Ferramentas
ui-renderer-annotations-shortcuts_actions = Ações
ui-renderer-annotations-tool_select = Selecionar
ui-renderer-annotations-tool_arrow = Seta
ui-renderer-annotations-tool_freehand = Mão livre
ui-renderer-annotations-tool_line = Linha
ui-renderer-annotations-tool_circle = Círculo
ui-renderer-annotations-tool_rectangle = Retângulo
ui-renderer-annotations-tool_triangle = Triângulo
ui-renderer-annotations-tool_measurement = Medição
ui-renderer-annotations-tool_eraser = Borracha
ui-renderer-annotations-action_undo = Desfazer
ui-renderer-annotations-action_multi_select = Seleção múltipla
ui-renderer-annotations-action_stroke_width = Espessura do traço
ui-renderer-annotations-action_delete = Excluir
ui-renderer-annotations-action_cancel = Cancelar / Desmarcar
ui-renderer-annotations-toggle_fill = Clique novamente para alternar o preenchimento
ui-renderer-annotations-clear_all = Limpar tudo
ui-renderer-annotations-edit = Editar desenho
ui-renderer-annotations-size = Tamanho
ui-renderer-annotations-color = Cor
ui-renderer-annotations-custom_ink = Cor personalizada
ui-renderer-annotations-filled = Preenchido
ui-renderer-annotations-locked = Anotações bloqueadas pelo host

## ui.armor.cast
ui-armor-cast-prompt = { $key }+clique no casco para adicionar um ponto de impacto
ui-armor-cast-empty = Nenhum ponto de impacto ainda
ui-armor-cast-miss_nothing_under_pointer = Nenhuma placa de blindagem sob o cursor
ui-armor-cast-miss_thickness_unknown = As placas deste modelo não têm uma espessura conhecida para disparar contra
ui-armor-cast-miss_no_plate_on_path = A trajetória do projétil a partir deste ângulo não cruza nenhuma placa de blindagem

## ui.armor.pen
ui-armor-pen-search_hint = Pesquisar navios para comparar...
ui-armor-pen-add_a_ship = Pesquise e adicione navios acima para comparar a penetração
ui-armor-pen-hover_a_plate = Passe o cursor sobre uma placa do navio para verificá-la
ui-armor-pen-awaiting_plate = passe o cursor sobre uma placa
ui-armor-pen-clear_all = Limpar tudo
ui-armor-pen-remove = Remover da comparação
ui-armor-pen-firing_ship_tooltip = Usar os projéteis deste navio nas trajetórias
ui-armor-pen-use_for_trajectory = Usar na trajetória
ui-armor-pen-no_shells = { $ship } não tem projéteis de bateria principal
ui-armor-pen-ifhe = IFHE
ui-armor-pen-ifhe_tooltip = O Detonador de Inércia para Projéteis HE aumenta a penetração HE em um quarto
ui-armor-pen-penetrates = penetra
ui-armor-pen-shatters = estilhaça
ui-armor-pen-angle_dependent = depende do ângulo
ui-armor-pen-no_data = sem dados de penetração

## ui.armor.splash
ui-armor-splash-hint = Clique na blindagem no modo de respingo para ver quais zonas uma explosão atinge
ui-armor-splash-half_extent = Meia extensão da explosão: { $value } unidades do modelo
ui-armor-splash-triangles = Triângulos de blindagem na explosão: { $total }, penetrados: { $penetrated }
ui-armor-splash-no_zones = Nenhuma caixa de respingo se sobrepõe neste ponto
ui-armor-splash-zones = Zonas na explosão
ui-armor-splash-direct_hit = Acerto direto
ui-armor-splash-hp = { $value } HP
ui-armor-splash-section = Zonas de respingo

## ui.armor.export
ui-armor-export-hull = Casco
ui-armor-export-contents = Conteúdo
ui-armor-export-contents_mesh = Malha
ui-armor-export-contents_armor = Blindagem
ui-armor-export-contents_both = Malha e blindagem
ui-armor-export-lod = Detalhe da malha
ui-armor-export-lod_tooltip = 0 é o maior detalhe. A blindagem não tem níveis de detalhe e não é afetada.
ui-armor-export-res = Resolução das texturas
ui-armor-export-res_full = Completa
ui-armor-export-res_2048 = 2048 px
ui-armor-export-res_1024 = 1024 px
ui-armor-export-res_512 = 512 px
ui-armor-export-camos = Camuflagens
ui-armor-export-camo_select_none = Desmarcar todas
ui-armor-export-camo_reset = Restaurar padrão
ui-armor-export-loading_details = Carregando...
ui-armor-export-size_estimate = Tamanho estimado do arquivo: { $size }
ui-armor-export-size_breakdown = Geometria { $geometry } (exato), texturas { $textures } (estimado)
ui-armor-export-size_incomplete = Não foi possível estimar { $count ->
    [one] { $count } camuflagem
   *[other] { $count } camuflagens
}; o tamanho pode estar incompleto
ui-armor-export-exporting = Exportando { $ship }...
ui-armor-export-already_running = Uma exportação já está em andamento
ui-armor-export-exported = { $ship } exportado
ui-armor-export-exported_with_size = { $ship } exportado ({ $size })
ui-armor-export-vehicle_not_found = Navio não encontrado
ui-armor-export-export_failed = Falha na exportação: { $error }

## ui.armor.outcome
ui-armor-outcome-penetration = Penetração
ui-armor-outcome-citadel = Cidadela
ui-armor-outcome-ricochet = Ricochete
ui-armor-outcome-shatter = Estilhaçamento
ui-armor-outcome-overpenetration = Sobrepenetração
ui-armor-outcome-underwater = Subaquático
ui-armor-outcome-unknown = Não reconhecido ({ $raw })
ui-armor-outcome-stopped = Parado na blindagem

## ui.tactics
ui-tactics-a_peers_board = Um quadro compartilhado por outra pessoa nesta sessão
ui-tactics-map = Mapa
ui-tactics-map_hint = Encontrar um mapa
ui-tactics-mode = Modo
ui-tactics-pick_a_map = Escolha um mapa para desenhar
ui-tactics-drawing = Desenhando o mapa...
ui-tactics-reset_view = Redefinir visualização
ui-tactics-add_cap = Adicionar ponto
ui-tactics-add_cap_tooltip = Posicionar um ponto de captura no próximo clique no mapa
ui-tactics-selected_cap = Ponto { $letter }
ui-tactics-cap_radius = { $km } km
ui-tactics-cap_team = Equipe
ui-tactics-cap_team_tooltip = Alternar a equipe do ponto de captura selecionado
ui-tactics-team_neutral = Neutro
ui-tactics-team_one = Equipe 1 (Verde)
ui-tactics-team_two = Equipe 2 (Vermelha)
ui-tactics-delete_cap = Excluir
ui-tactics-delete_cap_tooltip = Remover o ponto de captura selecionado
ui-tactics-cap_replay_delete_tooltip = Um ponto de captura carregado de um replay não pode ser excluído
ui-tactics-cap_narrow_tooltip = Diminuir o raio do ponto de captura selecionado
ui-tactics-cap_widen_tooltip = Aumentar o raio do ponto de captura selecionado
ui-tactics-undo_tooltip = Desfazer a última alteração no quadro ({ $shortcut })
ui-tactics-redo_tooltip = Refazer a última alteração desfeita no quadro ({ $shortcut } ou { $alternate })
ui-tactics-clear_caps = Limpar pontos adicionados
ui-tactics-clear_added_caps_tooltip = Remover pontos de captura adicionados pelo usuário
ui-tactics-preset_name = Nome do quadro
ui-tactics-preset_save = Salvar
ui-tactics-preset_delete = Excluir
ui-tactics-reset_board = Redefinir quadro
ui-tactics-reset_board_tooltip = Restaurar os pontos do modo selecionado e limpar os desenhos
ui-tactics-preset_needs_a_name = Dê um nome ao quadro primeiro
ui-tactics-preset_needs_a_map = Escolha um mapa para o quadro primeiro
ui-tactics-preset_saved = { $name } salvo
ui-tactics-preset_save_failed = Não foi possível salvar o quadro: { $error }
ui-tactics-preset_load_failed = Não foi possível abrir o quadro: { $error }
ui-tactics-preset_delete_failed = Não foi possível excluir o quadro: { $error }
ui-tactics-presets = Quadros salvos
ui-tactics-preset_open = Abrir
ui-tactics-clear_drawing = Limpar desenho
ui-tactics-scan_replays = Preencher pontos a partir de replays
ui-tactics-scan_replays_tooltip = Ler os replays para obter a disposição dos pontos de captura dos modos em que foram jogados
ui-tactics-scan_needs_game_data = Carregue uma build do jogo primeiro
ui-tactics-scan_running = Lendo replays...
ui-tactics-scan_progress = Lendo replays... { $read } / { $total }
ui-tactics-scan_done = { $total ->
    [one] { $total } replay lido
   *[other] { $total } replays lidos
} e { $added ->
    [one] { $added } nova disposição encontrada
   *[other] { $added } novas disposições encontradas
}
ui-tactics-session_boards_full = A sessão já tem { $limit ->
    [one] { $limit } quadro aberto
   *[other] { $limit } quadros abertos
}, então este quadro fica no mapa atual
ui-tactics-placing = Posicionando { $ship }
ui-tactics-side_friendly = Aliado
ui-tactics-side_enemy = Inimigo
ui-tactics-ink_white = Tinta branca
ui-tactics-ink_gray = Tinta cinza
ui-tactics-ink_red = Tinta vermelha
ui-tactics-ink_orange = Tinta laranja
ui-tactics-ink_yellow = Tinta amarela
ui-tactics-ink_green = Tinta verde
ui-tactics-ink_blue = Tinta azul
ui-tactics-ink_purple = Tinta roxa
ui-tactics-ink_pink = Tinta rosa
ui-tactics-no_map_art = Esta build não inclui imagem para esse mapa
ui-tactics-ship_config = Configuração do navio
ui-tactics-hull = Casco
ui-tactics-hull_default = Padrão
ui-tactics-modifiers = Modificadores
ui-tactics-concealment_expert = Especialista em ocultação
ui-tactics-gun_range_mod = Mod. de alcance dos canhões
ui-tactics-secondary_range_mod = Mod. de alcance secundário
ui-tactics-ship_ranges = Círculos de alcance
ui-tactics-shape = Forma
ui-tactics-shape_size = Tamanho
ui-tactics-shape_size_km = Tamanho (km)
ui-tactics-shape_filled = Preenchida
ui-tactics-shape_smaller = Diminuir a forma
ui-tactics-shape_larger = Aumentar a forma

## ui.armor.sim
ui-armor-sim-overmatch_rules_out_ricochet = Overmatch, portanto sem ricochete
ui-armor-sim-always_ricochet_band = Ricochete, na faixa de ricochete garantido
ui-armor-sim-ricochet_angle_too_low = Ricochete impossível, ângulo muito baixo

## ui.launcher
ui-launcher-rescan = Procurar novamente
ui-launcher-add_install = Adicionar instalação...
ui-launcher-add_install_title = Diretório de instalação do World of Warships
ui-launcher-no_installs = Nenhuma instalação encontrada. Adicione uma ou instale um jogo abaixo.
ui-launcher-managed_by_steam = gerenciado pela Steam
ui-launcher-checking_updates = verificando atualizações
ui-launcher-up_to_date = Atualizado
ui-launcher-update_available = Atualização disponível: { $parts }
ui-launcher-not_installed = não instalado: { $parts }
ui-launcher-check_failed = falha ao verificar atualizações: { $error }
ui-launcher-recheck = Verificar novamente
ui-launcher-download = Baixar / Atualizar...
ui-launcher-download_title = Onde instalar a build
ui-launcher-download_hint = Uma instalação nova ocupa dezenas de GB; atualizações delta ainda não são suportadas, então uma parte alterada é baixada por completo.
ui-launcher-pause = Pausar
ui-launcher-cancel = Cancelar
ui-launcher-resume = Retomar
ui-launcher-paused = Download pausado; os arquivos preparados são mantidos.
ui-launcher-installed = Instalado
ui-launcher-installed_skipped = Instalado ({ $count ->
    [one] { $count } parte delta ignorada
   *[other] { $count } partes delta ignoradas
}: ainda não suportado, baixado mas não aplicado)
ui-launcher-download_failed = Falha no download: { $error }
ui-launcher-update_in_place = Uma atualização é instalada na pasta da própria instalação. Escolha { $path } para atualizá-la.
ui-launcher-cancelled = Download cancelado.
ui-launcher-launch_failed = Falha ao iniciar: { $error }
ui-launcher-use_for_toolkit = Usar no toolkit
ui-launcher-install_heading = Instalar um jogo
ui-launcher-install_hint = Uma instalação nova ocupa dezenas de GB.
ui-launcher-install_title = Onde instalar o jogo
ui-launcher-install = Instalar...
ui-launcher-checking_version = verificando versão disponível
ui-launcher-version = versão { $version }
ui-launcher-version_unreported = versão do cliente não informada
ui-launcher-unavailable = indisponível: { $error }
ui-launcher-retry = Tentar novamente

## ui.launcher.account
ui-launcher-account-sign_in = Entrar
ui-launcher-account-add_title = Adicionar conta
ui-launcher-account-again_title = Entrar novamente
ui-launcher-account-memory_only = Conectado apenas nesta sessão. Os dados não são salvos, então você precisará entrar novamente após reiniciar.
ui-launcher-account-password_note = Sua senha é enviada apenas para a Wargaming e nunca é armazenada.
ui-launcher-account-realm_fixed = Região: { $realm }
ui-launcher-account-email = E-mail
ui-launcher-account-password = Senha
ui-launcher-account-code = Código de dois fatores
ui-launcher-account-code_required = Código de dois fatores necessário.
ui-launcher-account-use_backup_code = Usar código de backup
ui-launcher-account-submit_code = Enviar código
ui-launcher-account-captcha = Este login exige um CAPTCHA, que este inicializador ainda não consegue exibir. Entre uma vez pelo Wargaming Game Center oficial ou pelo wargaming.net em um navegador e tente novamente.

## ui.launcher.sign_in_error
ui-launcher-sign_in_error-wrong_credentials = E-mail ou senha incorretos.
ui-launcher-sign_in_error-two_factor_required = Esta conta exige aprovação em duas etapas. Aprove o login no Wargaming Auth ou com o código enviado por e-mail e entre novamente.
ui-launcher-sign_in_error-two_factor_invalid = O código de duas etapas está incorreto ou expirou. Digite um código novo.
ui-launcher-sign_in_error-captcha_required = A Wargaming pede um CAPTCHA. Entre uma vez no Game Center oficial ou em wargaming.net e tente novamente.
ui-launcher-sign_in_error-rate_limited = Muitas tentativas de login. Aguarde alguns minutos e tente novamente.
ui-launcher-sign_in_error-account_banned = Esta conta está banida.
ui-launcher-sign_in_error-network_unreachable = Não foi possível acessar a Wargaming. Verifique sua conexão com a internet.
ui-launcher-sign_in_error-service_unavailable = O serviço de login da Wargaming está indisponível. Tente novamente mais tarde.
ui-launcher-sign_in_error-session_expired = O login salvo expirou. Entre novamente.
ui-launcher-sign_in_error-keyring_locked = O chaveiro do sistema está bloqueado. Desbloqueie-o e entre novamente.
ui-launcher-sign_in_error-not_saved = Conectado, mas o login não pôde ser salvo. Você precisará entrar novamente após reiniciar.
ui-launcher-sign_in_error-unexpected = Falha no login. O log tem os detalhes.

## ui.launcher.realm
ui-launcher-realm-eu = Europa
ui-launcher-realm-na = América do Norte
ui-launcher-realm-asia = Ásia

## ui.launcher.runner
ui-launcher-runner-looking = Procurando o Proton.
ui-launcher-runner-steam_proton = { $name } do Steam
ui-launcher-runner-umu = umu-run com { $name }
ui-launcher-runner-umu_default = umu-run com UMU-Proton
ui-launcher-runner-native = O jogo roda nativamente.
ui-launcher-runner-first_run_prefix = A primeira inicialização cria um prefixo do Wine, o que leva alguns segundos.
ui-launcher-runner-first_run_download = A primeira inicialização baixa o UMU-Proton e um runtime do Steam, cerca de 700 MB.
ui-launcher-runner-choose = Escolher uma pasta do Proton
ui-launcher-runner-automatic = Encontrar automaticamente
ui-launcher-runner-detect = Procurar novamente
ui-launcher-runner-needed = É necessário o Proton para iniciar no Linux. Instale o Steam com Proton ou o umu-launcher.

## ui.replay.damage_share
ui-replay-damage_share-mode = Dano como parte do total do outro navio
ui-replay-damage_share-dealt = { $pct } do dano infligido por { $ship } foi para { $other }.
ui-replay-damage_share-dealt_inverse = { $pct } do dano sofrido por { $other } veio de { $ship }.
ui-replay-damage_share-received = { $pct } do dano sofrido por { $ship } veio de { $other }.
ui-replay-damage_share-received_inverse = { $pct } do dano infligido por { $other } foi para { $ship }.
ui-replay-damage_share-hold = Segure { $key } para inverter.
ui-replay-damage_share-release = Solte { $key } para voltar.

## ui.reindex
ui-reindex-context = { $count ->
    [one] Forçar reindexação
   *[other] Forçar reindexação de { $count } replays
}
ui-reindex-palette_by_version = Forçar reindexação de replays: por versão
ui-reindex-palette_all = Forçar reindexação de replays: todos
ui-reindex-versions_title = Forçar reindexação de replays: por versão
ui-reindex-versions_placeholder = Filtrar versões do jogo...
ui-reindex-version_row = { $version } -- { $count ->
    [one] { $count } replay
   *[other] { $count } replays
}
ui-reindex-no_versions = Nenhum replay listado indica uma versão do jogo
ui-reindex-confirm_title = Forçar reindexação de replays
ui-reindex-confirm_ok = Reindexar
ui-reindex-confirm_count = { $count ->
    [one] { $count } replay será lido novamente e sua entrada de índice será reescrita.
   *[other] { $count } replays serão lidos novamente e suas entradas de índice serão reescritas.
}
ui-reindex-confirm_range = Jogados de { $oldest } até { $newest }.
ui-reindex-confirm_undated = { $count ->
    [one] { $count } replay a ser lido não tem data de batalha legível e não está nesse intervalo.
   *[other] { $count } replays a serem lidos não têm data de batalha legível e não estão nesse intervalo.
}
ui-reindex-confirm_downloads = Os dados do jogo dessas versões são baixados primeiro:
ui-reindex-confirm_unobtainable = Não é possível obter dados do jogo para essas versões, então { $count ->
    [one] seu { $count } replay é ignorado:
   *[other] seus { $count } replays são ignorados:
}
ui-reindex-confirm_no_cache_dir = Nenhuma pasta de cache de dados do jogo foi definida, então não é possível baixar os dados do jogo dessas versões e { $count ->
    [one] seu { $count } replay é ignorado:
   *[other] seus { $count } replays são ignorados:
}
ui-reindex-confirm_no_build = { $count ->
    [one] { $count } replay não indica build do jogo e é ignorado.
   *[other] { $count } replays não indicam build do jogo e são ignorados.
}
ui-reindex-confirm_no_version = { $count ->
    [one] { $count } replay não indica uma versão do jogo legível e é ignorado.
   *[other] { $count } replays não indicam uma versão do jogo legível e são ignorados.
}
ui-reindex-build_row = { $version } (build { $build }): { $count ->
    [one] { $count } replay
   *[other] { $count } replays
}
ui-reindex-nothing_listed = A lista não contém replays para reindexar.
ui-reindex-nothing_readable = Nenhum desses replays pode ser reindexado: não há dados do jogo disponíveis ({ $count } ignorados).
ui-reindex-no_game_data = Os replays só podem ser lidos depois que a pasta do jogo for definida.
ui-reindex-busy_index = Uma indexação já está em andamento.
ui-reindex-busy_cache = Uma tarefa de dados do jogo já está em andamento. Tente novamente quando terminar.
ui-reindex-planning = Verificando quais dados do jogo estão disponíveis...
ui-reindex-no_index = O índice de replays não está aberto, então nada foi reindexado.
ui-reindex-done = { $indexed ->
    [one] { $indexed } replay reindexado
   *[other] { $indexed } replays reindexados
}; { $skipped } ignorados, { $failed } não puderam ser lidos
ui-reindex-stopped = Reindexação parada: { $indexed } reindexados, { $skipped } ignorados, { $failed } não puderam ser lidos
ui-reindex-stopped_before_reading = Reindexação parada antes de qualquer replay ser lido.

## ui.index_folder
ui-index_folder-tooltip = Indexar todos os replays desta lista que o índice de replays ainda não contém
ui-index_folder-confirm_title = Indexar replays
ui-index_folder-confirm_ok = Indexar
ui-index_folder-confirm_count = { $count ->
    [one] { $count } replay desta lista ainda não está no índice de replays e será lido e adicionado.
   *[other] { $count } replays desta lista ainda não estão no índice de replays e serão lidos e adicionados.
}
ui-index_folder-note_live = Os replays indexados são adicionados ao índice de replays lido pela Pesquisa e pelo Rastreador de jogadores.
ui-index_folder-note_folder = Os replays indexados são adicionados ao índice de replays sob a fonte própria deste diretório ({ $dir }). A Pesquisa e a lista de jogadores do Rastreador de jogadores os incluem. A lista de clãs do Rastreador de jogadores lê apenas os replays do próprio jogo.
ui-index_folder-nothing_listed = Esta lista não contém replays para indexar.
ui-index_folder-nothing_missing = Todos os replays desta lista já estão indexados.
ui-index_folder-unreadable_index = Não foi possível ler o índice de replays.
ui-index_folder-done = { $indexed ->
    [one] { $indexed } replay indexado
   *[other] { $indexed } replays indexados
}; { $skipped } ignorados, { $failed } não puderam ser lidos
ui-index_folder-stopped = Indexação parada: { $indexed } indexados, { $skipped } ignorados, { $failed } não puderam ser lidos
ui-index_folder-stopped_before_reading = A indexação parou antes de qualquer replay ser lido.

## ui.replay_folders
ui-replay_folders-install = Jogo
ui-replay_folders-watched = Monitorado: replays novos e alterados aparecem aqui
ui-replay_folders-unwatched = Não monitorado: use Atualizar para lê-lo novamente
ui-replay_folders-unseen = { $count ->
    [one] { $count } replay novo ou alterado desde a última visualização
   *[other] { $count } replays novos ou alterados desde a última visualização
}
ui-replay_folders-missing = Este diretório não existe mais: { $dir }
ui-replay_folders-missing_hint = Ele será aberto aqui a cada inicialização até você desafixá-lo.
ui-replay_folders-pin = Fixar
ui-replay_folders-unpin = Desafixar
ui-replay_folders-refresh = Atualizar
ui-replay_folders-close = Fechar
ui-replay_folders-pins_unreadable = Não foi possível ler os diretórios de replays fixados.
ui-replay_folders-game_data_available = Os dados do jogo estão disponíveis para os replays listados.
ui-replay_folders-browse = Procurar...
ui-replay_folders-recent_title = Abrir diretório de replays
ui-replay_folders-recent_placeholder = Filtrar diretórios recentes...
ui-replay_folders-recent_missing = Este diretório não existe mais e foi removido da lista de recentes: { $dir }
ui-replay_folders-checking = Procurando { $dir }...
ui-replay_folders-indexing = Indexando { $dir }
ui-replay_folders-indexing_in_background = Este diretório ainda está sendo indexado
ui-replay_folders-search_while_indexing = Este diretório ainda está sendo indexado, então os resultados ainda não estão completos. Pesquise novamente quando terminar.
ui-replay_folders-source_failed = Não é possível indexar os replays deste diretório: { $error }
ui-replay_folders-restart_watch = Reiniciar monitoramento
ui-replay_folders-watch_stopped = O monitoramento de { $folder } parou: { $cause }.
ui-replay_folders-watch_restarting = Reinicia em { $seconds } s (tentativa { $attempt } de { $attempts }).
ui-replay_folders-watch_gave_up = O reinício falhou { $attempts ->
    [one] { $attempts } vez
   *[other] { $attempts } vezes
} e não será tentado novamente.
ui-replay_folders-watch_restart_hint = Clique no ponto vermelho ou escolha Reiniciar monitoramento no menu da aba para reiniciá-lo agora.
ui-replay_folders-watch_resumed = O monitoramento de { $folder } foi retomado.
ui-replay_folders-watch_cause_backend = o sistema de arquivos relatou um erro: { $error }
ui-replay_folders-watch_cause_gone = a pasta foi movida ou removida
ui-replay_folders-watch_cause_panicked = erro interno: { $error }
ui-replay_folders-watch_cause_exited = o monitoramento terminou inesperadamente
ui-replay_folders-dropped = Soltos
ui-replay_folders-dropped_numbered = Soltos { $number }
ui-replay_folders-dropped_count = { $count ->
    [one] { $count } replay solto
   *[other] { $count } replays soltos
}
ui-replay_folders-dropped_no_source = Replays soltos são indexados com seus próprios diretórios. Abra um diretório para pesquisá-lo ou indexá-lo.
ui-replay_folders-unreadable_replay = Não foi possível ler: { $error }

## ui.models
ui-models-heading = Modelos
ui-models-loading = Carregando modelos...
ui-models-unavailable = Modelos indisponíveis
ui-models-search = Pesquisar navios e modelos...
ui-models-open = Abrir
ui-models-export = Exportar modelo
ui-models-copy_path = Copiar caminho do modelo
ui-models-path_copied = { $path } copiado
ui-models-skin_replaces = Substitui { $model } quando { $exterior } está equipado
ui-models-geometry_absent = Não está nos seus arquivos do jogo
ui-models-geometry_none = Este modelo não tem geometria
ui-models-hardpoints_tooltip = Mostrar pontos de fixação
ui-models-common = Modelos comuns

## ui.models.group
ui-models-group-gameplay = Jogo
ui-models-group-building = Edifícios
ui-models-group-character = Personagens
ui-models-group-location = Locais
ui-models-group-port = Portos
ui-models-group-style = Estilos
ui-models-group-other = Outros modelos

## ui.models.building
ui-models-building-weapon = Armas de edifícios
ui-models-building-unassigned = Edifícios não atribuídos
ui-models-building-unspecified = Edifícios sem tipo

## ui.models.misc
ui-models-misc-crane = Guindastes
ui-models-misc-anchor = Âncoras
ui-models-misc-deck_fitting = Ferragens de convés
ui-models-misc-instrument = Instrumentos
ui-models-misc-propeller = Hélices
ui-models-misc-searchlight = Holofotes
ui-models-misc-boat = Botes
ui-models-misc-flag = Bandeiras
ui-models-misc-insignia = Insígnias
ui-models-misc-railing = Guarda-corpos
ui-models-misc-hatch = Escotilhas
ui-models-misc-ventilation = Ventilação
ui-models-misc-antenna = Antenas
ui-models-misc-weapon = Armas
ui-models-misc-stowage = Armazenamento
ui-models-misc-other = Outros

## ui.models.variant
ui-models-variant-dead = Destroços
ui-models-variant-skin = Visual

## ui.models.kind
ui-models-kind-ship = Navios
ui-models-kind-gun = Canhões
ui-models-kind-director = Diretores de tiro
ui-models-kind-finder = Telêmetros
ui-models-kind-radar = Radares
ui-models-kind-catapult = Catapultas
ui-models-kind-aircraft = Aeronaves
ui-models-kind-projectile = Projéteis
ui-models-kind-misc = Diversos
ui-models-kind-cap = Pontos de captura
ui-models-kind-other = Outros

## ui.models.sub_kind
ui-models-sub_kind-battleship = Encouraçados
ui-models-sub_kind-cruiser = Cruzadores
ui-models-sub_kind-destroyer = Contratorpedeiros
ui-models-sub_kind-aircraft_carrier = Porta-aviões
ui-models-sub_kind-submarine = Submarinos
ui-models-sub_kind-auxiliary = Navios auxiliares
ui-models-sub_kind-main_battery = Bateria principal
ui-models-sub_kind-secondary_battery = Bateria secundária
ui-models-sub_kind-anti_aircraft = Antiaéreo
ui-models-sub_kind-torpedo_tubes = Tubos lança-torpedos
ui-models-sub_kind-depth_charge_launcher = Lançadores de cargas de profundidade
ui-models-sub_kind-missile_launcher = Lança-mísseis
ui-models-sub_kind-search_radar = Radares de busca
ui-models-sub_kind-fire_control_radar = Radares de controle de tiro
ui-models-sub_kind-fighter = Caças
ui-models-sub_kind-bomber = Bombardeiros
ui-models-sub_kind-dive_bomber = Bombardeiros de mergulho
ui-models-sub_kind-scout = Batedores
ui-models-sub_kind-torpedo = Torpedos
ui-models-sub_kind-bomb = Bombas
ui-models-sub_kind-rocket = Foguetes
ui-models-sub_kind-depth_charge = Cargas de profundidade
ui-models-sub_kind-mine = Minas
ui-models-sub_kind-missile = Mísseis
ui-models-sub_kind-jato = Foguetes de decolagem
ui-models-sub_kind-shell = Projéteis de artilharia

## ui.models.part
ui-models-part-hull = Casco
ui-models-part-main_battery = Bateria principal
ui-models-part-secondaries = Secundárias
ui-models-part-anti_aircraft = AA
ui-models-part-torpedoes = Torpedos
ui-models-part-directors = Diretores de tiro
ui-models-part-finders = Telêmetros
ui-models-part-radars = Radares
ui-models-part-depth_charges = Cargas de profundidade
ui-models-part-aircraft = Aeronaves
ui-models-part-other = Outros
ui-models-part-misc = Diversos

## ui.model_view
ui-model_view-thickness_from_building = Espessura de { $building }
ui-model_view-thickness_from = Espessura de { $ship } ({ $hp })
ui-model_view-thickness_unknown = Espessura desconhecida: nenhum navio monta este modelo
ui-model_view-thickness_other_ships = Outros navios
ui-model_view-export_title = Exportar modelo
ui-model_view-export_tooltip = Exportar o modelo para um arquivo GLB
ui-model_view-part_model = Modelo: { $model }
ui-model_view-part_hardpoint = Ponto de fixação: { $hardpoint }

## ui.model_view.part_group
ui-model_view-part_group-hull = Casco
ui-model_view-part_group-main_battery = Bateria principal
ui-model_view-part_group-secondary_battery = Secundárias
ui-model_view-part_group-aa_guns = AA
ui-model_view-part_group-torpedoes = Torpedos
ui-model_view-part_group-depth_charges = Cargas de profundidade
ui-model_view-part_group-fire_control = Controle de tiro
ui-model_view-part_group-radar = Radares
ui-model_view-part_group-missiles = Lança-mísseis
ui-model_view-part_group-decorations = Decorações
ui-model_view-part_group-misc = Diversos
ui-model_view-part_group-armor = Blindagem
ui-model_view-part_group-other = Outros

## ui.scene_problem
ui-scene_problem-cannot_show = Não é possível mostrar { $name }
ui-scene_problem-geometry_absent = A geometria deste modelo não está nos seus arquivos do jogo.
ui-scene_problem-model_unreadable = Não foi possível ler os dados deste modelo. O log contém os detalhes.
ui-scene_problem-ship_unreadable = Não foi possível ler os dados deste navio. O log contém os detalhes.
ui-scene_problem-no_meshes = Este modelo não tem malhas.
ui-scene_problem-no_meshes_at_lod = Este modelo não tem malhas no nível de detalhe escolhido.
ui-scene_problem-no_visible_surface = Este modelo não tem nenhuma superfície visível para desenhar.
ui-scene_problem-section_stand_in = Esta seção do casco não tem geometria própria. O navio desenha esta parte do casco em outra seção.

## ui.armor.option_tabs
ui-armor-option_tabs-label = Abas de opções
ui-armor-option_tabs-ship = Navio
ui-armor-option_tabs-layers = Camadas
ui-armor-option_tabs-analysis = Análise
ui-armor-option_tabs-configuration = Configuração
ui-armor-option_tabs-hull_parts = Partes do casco
ui-armor-option_tabs-tab_matches = { $tab }, { $count } correspondências
ui-armor-option_tabs-empty = Nada nesta aba se aplica ao modelo exibido.
ui-armor-option_tabs-no_choices = Este modelo não tem casco, módulo, camuflagem nem nível de detalhe para escolher.

## ui.armor.hidden_ship
ui-armor-hidden_ship-clan_rental = Aluguel de clã
ui-armor-hidden_ship-other_class = Não é uma classe de navio

## ui.armor.textures
ui-armor-textures-title = Texturas
ui-armor-textures-hint = O jogo lê um arquivo na pasta res_mods da build no lugar do arquivo empacotado no mesmo caminho. Clique com o botão direito em uma textura para copiar esse caminho.
ui-armor-textures-under_pointer = Sob o cursor: { $material }
ui-armor-textures-reading = Lendo texturas...
ui-armor-textures-summary = { $materials ->
    [one] { $materials } material
   *[other] { $materials } materiais
}, { $textures ->
    [one] { $textures } textura
   *[other] { $textures } texturas
}
ui-armor-textures-no_material = Sem material
ui-armor-textures-unresolved = O caminho não está no assets.bin
ui-armor-textures-absent = Não está nos pacotes instalados
ui-armor-textures-unreadable = Não foi possível ler o cabeçalho DDS
ui-armor-textures-unknown_format = Formato não reconhecido
ui-armor-textures-details = { $format }, { $size }, { $count ->
    [one] { $count } arquivo
   *[other] { $count } arquivos
}, { $bytes }
ui-armor-textures-role_albedo = Albedo
ui-armor-textures-role_normal = Normal
ui-armor-textures-role_metal_gloss = Metal-brilho
ui-armor-textures-role_ambient_occlusion = Oclusão de ambiente
ui-armor-textures-role_detail = Detalhe
ui-armor-textures-role_name_matched = Albedo, encontrado pelo nome do arquivo
ui-armor-textures-copy_path = Copiar caminho
ui-armor-textures-copy_overrides = { $count ->
    [one] Copiar caminho res_mods
   *[other] Copiar { $count } caminhos res_mods
}
ui-armor-textures-paths_copied = { $count ->
    [one] { $count } caminho copiado
   *[other] { $count } caminhos copiados
}
ui-armor-textures-no_build = Os dados do jogo não foram carregados, então a pasta de mods da build é desconhecida.
ui-armor-textures-show_in_unpacker = Mostrar no extrator de recursos
ui-armor-textures-show_textures = Mostrar texturas
ui-armor-textures-extract = Extrair arquivos...
ui-armor-textures-extract_title = Extrair arquivos de textura para
ui-armor-textures-extracting = Extraindo arquivos de textura...
ui-armor-textures-extracted = { $count ->
    [one] { $count } arquivo extraído
   *[other] { $count } arquivos extraídos
} para { $folder }
ui-armor-textures-extract_failed = Não foi possível extrair os arquivos de textura: { $reason }
ui-armor-textures-replace_title = Substituir arquivos existentes?
ui-armor-textures-replace_description = { $count ->
    [one] { $count } destes arquivos já está em { $folder }. A extração o substituirá.
   *[other] { $count } destes arquivos já estão em { $folder }. A extração os substituirá.
}
ui-armor-textures-replace = Substituir

## ui.settings.mcp
ui-settings-mcp-enabled = Ativar servidor MCP
ui-settings-mcp-enabled_description = Disponibiliza replays e dados do jogo para assistentes de IA via MCP. Mostra um controle na barra de status.
ui-settings-mcp-start_on_launch = Iniciar com o programa
ui-settings-mcp-start_on_launch_description = Inicia o servidor ao abrir o WoWs Toolkit.
ui-settings-mcp-bind_address = Endereço de escuta
ui-settings-mcp-bind_address_description = Endereço IP de loopback em que o servidor escuta, como 127.0.0.1 ou ::1. Só este computador consegue acessar o servidor.
ui-settings-mcp-port = Porta
ui-settings-mcp-port_description = Porta TCP onde escutar. Escolhida aleatoriamente na primeira ativação e mantida.
ui-settings-mcp-port_unset = Escolhida na primeira ativação
ui-settings-mcp-address_not_loopback = { $input } não é um endereço de loopback. O servidor só escuta neste computador, por exemplo em 127.0.0.1.
ui-settings-mcp-address_invalid = { $input } não é um endereço IP
ui-settings-mcp-port_invalid = { $input } não é uma porta de 1 a 65535
ui-settings-mcp-port_in_use = Outro programa está usando esta porta. Escolha outra porta.

## ui.mcp
ui-mcp-started = Servidor MCP ativo em { $url }
ui-mcp-copy_url = Copiar URL
ui-mcp-start_failed = Não foi possível iniciar o servidor MCP: { $reason }
ui-mcp-port_in_use = a porta { $port } em { $address } está em uso por outro programa
ui-mcp-no_watchdog = o supervisor de tarefas em segundo plano não está em execução
ui-mcp-status_running = Servidor MCP ativo em { $url }. Clique para parar.
ui-mcp-status_starting = Iniciando servidor MCP em { $address }
ui-mcp-status_failed = O servidor MCP não iniciou. Clique para tentar novamente.
ui-mcp-status_stopped = Servidor MCP parado. Clique para iniciar.

## ui.launcher.opt_in
ui-launcher-opt_in-setting = Ativar inicializador do jogo
ui-launcher-opt_in-description = Jogue World of Warships pelo toolkit no Linux, conectado com sua conta Wargaming.
ui-launcher-opt_in-title = Ativar o inicializador do jogo?
ui-launcher-opt_in-notice_account = Ao usar o recurso de inicializador, você reconhece que este recurso não é um cenário suportado pelo desenvolvedor do jogo. Você está digitando sua senha diretamente neste aplicativo, que, caso você não confie nele, poderia ser usado para roubar sua conta (embora o desenvolvedor do aplicativo não deseje fazer isso).
ui-launcher-opt_in-notice_reports = Além disso, você reconhece que, se usar este recurso, não relatará bugs gráficos/de estabilidade ao desenvolvedor do jogo, a menos que realmente acredite que se trata de um problema multiplataforma que NÃO afeta APENAS o Linux.
ui-launcher-opt_in-continue = Continuar
ui-launcher-opt_in-cancel = Cancelar

## ui.launcher.keyring
ui-launcher-keyring-unencrypted = Seu login é salvo sem criptografia na pasta de dados do app porque nenhum provedor Secret Service (como GNOME Keyring, KeePassXC ou KDE Wallet) está disponível.
ui-launcher-keyring-how = Como mantê-lo criptografado
ui-launcher-keyring-how_detail = Instale e inicie um provedor Secret Service como GNOME Keyring, KeePassXC ou KDE Wallet e reinicie o toolkit. Os logins salvos são movidos para ele.
ui-launcher-keyring-unlock_hint = O chaveiro do sistema pode pedir para ser desbloqueado para que o toolkit possa ler seu login salvo.

## ui.launcher.play
ui-launcher-play-play = Jogar
ui-launcher-play-play_through_steam = Jogar pelo Steam
ui-launcher-play-set_up_game = Configurar jogo
ui-launcher-play-set_up_runner = Configurar Proton
ui-launcher-play-sign_in_again = Entrar novamente
ui-launcher-play-update = Atualizar { $size }
ui-launcher-play-updating = Atualizando { $percent }%
ui-launcher-play-updating_unknown = Atualizando
ui-launcher-play-paused = Pausado
ui-launcher-play-starting = Iniciando...
ui-launcher-play-running = Em execução
ui-launcher-play-in_battle = Em batalha
ui-launcher-play-menu = Contas e opções do jogo

## ui.launcher.popover
ui-launcher-popover-game = World of Warships
ui-launcher-popover-no_install = Nenhuma instalação do jogo definida.
ui-launcher-popover-update_available = Atualização disponível
ui-launcher-popover-check_failed = Não foi possível verificar atualizações
ui-launcher-popover-accounts = Contas
ui-launcher-popover-no_accounts = Nenhuma conta ainda. Adicione uma para jogar conectado.
ui-launcher-popover-close_to_switch = Feche o jogo para trocar de conta.
ui-launcher-popover-default = Padrão
ui-launcher-popover-ready = Pronta
ui-launcher-popover-restoring = Verificando...
ui-launcher-popover-not_saved = Não salva
ui-launcher-popover-unencrypted = Salvo sem criptografia
ui-launcher-popover-keyring_locked = Chaveiro bloqueado: desbloqueie-o para restaurar os logins salvos
ui-launcher-popover-forget = Remover
ui-launcher-popover-sign_out = Sair
ui-launcher-popover-expires_in = Expira em { $duration }
ui-launcher-popover-expired = Sessão expirada
ui-launcher-popover-expiry_unknown = Expiração desconhecida
ui-launcher-popover-make_default = Tornar padrão
ui-launcher-popover-account_actions = Ações da conta
ui-launcher-popover-play_anyway = Jogar mesmo assim
ui-launcher-popover-play_safe_mode = Jogar no modo seguro
ui-launcher-popover-add_account = Adicionar conta...
ui-launcher-popover-game_settings = Configurações do jogo...
ui-launcher-popover-installs = Instalações e configuração...
ui-launcher-popover-open_replay = Abrir replay no jogo...

## ui.launcher.palette
ui-launcher-palette-play = Jogar
ui-launcher-palette-play_as = Jogar como { $nickname } ({ $realm })
ui-launcher-palette-game_settings = Configurações do jogo
ui-launcher-palette-add_account = Adicionar conta
ui-launcher-palette-update = Atualizar World of Warships
ui-launcher-palette-open_replay = Abrir replay no jogo

## ui.launcher.installs
ui-launcher-installs-toolkit = Diretório do jogo do toolkit
ui-launcher-installs-played = A conta padrão joga esta
ui-launcher-installs-play_from = Jogar daqui
ui-launcher-installs-update = Atualizar

## ui.launcher.wgc_login
ui-launcher-wgc_login-label = Usar o login da Wargaming
ui-launcher-wgc_login-about = Executa esta instalação do Steam com o login do toolkit em vez do Steam. As configurações e os replays ficam separados das execuções pelo Steam. A carteira do Steam, as conquistas e o tempo de jogo não funcionam neste modo.
ui-launcher-wgc_login-steam_accounts = Contas criadas pelo Steam precisam de um e-mail e uma senha. Adicione-os primeiro no Gerenciamento de Conta da Wargaming.
ui-launcher-wgc_login-steam_accounts_link = Como adicionar um e-mail e uma senha
ui-launcher-wgc_login-ready = Arquivos do Game Center prontos
ui-launcher-wgc_login-fetchable = Ativar esta opção baixa os arquivos do Game Center da WG para esta build ({ $size }).
ui-launcher-wgc_login-downloading = Baixando os arquivos do Game Center: { $percent }%
ui-launcher-wgc_login-downloading_unknown = Baixando os arquivos do Game Center
ui-launcher-wgc_login-checking = Procurando os arquivos do Game Center desta build...
ui-launcher-wgc_login-needs_sign_in = Adicione primeiro uma conta Wargaming ao toolkit.
ui-launcher-wgc_login-unavailable = Indisponível para esta build: { $error }

## ui.launcher.replay
ui-launcher-replay-no_install = Nenhuma instalação tem a build { $build } do jogo. Mantenha ou instale uma cópia dessa build para assistir a este replay.

## ui.launcher.setup
ui-launcher-setup-title = Configurar World of Warships
ui-launcher-setup-game = Jogo
ui-launcher-setup-proton = Proton
ui-launcher-setup-accounts = Contas
ui-launcher-setup-replays = Replays
ui-launcher-setup-game_question = Qual cópia do jogo o toolkit deve usar?
ui-launcher-setup-none_found = Nenhuma instalação foi encontrada neste computador.
ui-launcher-setup-choose_folder = Escolher uma pasta...
ui-launcher-setup-install_new = Instalar uma nova cópia...
ui-launcher-setup-proton_explain = No Linux, o jogo roda pelo Proton: o do Steam quando o Steam está instalado, caso contrário o umu-run.
ui-launcher-setup-proton_ready = Proton encontrado.
ui-launcher-setup-accounts_explain = Entre nas contas Wargaming com que você joga. Jogar conecta o jogo com a conta padrão.
ui-launcher-setup-replays_explain = O jogo exclui os replays mais antigos quando atinge esta quantidade.
ui-launcher-setup-skip = Pular configuração
ui-launcher-setup-back = Voltar
ui-launcher-setup-next = Avançar
ui-launcher-setup-done = Concluir

## ui.launcher.game_settings
ui-launcher-game_settings-title = Configurações do jogo
ui-launcher-game_settings-build = build { $build }
ui-launcher-game_settings-display = Tela
ui-launcher-game_settings-graphics = Gráficos
ui-launcher-game_settings-sound = Som
ui-launcher-game_settings-replays = Replays
ui-launcher-game_settings-locked = O jogo está em execução. Ele reescreve estes arquivos ao fechar, então eles ficam somente leitura até lá.
ui-launcher-game_settings-footer = Gravado nos arquivos de preferências do jogo. Um backup é mantido.
ui-launcher-game_settings-cancel = Cancelar
ui-launcher-game_settings-save = Salvar
ui-launcher-game_settings-saved = Configurações do jogo salvas.
ui-launcher-game_settings-no_install = Nenhuma instalação do jogo definida.
ui-launcher-game_settings-window_mode = Modo de janela
ui-launcher-game_settings-windowed = Janela
ui-launcher-game_settings-borderless = Sem bordas
ui-launcher-game_settings-fullscreen = Tela cheia
ui-launcher-game_settings-monitor = Monitor
ui-launcher-game_settings-monitor_name = Nome do monitor
ui-launcher-game_settings-resolution = Resolução em tela cheia
ui-launcher-game_settings-width = Largura
ui-launcher-game_settings-height = Altura
ui-launcher-game_settings-refresh = Taxa de atualização
ui-launcher-game_settings-display_rate = Taxa de exibição
ui-launcher-game_settings-vsync = Sincronização vertical
ui-launcher-game_settings-ui_scale = Escala da interface
ui-launcher-game_settings-battle_fps = Limite de FPS em batalha
ui-launcher-game_settings-port_fps = Limite de FPS no porto
ui-launcher-game_settings-unlimited = Ilimitado
ui-launcher-game_settings-skip_intro = Pular vídeo de introdução
ui-launcher-game_settings-master = Volume geral
ui-launcher-game_settings-music_port = Música no porto
ui-launcher-game_settings-music_battle = Música em batalha
ui-launcher-game_settings-preset = Predefinição
ui-launcher-game_settings-antialiasing = Antisserrilhamento
ui-launcher-game_settings-textures = Qualidade das texturas
ui-launcher-game_settings-filtering = Filtragem de texturas
ui-launcher-game_settings-shadows = Qualidade das sombras
ui-launcher-game_settings-graphics_note = Altere os gráficos no jogo. Ele os verifica de acordo com o seu hardware.
ui-launcher-game_settings-replays_kept = Replays a manter
ui-launcher-game_settings-replays_kept_zero = Mantenha pelo menos um replay.
ui-launcher-game_settings-versioned = Ordenar replays por versão do jogo
ui-launcher-game_settings-replays_note = Gravado na pasta res_mods desta build. O modo seguro não a usa.
ui-launcher-game_settings-absent = Não definido nos arquivos do jogo
ui-launcher-game_settings-unrecognized = Valor desconhecido: { $value }
ui-launcher-game_settings-not_a_number = { $field }: digite um número.

## ui.settings.launcher
ui-settings-launcher-installs = Instalações do jogo
ui-settings-launcher-installs_description = As instalações do jogo que o inicializador executa e atualiza.
ui-settings-launcher-runner = Proton (Linux)
ui-settings-launcher-runner_description = Como o jogo roda no Linux. O Proton Experimental ou o Proton mais recente do Steam é encontrado automaticamente; caso contrário, é usado o umu-run.
ui-settings-launcher-launch_args = Argumentos de inicialização
ui-settings-launcher-launch_args_description = Argumentos de linha de comando extras para o jogo, um por linha.
ui-settings-launcher-environment = Ambiente
ui-settings-launcher-environment_description = Variáveis de ambiente para o jogo, KEY=VALUE, uma por linha.
ui-settings-launcher-safe_mode_args = Argumentos do modo seguro
ui-settings-launcher-safe_mode_args_description = Argumentos adicionados ao jogar no modo seguro, um por linha.
