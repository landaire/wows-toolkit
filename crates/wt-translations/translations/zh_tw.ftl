# You are encouraged to suggest edits to this file on GitHub by either filing
# an issue, editing the file directly and creating a pull request, or just
# mentioning it in the Discord.
## meta
meta-language_name = 繁體中文

# -- 語言選擇對話框 --

## dialog
dialog-continue_in_english = 以英文繼續
dialog-continue_in_language = 繼續使用繁體中文
dialog-select_language = 選擇語言
dialog-machine_translation_warning = 大部分翻譯為機器生成，可能不夠準確。

# -- 分頁標題 --

## ui.tabs
ui-tabs-launcher = 啟動器
ui-tabs-unpacker = 資源解包器
ui-tabs-settings = 設定
ui-tabs-replay_parser = 戰鬥回放檢視器
ui-tabs-replay_directory = 回放目錄
ui-tabs-player_tracker = 玩家追蹤
ui-tabs-mod_manager = 模組管理器
ui-tabs-armor_viewer = 裝甲檢視器
ui-tabs-model_viewer = 模型檢視器
ui-tabs-stats = 統計
ui-tabs-directory_stats = { $dir } 統計
ui-tabs-close_directory_stats = 關閉目錄統計
ui-tabs-search = 搜尋
ui-tabs-search_these_replays = 搜尋這些回放
ui-tabs-search_these_replays_unavailable = 這些回放尚未建立索引
ui-tabs-zoom_in_panel = 放大
ui-tabs-zoom_out_panel = 縮小
ui-tabs-close_panel = 關閉
ui-tabs-close_tab = 關閉
ui-tabs-close_other_tabs = 關閉其他
ui-tabs-close_tabs_to_the_right = 關閉右側分頁

# -- 常用按鈕 --

## ui.buttons
ui-buttons-yes = 是
ui-buttons-no = 否
ui-buttons-send_replays = 傳送回放
ui-buttons-send_build_data = 僅傳送配裝資料
ui-buttons-send_nothing = 不傳送任何資料
ui-buttons-switch_to_replays = 改為傳送回放
ui-buttons-keep_current = 保留目前設定
ui-buttons-stop = 停止
ui-buttons-copy = 複製
ui-buttons-cancel = 取消
ui-buttons-continue_ = 繼續
ui-buttons-install_update = 安裝更新
ui-buttons-view_release = 查看版本
ui-buttons-clear_settings = 清除設定
ui-buttons-more_options = 更多選項
ui-buttons-create_issue = 建立 Issue
ui-buttons-discord = Discord
ui-buttons-github = GitHub
ui-buttons-view_github = 在 GitHub 上查看專案
ui-buttons-choose = 選擇...
ui-buttons-reset = 重置
ui-buttons-select_all = 全選
ui-buttons-download = 下載
ui-buttons-dismiss = 關閉
ui-buttons-retry = 重試
ui-buttons-check_all = 全部勾選
ui-buttons-uncheck_all = 全部取消勾選
ui-buttons-refresh_persisted_data_confirm = 重新整理

# -- 選單項目 --

## ui.menu
ui-menu-file = 檔案
ui-menu-check_updates = 檢查更新
ui-menu-about = 關於
ui-menu-quit = 結束
ui-menu-licenses = 授權條款

# -- 視窗標題 --

## ui.windows
ui-windows-build_consent = 配裝收集同意
ui-windows-replay_migration = 改為分享回放
ui-windows-crash_detected = 偵測到應用程式當機
ui-windows-update_available = 有可用更新
ui-windows-confirm = 確認
ui-windows-network_warning = 網路警告
ui-windows-download_game_data = 下載遊戲資料
ui-windows-error = 錯誤
ui-windows-software_encode = 沒有 GPU 影片編碼器
ui-windows-about = 關於
ui-windows-refresh_persisted_data = 重新整理已儲存的回放資料
ui-windows-tactics_board = 戰術板
ui-windows-minimize = 最小化
ui-windows-maximize = 最大化
ui-windows-restore = 還原
ui-windows-close = 關閉
ui-windows-licenses = 授權條款

# -- 對話框 / 訊息文字 --

## ui.dialogs
ui-dialogs-build_consent_message = 您是否願意將排位戰和隨機戰中的玩家配裝資訊傳送給開發者？此資料收集有助於社群批量分析玩家配裝。您可以隨時在設定中退出。
ui-dialogs-replay_migration_message = WoWs Toolkit 現在可以分享完整回放，這比單純的配裝資料能提供豐富得多的配對分析。您使用測試艦的戰鬥不會被分享。您是否要改為分享原始回放？
ui-dialogs-crash_message = WoWs Toolkit 上次執行時似乎發生了當機。如果您想回報此問題，請在 GitHub 上建立 Issue 或加入 Discord 伺服器，並提供以下資訊。
ui-dialogs-crash_clear_settings = 如果應用程式的持久化資料可能是問題的原因，您可以按下面的按鈕嘗試清除設定。這將清除所有設定。追蹤的玩家會保留。您的戰鬥回放和 WoWs 資料將保持安全。
ui-dialogs-update_message = WoWs Toolkit 版本 { $tag } 已可用
ui-dialogs-update_notes = 版本資訊
ui-dialogs-update_asset_pending = Windows 版下載尚不可用。請檢視版本頁面瞭解組建狀態。
ui-dialogs-update_windows_only = 有可用更新，但目前僅支援 Windows。
ui-dialogs-p2p_warning = 協作工作階段使用 P2P 網路。工作階段中的其他使用者可能能夠看到您的 IP 位址。
ui-dialogs-software_encode_message = 未找到受支援的 GPU 影片編碼器，因此本次匯出將使用軟體編碼。速度會慢很多，較長的戰鬥可能需要幾分鐘。
ui-dialogs-download_game_data_intro = 部分回放需要尚未安裝的遊戲資料。請選擇要下載的組建。
ui-dialogs-download_build_row = 版本 { $version }（組建 { $build }）
ui-dialogs-download_replays_needing = { $count } 個回放需要
ui-dialogs-download_availability_resolving = 正在檢查儲存庫
ui-dialogs-download_availability_exact = 已發布
ui-dialogs-download_availability_nearest = 沒有完全相符，最接近的是 { $version }（組建 { $build }）
ui-dialogs-download_availability_unpublished = 從未發布
ui-dialogs-download_availability_unreachable = 已發布，但無法讀取其檔案清單
ui-dialogs-download_plan_pending = 正在確認需要取得哪些內容...
ui-dialogs-download_objects_to_fetch = 已勾選組建需要取得的物件數：{ $count }
ui-dialogs-download_plan_failed = 無法確認需要取得哪些內容
ui-dialogs-download_plan_no_cache_dir = 尚未設定遊戲資料快取目錄。請先在設定中選擇一個，然後再試一次。
ui-dialogs-refresh_persisted_data_message = 重新解析回放目錄中的每個回放並重寫其儲存的資料。使用與其遊戲組建不匹配的回放資料對應記錄的條目將保持不變，但所有回放的個人評分和遊戲模式都會按當前的期望值重新計算。這可能需要一些時間。
ui-dialogs-file_dialog_failed = 無法開啟檔案對話方塊：{ $reason }

# -- 標籤 --

## ui.labels
ui-labels-error_occurred = 發生了一個錯誤：
ui-labels-made_by = 由 landaire 開發。
ui-labels-credits = 感謝 Trackpad、TTaro、lkolbly 的貢獻。
ui-labels-pr_credits = 個人評分 (PR) 的計算資料和公式由 WoWs Numbers 提供。
ui-labels-powered_by = 採用{" "}
ui-labels-and = {" "}和{" "}
ui-labels-suppress_warning = 不再顯示
ui-labels-more_info = 更多資訊
ui-labels-loading_replays = 正在載入 { $count } 個戰鬥回放...
ui-labels-app_version = 版本 { $version }
ui-labels-project_license = WoWs Toolkit 依據 { $license } 授權條款散布。
ui-labels-third_party_notices = 第三方聲明
ui-labels-notices_search_hint = 搜尋聲明
ui-labels-no_matching_notices = 沒有符合搜尋的聲明。

# -- 通知訊息 --

## ui.messages
ui-messages-bug_report_info_copied = 已複製錯誤回報資訊。
ui-messages-update_downloading = 正在下載更新：{ $read } / { $total }
ui-messages-update_downloading_unknown = 正在下載更新：已下載 { $read }
ui-messages-app_up_to_date = 應用程式已是最新版本
ui-messages-checking_app_updates = 正在檢查應用程式更新...
ui-messages-update_starting = 正在下載更新...
ui-messages-update_check_failed = 無法檢查應用程式更新
ui-messages-constants_fetch_failed = 無法獲取更新的回放資料對應。稍後將重試。
ui-messages-game_data_loaded = 成功載入遊戲資料
ui-messages-no_replays_detected = 未偵測到戰鬥回放 — 您的 WoWs 目錄是否正確設定？
ui-messages-build_loaded = 已載入配裝 { $build }
ui-messages-downloading_game_data = 正在下載遊戲資料...
ui-messages-game_data_downloaded = 已下載組建 { $build } 的遊戲資料
ui-messages-game_data_builds_downloaded = 已下載 { $count } 個組建的遊戲資料
ui-messages-game_data_download_failed = 無法下載遊戲資料
ui-messages-game_data_build_download_failed = 無法下載組建 { $build } 的遊戲資料
ui-messages-checking_game_data_updates = 正在檢查遊戲資料更新...
ui-messages-game_data_up_to_date = 遊戲資料已是最新
ui-messages-game_data_updates_available = { $count } 個快取組建有更新的資料可用
ui-messages-validating_game_data = 正在驗證遊戲資料快取...
ui-messages-game_data_cache_valid = 遊戲資料快取有效，且與遠端儲存庫相符
ui-messages-game_data_cache_invalid = { $count } 個快取組建已過期或損毀
ui-messages-replay_loaded = 成功載入戰鬥回放
ui-messages-replay_load_failed = 載入回放失敗
ui-messages-twitch_token_invalid = Twitch 權杖無效或已過期。請在設定中更新。
ui-messages-wows_dir_invalid = World of Warships 目錄無效。請在設定中檢查路徑。
ui-messages-wows_dir_not_set = 尚未設定 World of Warships 目錄
ui-messages-twitch_token_expired = Twitch 權杖無效或已過期
ui-messages-versioned_constants_rebuild_failed = 重建期間無法獲取版本化常數。稍後將重試。
ui-messages-constants_version_mismatch =
    回放資料對應檔案版本與遊戲版本不符。
    戰後結果可能不準確。請耐心等待專案維護者更新伺服器上的對應。
ui-messages-constants_updated = 回放資料對應檔案已成功更新
ui-messages-constants_imported = 已安裝組建 { $build } 的回放資料對應
ui-messages-constants_imported_other_build = 已安裝組建 { $build } 的回放資料對應，但它不是當前載入的組建（{ $loaded }）
ui-messages-constants_import_failed = 無法匯入回放資料對應：{ $error }
ui-messages-session_started = 工作階段已開始
ui-messages-user_joined = { $name } 已加入
ui-messages-user_timeout = { $name } 已逾時
ui-messages-user_left = { $name } 已離開
ui-messages-session_ended = 工作階段已結束
ui-messages-session_error = 工作階段錯誤：{ $msg }
ui-messages-connected_to_session = 已連線至工作階段
ui-messages-joined_session = 已加入工作階段：{ $name }
ui-messages-replay_spam_protection = 已斷線：主持人開啟回放過快
ui-messages-host_opened_replay = 主持人已開啟回放：{ $name }
ui-messages-host_closed_replay = 主持人已關閉一個回放
ui-messages-session_error_generic = 工作階段：{ $msg }
ui-messages-session_rejected = 工作階段被拒絕：{ $reason }
ui-messages-indexing_replays = 正在建立回放索引...
ui-messages-indexing_replays_progress = 正在建立回放索引（{ $done }/{ $total }）
ui-messages-replays_indexed = 已為 { $total } 個回放中的 { $indexed } 個建立索引
ui-messages-replays_already_indexed = 全部 { $total } 個回放皆已建立索引
ui-messages-loading_row_summaries = 正在載入回放詳細資料...
ui-messages-reading_replay_directory = 正在讀取回放目錄...
ui-messages-directory_replays_skipped = 已略過 { $skipped } 個無法讀取的回放
ui-messages-drop_not_a_replay = 這不是回放檔案
ui-messages-drop_not_replays = 這些不是回放檔案
ui-messages-drop_skipped = 已略過 { $count } 個不是回放或資料夾的拖放檔案
ui-messages-directory_replays_not_indexed = 已列出 { $count } 個回放，但沒有比賽詳細資料
ui-messages-log_copied = 已將 { $file } 中的 { $bytes } KB 複製到剪貼簿
ui-messages-log_copy_failed = 無法讀取日誌檔案：{ $error }
ui-messages-log_not_found = 在 { $dir } 中未找到日誌檔案

# -- 統計名稱 --

## stat
stat-damage = 傷害
stat-spotting_damage = 偵察傷害
stat-frags = 擊沉
stat-raw_xp = 原始 XP
stat-base_xp = 基礎 XP
stat-win_rate = 勝率
stat-win_rate_pct = 勝率 (%)
stat-personal_rating = 個人評分
stat-match_contribution = 戰鬥貢獻
stat-capture_points = 佔領點數
stat-planes_shot_down = 擊落飛機
stat-anti_air_damage = 防空傷害
stat-survival_rate = 存活率
stat-torpedoes_spotted = 發現魚雷
stat-avg_damage = 平均傷害
stat-avg_spotting_damage = 平均偵察傷害
stat-avg_frags = 平均擊沉
stat-avg_raw_xp = 平均原始 XP
stat-avg_base_xp = 平均基礎 XP
stat-avg_pr = 平均 PR
stat-avg_mc = 平均 MC
stat-avg_prefix = 平均 { $name }

## chart
chart-game_number = 場次 #
chart-combined = 合計
chart-combined_suffix = (合計)
chart-rolling_average_suffix = (移動平均)
chart-no_data = 所選艦船無可用資料
chart-win_rate_unavailable = 勝率不適用於每場折線圖（請啟用移動平均或使用長條圖）

# -- 確認對話框 --

## confirm
confirm-open_in_game = 這將啟動 World of Warships。是否繼續？
confirm-clear_all_session_stats = 這將清除所有工作階段統計。是否繼續？
confirm-clear_ship_session_stats = 這將從工作階段統計中移除此艦船的所有場次。是否繼續？
confirm-set_as_session_stats = 這將取代您目前的工作階段統計。是否繼續？

# -- 錯誤訊息 --

## error
error-file_watcher_creation = 無法建立檔案監視器：{ $error }
error-replay_dir_watch = 無法監視回放目錄：{ $error }

# -- 設定 --

## ui.settings.app
ui-settings-app-heading = 應用程式設定
ui-settings-app-description = 一般應用程式行為和外觀
ui-settings-app-check_for_updates = 啟動時檢查更新
ui-settings-app-enable_logging = 啟用日誌記錄（重新啟動後生效）
ui-settings-app-enable_logging_tooltip = 將偵錯日誌寫入應用程式資料目錄中的檔案。對疑難排解很有用。
ui-settings-app-data_sharing_mode = 資料分享（排位戰與隨機戰）
ui-settings-app-data_sharing_off = 不傳送任何資料
ui-settings-app-data_sharing_build_data = 將配裝資料傳送至 ShipBuilds.com
ui-settings-app-data_sharing_replays = 將回放傳送至 ShipBuilds.com
ui-settings-app-zoom_factor = 縮放比例（Ctrl + 和 Ctrl - 也可更改此設定）
ui-settings-app-theme = 佈景主題
ui-settings-app-theme_system = 系統
ui-settings-app-theme_dark = 深色
ui-settings-app-theme_light = 淺色
ui-settings-app-code_integrity = 封鎖未簽署的程式碼（重新啟動後生效）
ui-settings-app-code_integrity_tooltip = 拒絕載入未經 Microsoft 簽署的程式碼，可將覆蓋層、擷取工具與音訊公用程式擋在本應用程式之外。這些是渲染時當機與凍結的常見原因。顯示卡驅動程式已透過 Windows 簽署，仍可正常運作。工作階段開始後即無法關閉此功能，因此變更會在下次啟動時生效。「自動」會在您安裝了第三方輸入法時略過此功能，否則該輸入法會被封鎖。請注意，無論此設定為何，其他防護措施都會維持啟用；若要全部關閉，請以 --no-hardening 執行。
ui-settings-app-code_integrity_automatic = 自動
ui-settings-app-code_integrity_always = 永遠
ui-settings-app-code_integrity_never = 永不
ui-settings-app-language = 語言
ui-settings-app-open_data_dir = 開啟應用程式資料資料夾
ui-settings-app-open_data_dir_tooltip = 在檔案管理器中開啟應用程式資料資料夾（設定、日誌、快取資料）。
ui-settings-app-proxy_url = HTTP 代理
ui-settings-app-proxy_url_hint = host:port，留空則使用系統代理設定（重啟後生效）
ui-settings-app-proxy_url_hint_live = host:port，留空則使用系統代理設定

## ui.settings.wows
ui-settings-wows-heading = World of Warships 設定
ui-settings-wows-description = World of Warships 安裝路徑
ui-settings-wows-directory_hint = World of Warships 目錄
ui-settings-wows-browse = 瀏覽...

## ui.settings.wows.cache
ui-settings-wows-cache-heading = 遊戲資料快取
ui-settings-wows-cache-description = 已轉儲的遊戲資料，使舊組建的回放仍可開啟
ui-settings-wows-cache-auto_dump = 快取遊戲資料以相容舊回放
ui-settings-wows-cache-directory_label = 快取目錄：
ui-settings-wows-cache-browse = 瀏覽
ui-settings-wows-cache-open_folder = 開啟資料夾
ui-settings-wows-cache-delete_old = 刪除舊版本
ui-settings-wows-cache-stats = 遊戲資料快取：{ $count }個版本共{ $size }
ui-settings-wows-cache-check_updates = 檢查更新
ui-settings-wows-cache-actions = 快取操作
ui-settings-wows-cache-actions_tooltip = 檢查更新或校驗已快取的組建
ui-settings-wows-cache-update_all = 全部更新
ui-settings-wows-cache-updates_available = { $count } 個快取組建有更新的資料可用
ui-settings-wows-cache-validate = 驗證快取
ui-settings-wows-cache-validate_tooltip = 比對遠端儲存庫檢查每個快取組建，並驗證檔案完整性
ui-settings-wows-cache-repair = 修復
ui-settings-wows-cache-repair_needed = { $count } 個快取組建已過期或損毀

## ui.task
ui-task-batch_render_progress = 正在渲染 { $current }/{ $total }：{ $name }

## ui.settings.replay
ui-settings-replay-heading = 回放設定
ui-settings-replay-description = 設定回放結果表格中顯示的欄位
ui-settings-replay-show_raw_xp = 顯示原始 XP
ui-settings-replay-show_entity_id = 顯示實體 ID
ui-settings-replay-show_observed_damage = 顯示觀測傷害
ui-settings-replay-enable_previews = 滑鼠停留時顯示小地圖預覽
ui-settings-replay-enable_previews_tooltip = 將滑鼠停留在回放上時，會在背景讀取並重播該回放，以建立循環播放的小地圖預覽。若儲存裝置速度較慢，請關閉此功能。
ui-settings-replay-show_fires = 顯示火災
ui-settings-replay-show_floods = 顯示進水
ui-settings-replay-show_citadels = 顯示核心區命中
ui-settings-replay-show_crits = 顯示模組關鍵命中
ui-settings-replay-auto_export_data = 自動匯出資料
ui-settings-replay-format_json = JSON
ui-settings-replay-format_csv = CSV
ui-settings-replay-format_cbor = CBOR
ui-settings-replay-export_path_hint = 資料自動匯出目錄
ui-settings-replay-export_path_missing = 該目錄不存在，無法匯出資料。
ui-settings-replay-index_all_replays = 為所有回放建立索引
ui-settings-replay-reindex_all_replays = 重新為所有回放建立索引
ui-settings-replay-reindex_all_replays_hover = 重新解析並重新索引每個回放，包括已建立索引的回放。當更新新增了索引資料（例如個人評分、斷線狀態）時，可用此功能將資料回填至既有項目。
ui-settings-replay-show_heals = 顯示維修
ui-settings-replay-current_path = 當前回放路徑

## ui.settings.session
ui-settings-session-heading = 工作階段設定
ui-settings-session-description = 協作回放工作階段偏好設定
ui-settings-session-display_name = 顯示名稱
ui-settings-session-suppress_ip_warning = 不顯示 P2P IP 位址警告
ui-settings-session-ip_warning_tooltip = 協作工作階段使用 P2P 網路。工作階段中的其他使用者可能能夠看到您的 IP 位址。
ui-settings-session-disable_auto_open = 停用自動開啟工作階段視窗
ui-settings-session-auto_open_tooltip = 啟用後，工作階段視窗（回放、戰術板）不會自動開啟。您仍然可以從工作階段彈出視窗中開啟它們。

## ui.settings.twitch
ui-settings-twitch-heading = Twitch 設定
ui-settings-twitch-description = 連接 Twitch 以進行聊天整合和玩家追蹤
ui-settings-twitch-get_token = 取得登入權杖
ui-settings-twitch-get_token_tooltip = 我們使用 Chatterino 的登入頁面，因為它提供具有必要權限的權杖（基本上是具有聊天權限的管理員權杖），並且不需要 WoWs Toolkit 開發者架設自己的登入頁面網站以達到相同的效果。
ui-settings-twitch-paste_token_no_token = 貼上權杖（未設定權杖）
ui-settings-twitch-paste_token_tooltip = 從剪貼簿讀取憑據
ui-settings-twitch-paste_token_valid = 貼上權杖（權杖有效）
ui-settings-twitch-paste_token_unvalidated = 正在檢查權杖
ui-settings-twitch-paste_token_invalid = 貼上權杖（權杖無效 / 已過期）
ui-settings-twitch-monitored_channel = 監控的頻道（預設為自己）
ui-settings-twitch-signed_in_as = 已登入為 { $who }

# -- 回放檢視器 --

## ui.replay
ui-replay-copy_bug_report_info = 複製錯誤回報資訊
ui-replay-collapse_listing = 收合回放清單
ui-replay-expand_listing = 展開回放清單
ui-replay-match_age = { $age }前
ui-replay-listing_caption = 回放
ui-replay-listing_reading = 正在讀取 { $done } / { $total }...
ui-replay-listing_scanning = 正在掃描 { $done } / { $total }...
ui-replay-listing_downloading = 正在下載遊戲資料，{ $done } / { $total } 個檔案...
ui-replay-listing_loading_data = 正在載入 { $version } 的遊戲資料（{ $index } / { $count }）...
ui-replay-loading_game_data_version = 正在載入 { $version } 的遊戲資料...
ui-replay-spectator = 觀戰者
ui-replay-loading = 載入中...
ui-replay-preview_loading = 正在建立預覽...
ui-replay-preview_empty = 沒有可顯示的移動
ui-replay-preview_no_game_data = 沒有用戶端版本 { $value } 的遊戲資料
ui-replay-preview_unreadable = 無法讀取此回放
ui-replay-preview_unreadable_because = 無法讀取此回放: { $value }
ui-replay-preview_unwalkable = 無法重播此回放的戰鬥：{ $value }
ui-replay-preview_unknown_version = 此回放回報了無法讀取的用戶端版本：{ $value }
ui-replay-preview_no_map = 沒有 { $value } 的地圖資料
ui-replay-preview_cancelled = 預覽已被新的預覽取代
ui-replay-preview_render_failed = 無法繪製預覽：{ $value }
ui-replay-no_selection = 點擊回放以檢視，或雙擊在新分頁中開啟
ui-replay-workspace_closed = 此回放工作區已關閉
ui-replay-incomplete_results = 不完整的比賽結果
ui-replay-incomplete_results_tooltip = 回放尚無比賽結束結果。比賽結束並將結果加入回放後，資料將自動重新載入。
ui-replay-no_chat = 此回放中沒有發送聊天訊息
ui-replay-chat_copy_all = 複製全部
ui-replay-chat_save_to_file = 儲存到檔案
ui-replay-chat_save_title = 儲存遊戲聊天
ui-replay-export_results_title = 匯出結果
ui-replay-results_exported = 結果已匯出
ui-replay-chat_saved = 聊天已儲存到 { $path }
ui-replay-chat_save_failed = 聊天未能儲存：{ $error }
ui-replay-team_damage = 隊友傷害：
ui-replay-team_share = 佔隊伍總計的 { $pct }（{ $team }）
ui-replay-hide_my_stats = 隱藏我的測試艦統計
ui-replay-open_manually = 手動開啟回放檔案...
ui-replay-open_directory = 開啟回放目錄...
ui-replay-search_these_replays = 搜尋這些回放
ui-replay-reading_directory = 正在列出 { $dir } 中的回放
ui-replay-import_constants = 匯入 constants.json 檔案...
ui-replay-refresh_persisted_data = 重新整理已儲存的回放資料...
ui-replay-contributing = 正在提交回放 { $done } / { $total }
ui-replay-contributed = 已提交 { $sent } / { $total } 個回放
ui-replay-contribute_nothing_shared = 請先在設定中選擇要共享的內容
ui-replay-contribute_none_listed = 沒有可提交的回放
ui-replay-contribute_not_ready = 遊戲資料與設定載入完成後即可提交回放
ui-replay-contribute_already_running = 提交任務已在執行
ui-replay-auto_export_failed = 無法匯出一場戰鬥：{ $error }
ui-replay-auto_exported = 已將戰鬥匯出至 { $file }
ui-replay-opened_new_battle = 已開啟新戰鬥：{ $battle }
ui-replay-new_battle_listed = 新戰鬥：{ $battle }
ui-replay-results_arrived = { $battle } 的戰鬥結果已送達
ui-replay-constants_written = 已下載 { $count } 個組建的結果對應
ui-replay-constants_import_title = 匯入 constants.json
ui-replay-constants_imported = 已匯入組建 { $build } 的結果對應
ui-replay-constants_import_failed = 未能匯入該對應：{ $reason }
ui-replay-constants_no_build = 請先載入一個遊戲組建
ui-replay-constants_latest_written = 已下載最新的結果對應
ui-replay-constants_check_failed = 無法檢查結果對應：{ $reason }
ui-replay-constants_build_label = 組建 { $build }
ui-replay-constants_generating = 正在更新 { $version } 的遊戲常數...
ui-replay-constants_generated = 已在 { $seconds } 秒內產生 { $version } 的遊戲常數
ui-replay-constants_generation_failed = 無法更新 { $version } 的遊戲常數：{ $reason }。{ $fallback }
ui-replay-constants_fallback_kept = 保留已為其儲存的常數。
ui-replay-constants_fallback_published = 使用為組建 { $build } 發布的常數。
ui-replay-constants_fallback_embedded = 使用內建常數。
ui-replay-copy_latest_log = 將最新日誌複製到剪貼簿
ui-replay-load_alt_perspective = 載入另一隊的視角...
ui-replay-load_alt_perspective_tooltip = 將其他玩家錄製的同一場比賽合併至目前的回放。可顯示另一視角中的敵方位置、配裝、魚雷、砲擊與消耗品。
ui-replay-load_alt_perspective_disabled_tooltip = 請先開啟或聚焦一個回放。
ui-replay-load_alt_perspective_failed = 合併回放失敗：{ $error }
ui-replay-load_alt_perspective_limit = 這場戰鬥已在透過 { $limit } 個其他錄影讀取
ui-replay-consumable_charges_remaining = 剩餘 { $remaining } 次，共 { $total } 次
ui-replay-consumable_charges_unlimited = 已使用 { $used } 次（次數無限）
ui-replay-consumable_header_consumable = 消耗品
ui-replay-consumable_header_remaining = 剩餘
ui-replay-consumable_header_total = 總數
ui-replay-consumable_charges_infinite = ∞
ui-replay-autoload_latest = 自動載入最新回放
ui-replay-autoload_latest_tooltip = 遊戲寫入完成後立即開啟比賽
ui-replay-anonymize_names = 匿名化玩家名稱
ui-replay-anonymize_names_tooltip = 以產生的名稱取代玩家名稱與軍團標籤顯示。匯出會保留真實名稱。
ui-replay-group_win_rate_all_modes = 分組勝率統計所有遊戲模式
ui-replay-group_win_rate_all_modes_tooltip = 預設情況下，分組標題僅統計隨機戰和排位戰。開啟後會統計所有遊戲模式，且勝率不再著色，因為顏色是依隨機戰校準的。
ui-replay-select_replay = 選擇回放
ui-replay-drop_hint = 將回放拖放到此處即可開啟，拖放資料夾則列出其中的回放
ui-replay-scanning = 正在掃描回放...
ui-replay-scanning_count = 正在讀取回放 { $done } / { $total }
ui-replay-no_replays_in = { $folder } 中沒有回放
ui-replay-drop_folder_hint = 將資料夾拖放到視窗即可在其專屬分頁中列出
ui-replay-new_tag = 新
ui-replay-loading_game_data = 正在載入遊戲資料...
ui-replay-game_data_failed = 遊戲資料載入失敗：{ $error }
ui-replay-group_label = 回放分組
ui-replay-column_filters = 欄位篩選器
ui-replay-row_elements = 列顯示內容
ui-replay-row_element_personal_rating = 個人評分
ui-replay-row_element_match_contribution = 比賽貢獻
ui-replay-row_element_division = 分隊
ui-replay-row_element_twitch = Twitch 聊天室
ui-replay-row_element_damage = 傷害
ui-replay-row_element_kills = 擊沉
ui-replay-row_element_time = 戰鬥時間
ui-replay-listing_options = 列表選項
ui-replay-open = 開啟
ui-replay-open_file = 開啟回放檔案...
ui-replay-check_game_data = 檢查遊戲資料
ui-replay-check_game_data_tooltip = 檢查此目錄中的回放所需的遊戲資料
ui-replay-close_side_panel = 關閉面板
ui-replay-nda = NDA
ui-replay-render = 渲染
ui-replay-chat = 聊天
ui-replay-game_chat = 遊戲聊天
ui-replay-copy_all = 全部複製
ui-replay-save_to_file = 儲存至檔案
ui-replay-chat_copied = 聊天已複製
ui-replay-copy_message = 複製訊息
ui-replay-actions = 操作
ui-replay-row_actions_hint = 玩家操作（或右鍵點擊該行）
ui-replay-section_match = 比賽
ui-replay-section_export = 匯出
ui-replay-section_debug = 偵錯
ui-replay-timeline = 比賽時間軸
ui-replay-row_not_indexed = 未建立索引
ui-replay-hover_survived = 存活
ui-replay-hover_sunk = 已沉沒
ui-replay-hover_not_indexed = 未建立索引
ui-replay-row_kills = { $count } 擊沉
ui-replay-row_survived = 存活
ui-replay-row_sunk = 已沉沒
ui-replay-row_division = 分隊：{ $members }
ui-replay-export_chat_save = 聊天：儲存至檔案
ui-replay-export_chat_copy = 聊天：複製
ui-replay-export_results_json = 結果：JSON
ui-replay-export_results_cbor = 結果：CBOR
ui-replay-export_results_csv = 結果：CSV
ui-replay-timeline_filter = 篩選
ui-replay-timeline_filter_all = 全部
ui-replay-timeline_filter_none = 無
ui-replay-timeline_search_hint = 搜尋艦船或玩家
ui-replay-timeline_no_events = 未偵測到重要事件。
ui-replay-timeline_no_matches = 沒有事件符合目前的篩選條件。
ui-replay-timeline_parsing = 正在解析事件...
ui-replay-timeline_failed = 擷取比賽時間軸失敗：{ $error }
ui-replay-timeline_retry = 重試
ui-replay-timeline_export_tooltip = 套用於篩選後目前顯示的事件。
ui-replay-timeline_copied = 時間軸已複製
ui-replay-timeline_event_copied = 事件已複製
ui-replay-timeline_saved = 時間軸已儲存至 { $path }
ui-replay-timeline_save_failed = 未能儲存時間軸：{ $error }
ui-replay-timeline_kind_health_lost = 重大傷害
ui-replay-timeline_kind_death = 被摧毀
ui-replay-timeline_kind_cap_contested = 佔領點爭奪中
ui-replay-timeline_kind_cap_flipped = 佔領點已佔領
ui-replay-timeline_kind_cap_being_captured = 佔領點佔領中
ui-replay-timeline_kind_radar = 使用雷達
ui-replay-timeline_kind_advantage = 優勢變化
ui-replay-timeline_kind_disconnected = 斷線

## ui.replay.context
ui-replay-context-open_in_new_tab = 在新分頁中開啟
ui-replay-context-copy_cell = 複製儲存格
ui-replay-context-copy_row = 複製列
ui-replay-context-copy_table = 複製表格
ui-replay-context-pin_column = 固定欄
ui-replay-context-unpin_column = 取消固定欄
ui-replay-context-copied = 已複製
ui-replay-context-copy_path = 複製路徑
ui-replay-context-copy_replay = 複製回放
ui-replay-context-copy_replays = 複製 { $count } 個回放
ui-replay-context-copy_paths = 複製 { $count } 個路徑
ui-replay-context-show_in_explorer = 在檔案總管中顯示
ui-replay-context-show_replay_controls = 顯示回放控制項
ui-replay-context-open_in_game = 在遊戲中開啟
ui-replay-context-render_replay = 渲染回放
ui-replay-context-set_session_stats_one = 設為工作階段統計（1 個回放）
ui-replay-context-set_session_stats_many = 設為工作階段統計（{ $count } 個回放）
ui-replay-context-add_session_stats_one = 加入工作階段統計（1 個回放）
ui-replay-context-add_session_stats_many = 加入工作階段統計（{ $count } 個回放）
ui-replay-context-render_to_video = 匯出為影片
ui-replay-context-render_to_video_many = 匯出{ $count }個回放為影片
ui-replay-context-render_to_clipboard = 匯出到剪貼簿
ui-replay-context-render_to_clipboard_many = 匯出{ $count }個回放到剪貼簿

## ui.replay.column
ui-replay-column-actions = 操作
ui-replay-column-player_name = 玩家名稱
ui-replay-column-base_xp = 基礎 XP
ui-replay-column-raw_xp = 原始 XP
ui-replay-column-ship_name = 艦船名稱
ui-replay-column-hits = 命中
ui-replay-column-heals = 維修
ui-replay-column-heals_tooltip = 觀測到此玩家啟動維修小組的次數。對於未在畫面上渲染的艦船（例如從未被發現的敵艦）可能不準確。
ui-replay-column-heals_no_repair_tooltip = 此艦船沒有維修小組消耗品。
ui-replay-column-kills = 擊沉
ui-replay-column-observed_damage = 觀測傷害
ui-replay-column-actual_damage = 實際傷害
ui-replay-column-spotting_damage = 偵察傷害
ui-replay-column-potential_damage = 潛在傷害
ui-replay-column-time_lived = 存活時間
ui-replay-column-fires = 火災
ui-replay-column-floods = 進水
ui-replay-column-citadels = 核心區命中
ui-replay-column-crits = 關鍵命中
ui-replay-column-received_damage = 受到的傷害
ui-replay-column-distance_traveled = 航行距離
ui-replay-column-skills = 技能
ui-replay-column-personal_rating = PR
ui-replay-column-match_contribution = MC

## ui.rating
ui-rating-personal_rating = 個人評分
ui-rating-match_contribution = 戰鬥貢獻
ui-rating-versus_average = 相較平均
ui-rating-average = 平均
ui-rating-damage = 傷害
ui-rating-kills = 擊沉
ui-rating-wins = 勝利
ui-rating-team_damage_share = 隊伍傷害佔比
ui-rating-tanking = 吸引火力
ui-rating-spotting = 偵察
ui-rating-objectives = 目標
ui-rating-anti_air = 防空
ui-rating-torpedo_warnings = 魚雷預警
ui-rating-critical_bursts = 關鍵爆發
ui-rating-unrated_early_exit = 無 MC：錄影在戰鬥結束前就已結束
ui-rating-mc_unrated_battle_type = 無 MC：戰鬥貢獻僅適用於隨機戰與排位戰

## ui.rating.detail
ui-rating-detail-versus_expected = { $actual }，期望值 { $expected }
ui-rating-detail-wins = 勝利 { $actual }，期望值 { $expected }
ui-rating-detail-weighted_damage = { $actual }，期望值 { $expected }，對驅逐艦與潛艦的傷害 x1.2
ui-rating-detail-team_damage_share = 佔隊伍對敵傷害的 { $share }%，平均分配為 { $even }%
ui-rating-detail-tanking = { $potential } 潛在傷害，為艦種平均的 { $class }x，為隊伍平均分配的 { $team }x
ui-rating-detail-spotting = { $damage } 偵察傷害（艦種平均 { $baseline }），偵察到 { $ships } 艦船 HP（平均 { $ships_baseline }）
ui-rating-detail-objectives = 得分：佔領 { $capture }、阻止佔領 { $block }、重置 { $reset }、結束時持有 { $battle_end }、持有 { $hold }（以四分之一計）
ui-rating-detail-anti_air_planes = { $strike } 架攻擊機，{ $support } 架戰鬥機與偵察機
ui-rating-detail-anti_air_damage = { $damage } 防空傷害
ui-rating-detail-torpedo_warnings = 發現 { $threatening } 枚具威脅的魚雷
ui-rating-detail-torpedo_warnings_one = 發現 1 枚具威脅的魚雷
ui-rating-detail-critical_bursts = { $count } 次關鍵爆發：{ $bursts }。僅能看到錄影玩家附近的艦船
ui-rating-detail-critical_bursts_one = 1 次關鍵爆發：{ $bursts }。僅能看到錄影玩家附近的艦船
ui-rating-detail-critical_burst = { $seconds } 秒內造成 { $ship } 的 { $share }%
ui-rating-detail-critical_burst_at_once = 一次造成 { $ship } 的 { $share }%

## ui.personal_rating
ui-personal_rating-unrated_hover = 個人評分僅適用於隨機戰與排位戰。

## ui.rating.basis
ui-rating-basis-static = 靜態
ui-rating-basis-dynamic = 動態
ui-rating-basis-static_explained = 靜態：與固定常數或艦種基準比較
ui-rating-basis-dynamic_explained = 動態：與 wows-numbers 期望值或本場戰鬥中的其他玩家比較

## ui.replay.filter
ui-replay-filter-raw_xp = 原始 XP
ui-replay-filter-entity_id = 實體 ID
ui-replay-filter-observed_damage = 觀測傷害
ui-replay-filter-heals = 維修
ui-replay-filter-fires = 火災
ui-replay-filter-floods = 進水
ui-replay-filter-citadels = 核心區命中
ui-replay-filter-crits = 模組關鍵命中

## ui.replay.group
ui-replay-group-date = 日期
ui-replay-group-ship = 艦船
ui-replay-group-none = 無
ui-replay-group-prefix = 分組：{ $label }

## ui.replay.player
ui-replay-player-hidden_profile = 該玩家的個人檔案已隱藏
ui-replay-player-never_connected = 該玩家從未連線至比賽

## ui.replay.build
ui-replay-build-open_shiptool = 在 ShipTool 中開啟配裝
ui-replay-build-open_in_ship_builder = 在 WoWs SB 中開啟配裝
ui-replay-build-copy_link = 複製配裝連結
ui-replay-build-copy_short_link = 複製短配裝連結
ui-replay-build-link_copied = 配裝連結已複製
ui-replay-build-open_wows_numbers = 開啟 WoWs Numbers 頁面
ui-replay-build-not_spotted = 此艦船從未被發現。配裝資訊不可用。

## ui.replay.sections
ui-replay-sections-achievements = 成就
ui-replay-sections-ribbons = 勳帶
ui-replay-sections-modules = 模組：
ui-replay-sections-modules_none = 無模組
ui-replay-sections-signals = 信號旗：
ui-replay-sections-loadout = 配置：
ui-replay-sections-loadout_none = 無配置
ui-replay-sections-abilities = 消耗品：
ui-replay-sections-abilities_none = 無消耗品
ui-replay-sections-captain_skills = 艦長技能：
ui-replay-sections-captain_skills_none = 無艦長技能
ui-replay-sections-consumables = 消耗品：
ui-replay-sections-fire_chance = 有效縱火機率
ui-replay-sections-fire_chance_expected = 預期
ui-replay-sections-fire_chance_fires = { $fires } 次縱火
ui-replay-sections-fire_chance_fires_one = 1 次縱火
ui-replay-sections-fire_chance_eligible_hits = { $hits } 次可能引發火災的命中
ui-replay-sections-fire_chance_eligible_hits_one = 1 次可能引發火災的命中
ui-replay-sections-fire_chance_ships = 橫跨 { $ships } 艘目標艦船
ui-replay-sections-fire_chance_ships_one = 橫跨 1 艘目標艦船
ui-replay-sections-fire_chance_expected_fires = 預期 { $fires } 次縱火
ui-replay-sections-fire_chance_per_ship = 各目標艦船
ui-replay-sections-fire_chance_no_eligible_hits = 沒有可能引發火災的命中
ui-replay-sections-fire_chance_formula = 攻擊方縱火機率公式
ui-replay-sections-fire_chance_formula_base = 基礎 burnProb
ui-replay-sections-fire_chance_formula_clamped = 已限制範圍
ui-replay-sections-fire_chance_breakdown = 明細
ui-replay-sections-fire_chance_shells_fired = 已發射的 HE 砲彈
ui-replay-sections-fire_chance_shells_fired_one = 已發射 1 發 HE 砲彈
ui-replay-sections-fire_chance_he_hits = 命中艦船的 HE 砲彈
ui-replay-sections-fire_chance_he_hits_one = 命中艦船的 HE 砲彈 1 發
ui-replay-sections-fire_chance_no_target_ship = 無法歸屬至目標艦船的 HE 命中
ui-replay-sections-fire_chance_no_target_ship_one = 無法歸屬至目標艦船的 HE 命中 1 次
ui-replay-sections-fire_chance_eligible = 可能引發火災
ui-replay-sections-fire_chance_ribbons = 縱火勳帶
ui-replay-sections-fire_chance_ribbons_one = 縱火勳帶 1 個
ui-replay-sections-fire_chance_ribbons_credited = 已歸屬至某發砲彈
ui-replay-sections-fire_chance_ribbons_uncredited = 未歸屬
ui-replay-sections-fire_chance_unattributed_no_burn_state = 沒有可供比對的燃燒狀態變化
ui-replay-sections-fire_chance_unattributed_already_credited = 先前的火災已使用附近的那次命中
ui-replay-sections-fire_chance_unattributed_secondary_contest = 我方副砲同樣有可能造成此次縱火
ui-replay-sections-fire_chance_unattributed_hit_contest = 我方附近的每次命中都與其他命中難以區分
ui-replay-sections-fire_chance_unattributed_all_excluded = 我方附近的每次命中都已被排除
ui-replay-sections-fire_chance_unattributed_none_could_burn = 我方附近沒有任何命中可能引發火災
ui-replay-sections-fire_chance_unattributed_no_hit = 我方沒有命中落在此時間範圍內
ui-replay-sections-fire_chance_not_applicable = 目標已被摧毀，不適用
ui-replay-sections-fire_chance_copied = 縱火機率明細已複製
ui-replay-sections-fire_chance_click_to_copy = 點擊以複製此明細
ui-replay-sections-fire_chance_exclusion_already_burning = 該區段已在燃燒
ui-replay-sections-fire_chance_exclusion_victim_build_unknown = 目標配裝未知，縱火區段可能被合併
ui-replay-sections-fire_chance_exclusion_damage_control_active = 損管小組作用中
ui-replay-sections-fire_chance_exclusion_damage_control_unknown = 損管狀態未知
ui-replay-sections-fire_chance_exclusion_observation_gap = 觀測中斷
ui-replay-sections-fire_chance_exclusion_consumable_unreliable = 消耗品啟動模型不可靠
ui-replay-sections-fire_chance_exclusion_victim_fate_unknown = 目標的結局未知
ui-replay-sections-fire_chance_exclusion_hit_type_does_not_roll = 此命中類型不進行縱火判定
ui-replay-sections-fire_chance_exclusion_no_geometry = 沒有縱火區段的幾何資料
ui-replay-sections-fire_chance_exclusion_impact_unplaceable = 無法將此次命中定位在所比對到的艦船上
ui-replay-sections-fire_chance_exclusion_victim_pose_unknown = 命中當下目標的位置未知
ui-replay-sections-fire_chance_exclusion_ambiguous_hit = 與我方另一次命中難以區分
ui-replay-sections-damage_events = 傷害事件

## ui.replay.debug
ui-replay-debug-view_raw_metadata = 查看原始玩家中繼資料
ui-replay-debug-raw_metadata = 原始中繼資料
ui-replay-debug-raw_results = 原始結果
ui-replay-debug-mapped_results = 對應結果
ui-replay-debug-player_metadata = 玩家中繼資料
ui-replay-debug-no_results_packet = 此回放沒有戰鬥結果資料包
ui-replay-debug-raw_json_tooltip = 由 WG 序列化的原始戰鬥結果。
ui-replay-debug-mapped_json_tooltip = 將位置陣列解析為具名欄位的戰鬥結果。
ui-replay-debug-results_raw_json = 戰鬥結果：原始 JSON
ui-replay-debug-results_mapped_json = 戰鬥結果：已對應 JSON
ui-replay-debug-search = 搜尋
ui-replay-debug-previous_match = 上一個符合項 (Shift+Enter)
ui-replay-debug-next_match = 下一個符合項 (Enter)

## ui.replay.results
ui-replay-results-victory = 勝利
ui-replay-results-defeat = 失敗
ui-replay-results-draw = 平手

## ui.replay.controls
ui-replay-controls-window_title = 回放控制項
ui-replay-controls-unavailable = 控制項不可用（在遊戲檔案中找不到 commands.scheme.xml）。

# -- 協作 / 工作階段 --

## ui.collab
ui-collab-session = 工作階段
ui-collab-starting = 正在啟動工作階段…
ui-collab-session_active = 工作階段進行中
ui-collab-connected_to_session = 已連線至工作階段
ui-collab-session_token = 工作階段權杖：
ui-collab-toggle_visibility = 切換權杖可見性
ui-collab-copy_token = 複製權杖
ui-collab-token_copied = 權杖已複製到剪貼簿
ui-collab-copy_web_link = 複製網頁連結
ui-collab-copy_web_link_tooltip = 複製網頁用戶端的連結
ui-collab-web_link_copied = 網頁連結已複製到剪貼簿
ui-collab-copy_localhost_link = 複製 localhost 連結
ui-collab-copy_localhost_tooltip = 複製 localhost 連結（開發用）
ui-collab-localhost_copied = localhost 連結已複製到剪貼簿
ui-collab-connected_count = { $count } 人已連線
ui-collab-promote_cohost = 升級為共同主持人
ui-collab-promoted_cohost = 已將 { $name } 升級為共同主持人
ui-collab-permissions = 權限
ui-collab-lock_annotations = 鎖定標註
ui-collab-lock_settings = 鎖定設定
ui-collab-reset_overrides = 重置用戶端覆蓋
ui-collab-reset_overrides_tooltip = 重置所有用戶端顯示設定更改
ui-collab-leave = 離開
ui-collab-role_host = 主持人
ui-collab-role_cohost = 共同主持人
ui-collab-you = （你）
ui-collab-display_name = 顯示名稱：
ui-collab-display_name_hint = 您的名稱...
ui-collab-display_name_error = 請輸入顯示名稱
ui-collab-enter_display_name = 請先輸入顯示名稱
ui-collab-host_session = 主持工作階段
ui-collab-start_session = 開始工作階段
ui-collab-join_session = 加入工作階段
ui-collab-paste_and_join = 貼上權杖並加入
ui-collab-clipboard_empty = 剪貼簿是空的
ui-collab-invalid_token = 無效的權杖：{ $error }
ui-collab-no_runtime = 無法啟動工作階段：沒有執行階段
ui-collab-shared_windows = 共享視窗
ui-collab-watching = 觀看中
ui-collab-waiting_for_frames = 正在等待戰鬥資料
ui-collab-open = 開啟
ui-collab-open_for_everyone = 為所有人開啟
ui-collab-tactics_board = 戰術板
ui-collab-max_boards = 最多開啟 { $count } 個戰術板
ui-collab-waiting_for_data = 正在等待遊戲資料載入…

# -- 戰鬥結果文字 --

## battle
battle-victory = 勝利
battle-defeat = 失敗
battle-draw = 平手
battle-battle_starts_in = 戰鬥即將開始

# -- 結束類型描述 --

## finish_type
finish_type-unknown = 戰鬥結束
finish_type-extermination = 所有敵艦已被摧毀
finish_type-base_captured = 基地已被佔領
finish_type-timeout = 時間到
finish_type-failure = 任務失敗
finish_type-technical = 技術性結束
finish_type-score = 達到分數上限
finish_type-score_on_timeout = 時間到時的分數優勢
finish_type-pve_success = 任務成功
finish_type-pve_failed = 任務失敗
finish_type-score_zero = 分數耗盡
finish_type-score_excess = 超過分數上限

# -- 統計分頁 --

## ui.stats
ui-stats-overview = 概覽
ui-stats-sample_notice = 範例資料，並非記錄的戰鬥
ui-stats-limit = 限制
ui-stats-division = 分隊：
ui-stats-div_all = 全部
ui-stats-div_solo = 單人
ui-stats-div_div = 分隊
ui-stats-filters = 篩選
ui-stats-own_filters = 單獨篩選此圖表
ui-stats-own_filters_tooltip = 僅縮小此圖表的範圍，不影響其他面板的顯示內容
ui-stats-mode_label = 模式：
ui-stats-clear = 清除
ui-stats-pr = PR：
ui-stats-mc = MC：
ui-stats-frags_count = { $count } 擊沉
ui-stats-best_frags = 最佳：{ $ship }（{ $count } 擊沉）
ui-stats-max_damage = 最大傷害：{ $ship }（{ $damage }）
ui-stats-copy_markdown = 複製為 Markdown
ui-stats-copy_image = 複製為圖片
ui-stats-copy_image_done = 圖表已複製到剪貼簿
ui-stats-copy_image_failed = 無法複製圖表
ui-stats-copy_csv = 複製為 CSV
ui-stats-remove_games_hint = 移除 { $ship } 的所有場次（Ctrl+Click 跳過確認）
ui-stats-no_stats = 沒有可用的工作階段統計。請先進行一些場次！
ui-stats-settings = 設定
ui-stats-chart_type = 圖表類型
ui-stats-stat_label = 統計：
ui-stats-chart_line = 折線圖
ui-stats-chart_bar = 長條圖
ui-stats-options = 選項
ui-stats-combined = 合計
ui-stats-rolling_avg = 移動平均
ui-stats-labels = 標籤
ui-stats-ships = 艦船
ui-stats-filter_ships = 篩選圖表艦船
ui-stats-all_ships = 全部
ui-stats-no_ships = 無
ui-stats-search_ships = 搜尋艦船...
ui-stats-no_ship_matches = 沒有符合搜尋條件的艦船
ui-stats-search_statistics = 搜尋統計項目
ui-stats-no_statistic_matches = 沒有符合搜尋條件的統計項目
ui-stats-ship_selection_count = 已選擇 { $selected } / { $total }
ui-stats-all_ships_tooltip = 選擇所有匹配的艦船
ui-stats-no_ships_tooltip = 清除匹配艦船的選擇
ui-stats-copy_as_image = 複製為圖片
ui-stats-limit_to_recent = 僅限最近的比賽
ui-stats-limit_to_recent_tooltip = 在「艦船」和「圖表」中按每艘艦船分別限制，在「概覽」中對所有比賽統一限制。
ui-stats-division_label = 分隊：
ui-stats-add_chart = 新增圖表
ui-stats-clear_confirm = 清除所有比賽？
ui-stats-clear_tooltip = 刪除所有已記錄的比賽
ui-stats-clear_failed = 未清除：{ $reason }
ui-stats-clear_ship_confirm = 再次按下以刪除此艦船的比賽
ui-stats-copy_table = 複製此表格
ui-stats-copied = 已複製到剪貼簿
ui-stats-no_games = 當前篩選條件下沒有已記錄的比賽
ui-stats-nothing_to_plot = 當前篩選條件下沒有可繪製的資料
ui-stats-column_ship = 艦船
ui-stats-column_battles = 戰鬥場數
ui-stats-column_win_rate = 勝率
ui-stats-column_avg_damage = 平均傷害
ui-stats-column_avg_frags = 平均擊沉
ui-stats-frags = { $count } 次擊沉
ui-stats-statistic = 統計項
ui-stats-open_directory_stats = 目錄統計
ui-stats-open_directory_stats_tooltip = 開啟此目錄及其子目錄中所有回放的統計
ui-stats-directory_stats_unavailable = 遊戲資料仍在載入
ui-stats-directory_reading_task = 正在讀取回放以生成目錄統計...
ui-stats-directory_walking = 正在尋找回放...
ui-stats-directory_scanning = 正在檢查回放 { $done } / { $total }...
ui-stats-directory_reading = 正在讀取回放 { $done } / { $total }...
ui-stats-directory_reload = 重新載入
ui-stats-directory_reload_tooltip = 重新讀取此目錄中的回放
ui-stats-directory_skipped = 已跳過 { $count } 個回放
ui-stats-directory_skipped_detail =
    無法讀取：{ $unreadable }
    沒有對應組建的遊戲資料：{ $missing_build }
    沒有戰鬥結果：{ $no_results }
ui-stats-directory_no_games = 此目錄中沒有包含戰鬥結果的回放
ui-stats-chart = 圖表
ui-stats-combine_ships = 合併艦船
ui-stats-running_average = 累計平均
ui-stats-value_labels = 數值標籤
ui-stats-reset_view = 重置檢視
ui-stats-keep_current_layout = 保持當前佈局
ui-stats-session_title = 工作階段統計
ui-stats-session_manual = 手動
ui-stats-session_since_opened = 自應用程式啟動以來
ui-stats-session_manual_tooltip = 工作階段包含您進行的戰鬥，以及您從回放清單中設定或加入的回放。它會在多次執行之間保留。
ui-stats-session_since_opened_tooltip = 工作階段包含在 { $time }（應用程式啟動時）或之後開始的所有戰鬥。您的手動工作階段會被保留，切換到手動時即可恢復。
ui-stats-session_battles_one = 1 場戰鬥
ui-stats-session_battles_many = { $count } 場戰鬥
ui-stats-session_chip_manual = 工作階段：{ $battles }
ui-stats-session_chip_since = 自 { $time } 起的工作階段：{ $battles }
ui-stats-session_empty_title = 此工作階段中尚無戰鬥
ui-stats-session_empty_manual = 您進行的戰鬥在結束後加入工作階段。您也可以從回放清單的選單中設定或加入回放。
ui-stats-session_empty_since = 在 { $time } 之後開始的戰鬥會在結束後加入工作階段。
ui-stats-session_set_add_unavailable = 工作階段包含自 { $time } 起的戰鬥。切換到手動以設定或加入回放。
ui-stats-clear_unavailable_since = 自應用程式啟動以來的工作階段不會被儲存。切換到手動以清除已儲存的工作階段。
ui-stats-charts_save_failed = 圖表設定未儲存：{ $reason }
ui-stats-layout_save_failed = 圖表版面配置未儲存：{ $reason }
ui-stats-axis_game = 場次
ui-stats-chart_hover_game = 第{ $number }場，{ $time }
ui-stats-series_average = { $stat }（平均）
ui-stats-series_running = { $stat }（累計）
ui-stats-series_more = 還有 { $count } 項
ui-stats-mode_random = 隨機戰
ui-stats-mode_ranked = 排位戰
ui-stats-mode_other = 其他
ui-stats-win_rate_rated_only = 勝率僅統計隨機戰和排位戰
ui-stats-win_rate_breakdown_line = { $mode }：{ $rate }（{ $battles } 場）
ui-stats-matches = 對局
ui-stats-column_time = 時間
ui-stats-column_mode = 模式
ui-stats-column_result = 結果
ui-stats-keep_game = 將此對局計入統計
ui-stats-leave_out_game = 將此對局排除在統計之外
ui-stats-leave_out_ship = 排除所有 { $ship } 對局
ui-stats-leave_out_ship_hint = 將 { $ship } 排除在統計之外
ui-stats-keep_all_games = 計入所有對局
ui-stats-keep_all_games_tooltip = 重新計入所有手動排除的對局
ui-stats-left_out_only = 僅顯示已排除
ui-stats-match_count = { $total } 場中的 { $shown } 場，已排除 { $left_out } 場
ui-stats-no_matches = 沒有符合目前篩選條件的對局
ui-stats-search_matches = 依艦船搜尋...
ui-stats-ship_filter_tooltip = 選擇計入統計的艦船
ui-stats-keep_ships_tooltip = 計入所有符合的艦船
ui-stats-leave_out_ships_tooltip = 將所有符合的艦船排除在統計之外
ui-stats-dates = 日期
ui-stats-all_dates = 所有日期
ui-stats-clear_dates = 顯示所有日期
ui-stats-games_left_out = 已排除 { $count } 場對局
ui-stats-reset_filters = 重設篩選
ui-stats-reset_filters_tooltip = 顯示所有對局：無數量限制、日期範圍、分隊、模式或已排除的艦船和對局

## ui.stats.table
ui-stats-table-min = 最小
ui-stats-table-max = 最大
ui-stats-table-total = 總計
ui-stats-table-average = 平均

# -- 玩家追蹤 --

## ui.player_tracker
ui-player_tracker-clear_stats = 清除統計
ui-player_tracker-player_filter = 玩家篩選器
ui-player_tracker-populate_from_replays = 從回放填充資料
ui-player_tracker-find_matches = 尋找比賽
ui-player_tracker-open_wows_numbers = 在 wows-numbers 上開啟
ui-player_tracker-open_shipbuilds = 在 shipbuilds 上開啟配裝
ui-player_tracker-subtab_historical = 歷史比賽
ui-player_tracker-subtab_players = 玩家
ui-player_tracker-filter_hint = 按玩家或公會篩選...
ui-player_tracker-time_period = 時間範圍
ui-player_tracker-allies = 友方
ui-player_tracker-enemies = 敵方
ui-player_tracker-live_no_directory = 請設定 World of Warships 目錄以監視戰鬥
ui-player_tracker-live_checking = 正在檢查是否有進行中的戰鬥...
ui-player_tracker-live_none = 沒有進行中的戰鬥
ui-player_tracker-live_loading_build = 正在載入此組建的艦船資料，載入完成後將補全名稱和艦種。
ui-player_tracker-waiting_for_index = 正在等待回放索引
ui-player_tracker-loading_clans = 正在載入公會...
ui-player_tracker-loading_players = 正在載入玩家...
ui-player_tracker-index_failed = 無法讀取索引：{ $reason }
ui-player_tracker-no_players = 此時間範圍內沒有已索引的玩家
ui-player_tracker-notes_title = 備註：{ $name }
ui-player_tracker-note_not_saved = 未儲存：{ $reason }
ui-player_tracker-subtab_current_match = 當前比賽
ui-player_tracker-your_team = 我方隊伍（{ $count }）
ui-player_tracker-enemy_team = 敵方隊伍（{ $count }）
ui-player_tracker-no_live_match = 目前沒有進行中的比賽。進入比賽後即會顯示名單。
ui-player_tracker-encounters_hover = 總共遭遇 { $total } 次，所選時間範圍內 { $range } 次。上次遭遇於 { $last }。
ui-player_tracker-never_encountered = 先前未曾遭遇
ui-player_tracker-copy_wg_id = 點擊以複製 WG ID
ui-player_tracker-copied_wg_id = 已複製 WG ID { $id }
ui-player_tracker-aliases_hover = 又稱：{ $names }
ui-player_tracker-notes_hint = 關於此玩家的備註
ui-player_tracker-arena_count = 已記錄 { $count } 場比賽
ui-player_tracker-last_encountered_exact = 上次遭遇於 { $timestamp }
ui-player_tracker-subtab_clans = 公會
ui-player_tracker-clan_no_data = 尚未記錄任何公會遭遇。
ui-player_tracker-find_clan_matches = 尋找提及此標籤的比賽。索引中沒有僅限公會的欄位，因此包含該標籤的玩家名稱也會被比對到。
ui-player_tracker-clan_sightings_hover = 其中 { $range } 次落在所選時間範圍內。
ui-player_tracker-clan_member_matches = 在 { $count } 場比賽中出現
ui-player_tracker-show_division_mates = 顯示分隊隊友
ui-player_tracker-show_division_mates_hover = 計入您在分隊中進行的比賽。預設為關閉，因為那些玩家是您自己邀請才遇到的。對上同一批玩家的其他比賽一律會計入。
ui-player_tracker-seen_in_twitch_chat = 在 Twitch 聊天室中出現
ui-player_tracker-seen_in_twitch_chat_hover = 僅顯示在與您同場的比賽中，名稱與 Twitch 聊天室登入名稱相符的玩家。
ui-player_tracker-twitch_seen_hover = 在與您同場的 { $count } 場比賽中以 { $logins } 出現在 Twitch 聊天室
ui-player_tracker-win_rate_overall = 總體
ui-player_tracker-win_rate_ship = 艦船
ui-player_tracker-view_compact = 精簡
ui-player_tracker-view_detailed = 詳細
ui-player_tracker-ship_stats = 艦船統計
ui-player_tracker-win_rate_mode_hover = 顯示帳號統計或此艦船的統計。該列的顏色、PR、戰鬥場次與平均傷害都會隨之變更。
ui-player_tracker-stats_resolving = 正在讀取比賽名單...
ui-player_tracker-stats_fetching = 正在取得玩家統計...
ui-player_tracker-stats_rate_limited = 已達速率限制，{ $wait } 後重試
ui-player_tracker-stats_unavailable = 統計不可用：{ $reason }
ui-player_tracker-roster_unavailable = 無法從回放中讀取比賽名單
ui-player_tracker-stats_hidden = 此玩家的個人檔案已隱藏
ui-player_tracker-stats_no_data = 此玩家沒有統計資料
ui-player_tracker-stats_unplayed_ship = 沒有此艦船的戰鬥記錄
ui-player_tracker-pr_overall_hover = 所有艦船的帳號 PR
ui-player_tracker-pr_ship_hover = 此艦船的 PR
ui-player_tracker-avg_damage = 平均傷害
ui-player_tracker-win_rate_hover_line = { $label }：{ $rate }，共 { $battles } 場戰鬥
ui-player_tracker-win_rate_hover_line_damage = { $label }：{ $rate }，共 { $battles } 場戰鬥，平均傷害 { $damage }
ui-player_tracker-team_average_win_rate = 平均勝率：{ $rate }
ui-player_tracker-team_players = { $count } 名玩家
ui-player_tracker-row_win_rate = 勝率 { $rate }
ui-player_tracker-row_personal_rating = PR { $value }
ui-player_tracker-row_battles = { $count } 場戰鬥
ui-player_tracker-row_seen = 遭遇 x{ $total }
ui-player_tracker-debug_replay_picker = 偵錯：將某個回放視為當前比賽
ui-player_tracker-debug_replay_picker_hover = 將已結束回放的名單載入此分頁，以便在戰鬥之外測試統計流程。
ui-player_tracker-debug_replay_none = 選擇回放

## ui.player_tracker.column
ui-player_tracker-column-win_rate = 勝率
ui-player_tracker-column-personal_rating = PR
ui-player_tracker-column-battles = 場次
ui-player_tracker-column-avg_damage = 傷害
ui-player_tracker-column-encounters = 遭遇
ui-player_tracker-column-clan = 公會
ui-player_tracker-column-player_name = 玩家名稱
ui-player_tracker-column-total_encounters = 總遭遇次數
ui-player_tracker-column-encounters_in_range = 時間範圍內遭遇次數
ui-player_tracker-column-last_encountered = 上次遭遇
ui-player_tracker-column-members = 成員
ui-player_tracker-column-matches = 比賽
ui-player_tracker-column-sightings = 目擊次數

## ui.player_tracker.period
ui-player_tracker-period-past_hour = 過去1小時
ui-player_tracker-period-past_six_hours = 過去6小時
ui-player_tracker-period-past_day = 過去24小時
ui-player_tracker-period-past_week = 過去1週
ui-player_tracker-period-past_month = 過去1個月
ui-player_tracker-period-all_time = 所有時間

# -- 模組管理器 --


## ui.twitch
ui-twitch-possible_name = 可能的直播名稱：{ $name }
ui-twitch-seen_minutes = 在比賽開始 { $minutes } 分鐘後看到
ui-twitch-click_to_copy = 點擊以複製
ui-twitch-copied = 已複製 { $name }
## ui.mod_manager
ui-mod_manager-category_filters = 類別篩選器
ui-mod_manager-enabled = 已啟用
ui-mod_manager-mod_home = 模組首頁
ui-mod_manager-discord_thread = Discord 審核討論串
ui-mod_manager-save_config = 儲存模組設定
ui-mod_manager-load_config = 載入模組設定
ui-mod_manager-filter = 篩選

## ui.mod_manager.column
ui-mod_manager-column-enabled = 已啟用
ui-mod_manager-column-name = 名稱
ui-mod_manager-column-author = 作者
ui-mod_manager-column-category = 類別

# -- 回放渲染器 --

## ui.renderer
ui-renderer-loading = 正在載入回放資料...
ui-renderer-preparing_export = 正在準備匯出...
ui-renderer-encoding = 編碼中
ui-renderer-muxing = 混流中

## ui.renderer.advantage
ui-renderer-advantage-breakdown = 優勢分析
ui-renderer-advantage-team_eliminated = 一支隊伍已被全滅
ui-renderer-advantage-score_projection = 分數預測
ui-renderer-advantage-fleet_power = 艦隊戰力
ui-renderer-advantage-strategic_threat = 戰略威脅
ui-renderer-advantage-total = 總計
ui-renderer-advantage-hp_incomplete = HP/艦船資料不完整 — 僅顯示分數因素

## ui.renderer.context
ui-renderer-context-show_trail = 顯示航跡
ui-renderer-context-disable_other_trails = 停用所有其他航跡
ui-renderer-context-ranges = 射程
ui-renderer-context-detection = 偵測距離
ui-renderer-context-main_battery = 主砲
ui-renderer-context-secondary = 副砲
ui-renderer-context-torpedo = 魚雷
ui-renderer-context-radar = 雷達
ui-renderer-context-hydro = 聲納
ui-renderer-context-enable_all = 全部啟用
ui-renderer-context-disable_all = 全部停用
ui-renderer-context-disable_other_ranges = 停用所有其他射程
ui-renderer-context-enable_all_ranges = 啟用所有艦船的射程
ui-renderer-context-show_armor = 顯示即時裝甲
ui-renderer-context-realtime_armor = 顯示即時裝甲

## ui.renderer.controls
ui-renderer-controls-jump_to_start = 跳至開始
ui-renderer-controls-previous_event = 上一個事件 (Shift+Left)
ui-renderer-controls-back_10s = 後退 10 秒 (Left)
ui-renderer-controls-pause = 暫停 (Space)
ui-renderer-controls-play = 播放 (Space)
ui-renderer-controls-forward_10s = 前進 10 秒 (Right)
ui-renderer-controls-next_event = 下一個事件 (Shift+Right)
ui-renderer-controls-jump_to_end = 跳至結束
ui-renderer-controls-save_as_video = 儲存為影片
ui-renderer-controls-copy_to_clipboard = 渲染影片至剪貼簿
ui-renderer-controls-speed = 播放速度

## ui.renderer.settings
ui-renderer-settings-title = 顯示設定
ui-renderer-settings-hp_bars = 血條
ui-renderer-settings-ship_settings = 艦船設定
ui-renderer-settings-armament = 武裝
ui-renderer-settings-dead_ships = 被摧毀的艦船
ui-renderer-settings-dead_ship_names = 被摧毀的艦船名稱
ui-renderer-settings-player_names = 玩家名稱
ui-renderer-settings-ship_ranges = 艦船射程
ui-renderer-settings-self_ship_ranges = 己方艦船射程
ui-renderer-settings-ship_names = 艦船名稱
ui-renderer-settings-camera_direction = 砲塔方向
ui-renderer-settings-trail_settings = 航跡設定
ui-renderer-settings-heat_trail = 熱力航跡
ui-renderer-settings-speed_trails = 速度航跡
ui-renderer-settings-dead_ship_trails = 被摧毀艦船的航跡
ui-renderer-settings-map_settings = 地圖設定
ui-renderer-settings-buildings = 建築物
ui-renderer-settings-capture_points = 佔領點
ui-renderer-settings-consumables = 消耗品
ui-renderer-settings-planes = 飛機
ui-renderer-settings-smoke = 煙幕
ui-renderer-settings-torpedoes = 魚雷
ui-renderer-settings-tracers = 曳光彈
ui-renderer-settings-hud_settings = HUD 設定
ui-renderer-settings-battle_result = 戰鬥結果
ui-renderer-settings-buff_counters = 增益計數器
ui-renderer-settings-chat_label = 聊天
ui-renderer-settings-kill_feed = 擊沉動態
ui-renderer-settings-score_label = 分數
ui-renderer-settings-score_timers = 分數計時器
ui-renderer-settings-team_advantage = 隊伍優勢
ui-renderer-settings-team_rosters = 隊伍名單
ui-renderer-settings-stats_panel = 統計面板
ui-renderer-settings-timer = 計時器
ui-renderer-settings-export_settings = 匯出設定
ui-renderer-settings-prefer_cpu = 優先使用 CPU 編碼器
ui-renderer-settings-prefer_cpu_tooltip = 影片匯出時使用軟體 (CPU) 編碼器而非 GPU
ui-renderer-settings-include_pre_battle = 包含戰鬥前階段
ui-renderer-settings-include_pre_battle_tooltip = 在匯出的影片中包含戰鬥開始前的出生與倒數階段
ui-renderer-settings-export_timing = 影片計時
ui-renderer-settings-export_timing_length = 符合長度
ui-renderer-settings-export_timing_speed = 速度
ui-renderer-settings-export_timing_result_speed = 約以 { $speed }x 播放
ui-renderer-settings-export_timing_result_length = 影片長度 { $length }
ui-renderer-settings-codec = 編解碼器
ui-renderer-settings-codec_auto = 自動
ui-renderer-settings-save_defaults = 儲存預設值
ui-renderer-settings-save_defaults_done = 已儲存為渲染器開啟時的預設設定
ui-renderer-settings-save_defaults_failed = 無法儲存預設設定
ui-renderer-settings-event_timeline = 事件時間軸

# -- 檔案解包器 --

## ui.unpacker
ui-unpacker-assets_bin = Assets.bin
ui-unpacker-loading_assets = 正在載入 Assets.bin...
ui-unpacker-load_failed = 載入失敗：{ $error }
ui-unpacker-no_game_data = 未載入遊戲資料。請在設定中設定您的 World of Warships 目錄。
ui-unpacker-filter_files = 篩選檔案...
ui-unpacker-search_in_files = 在檔案中搜尋
ui-unpacker-search_hint = 在檔案中搜尋...
ui-unpacker-path_filter_hint = 路徑篩選器（例如 *.xml）
ui-unpacker-queue_all = 全部排入佇列
ui-unpacker-queue_all_files = 所有檔案排入佇列
ui-unpacker-empty_directory = 空目錄
ui-unpacker-stop_search_tooltip = 停止搜尋
ui-unpacker-no_matches = 未找到相符結果。
ui-unpacker-view_as_json = 以 JSON 檢視
ui-unpacker-extract_as_json = 以 JSON 匯出
ui-unpacker-view_contents = 檢視內容
ui-unpacker-version = 版本：
ui-unpacker-game_params = GameParams
ui-unpacker-base_as_json = 基礎以 JSON 格式
ui-unpacker-as_json = 以 JSON 格式
ui-unpacker-as_cbor = 以 CBOR 格式
ui-unpacker-as_json_minimal = 以 JSON 格式（精簡 / 轉換）
ui-unpacker-as_cbor_minimal = 以 CBOR 格式（精簡 / 轉換）
ui-unpacker-decode_prototypes = 將原型解碼為 JSON
ui-unpacker-decode_prototypes_tooltip = 在匯出時將支援的 Assets.bin 原型（.visual、.model、.mfm）解碼為 JSON
ui-unpacker-extraction_queue = 匯出佇列
ui-unpacker-collapse_folders = 摺疊資料夾樹
ui-unpacker-expand_folders = 展開資料夾樹
ui-unpacker-clear_all = 全部清除
ui-unpacker-browse = 瀏覽
ui-unpacker-output_dir_hint = 輸出目錄...
ui-unpacker-go_to_directory = 前往目錄
ui-unpacker-remove_from_queue = 從佇列中移除
ui-unpacker-queue_folder = 將整個資料夾排入佇列
ui-unpacker-queue_file = 將此檔案加入佇列
ui-unpacker-extraction_output = 提取到
ui-unpacker-queue_empty = 在列表中勾選檔案或資料夾以加入佇列
ui-unpacker-show_queue = 提取佇列
ui-unpacker-double_click_open = 雙擊開啟
ui-unpacker-packages = 資源包
ui-unpacker-item_count = { $count } 項
ui-unpacker-search_title = 搜尋：{ $query }
ui-unpacker-search_in = 位於 { $source }
ui-unpacker-image_decode_failed = 無法解碼此影像：{ $reason }
ui-unpacker-find = 尋找
ui-unpacker-find_previous = 上一個符合項
ui-unpacker-find_next = 下一個符合項
ui-unpacker-find_close = 關閉尋找
ui-unpacker-find_count = { $current } / { $total }
ui-unpacker-find_none = 沒有符合項
ui-unpacker-find_match_case = 區分大小寫
ui-unpacker-queue = 加入佇列
ui-unpacker-reveal_in_tree = 在資源包樹中顯示
ui-unpacker-hit_offset = 位於 { $offset }
ui-unpacker-queued_count = 已加入佇列 { $count } 項
ui-unpacker-counting_files = 正在統計檔案...
ui-unpacker-extracting = 正在提取 { $done } / { $total }
ui-unpacker-extract_failed = 提取失敗：{ $reason }
ui-unpacker-extracted = 已提取 { $count } 個檔案
ui-unpacker-cancelled = 已在提取 { $count } 個檔案後取消
ui-unpacker-extract = 提取
ui-unpacker-extract_one = 提取 1 項
ui-unpacker-extract_many = 提取 { $count } 項
ui-unpacker-choose_directory = 請先選擇提取目標目錄
ui-unpacker-dump_parameters = 匯出 GameParams
ui-unpacker-dump_parameters_tooltip = 匯出此組建的 GameParams.data
ui-unpacker-load_a_build = 請先載入一個組建
ui-unpacker-parameters_written = 參數已寫入
ui-unpacker-save_parameters_title = 儲存遊戲參數
ui-unpacker-writing = 正在寫入...
ui-unpacker-prototype_written = 原型已寫入
ui-unpacker-whole_tree = 完整樹
ui-unpacker-base_only = 僅基礎參數

## ui.unpacker.column
ui-unpacker-column-name = 名稱
ui-unpacker-column-size = 大小
ui-unpacker-column-type_col = 類型
ui-unpacker-column-path = 路徑
ui-unpacker-column-file = 檔案
ui-unpacker-column-offset = 偏移
ui-unpacker-column-context = 上下文

# -- 裝甲檢視器 --

## ui.armor
ui-armor-load_settings = 請在設定中載入遊戲資料以使用裝甲檢視器。
ui-armor-gpu_unavailable = GPU 渲染狀態不可用。
ui-armor-loading_ship = 正在載入艦船資料...
ui-armor-load_failed = 載入艦船資料失敗：{ $error }
ui-armor-ship_load_failed = 載入 { $ship } 失敗：{ $reason }
ui-armor-empty = 空
ui-armor-mirror_cameras = 鏡像攝影機
ui-armor-sync_options = 同步選項
ui-armor-show_armor_by_default = 預設顯示裝甲
ui-armor-sync_tooltip = 在所有面板之間同步裝甲/船體可見性與顯示覆蓋層（船舶中心、水線、攝影機環）
ui-armor-common_settings = 通用設定
ui-armor-tier_filter = { $lowest } 至 { $highest } 階
ui-armor-fortifications = 要塞
ui-armor-show_hidden_ships = 顯示隱藏的艦船
ui-armor-show_hidden_ships_tooltip = 同時列出清單略去的艦船，例如公會租借艦船
ui-armor-compare_split = 在新分割中比較
ui-armor-open_in_new_tab = 在新分頁中開啟
ui-armor-export_model = 匯出艦船模型
ui-armor-armor_thickness = 裝甲厚度
ui-armor-export_disclaimer = 這些 3D 模型和貼圖是 Wargaming 的智慧財產，任何使用這些模型的行為都應遵守您當地的法律。
ui-armor-export_button = 匯出
ui-armor-armor_toggle = 裝甲
ui-armor-armor_tooltip = 切換裝甲區域可見性
ui-armor-hull_toggle = 船體
ui-armor-hull_tooltip = 切換船體模型部件可見性
ui-armor-splash_toggle = 爆風
ui-armor-splash_tooltip = 切換爆風區域可見性
ui-armor-show_hidden = 顯示隱藏
ui-armor-show_hidden_tooltip = 切換（可能）隱藏的面板可見性
ui-armor-gaps = 間隙
ui-armor-gaps_tooltip = 醒目標示裝甲板未連接的邊界邊緣（可能的間隙）
ui-armor-display = 顯示
ui-armor-display_tooltip = 顯示設定 (Ctrl+S)
ui-armor-marker_opacity = 標記不透明度
ui-armor-save_defaults = 儲存為預設值
ui-armor-export_tab = 匯出
ui-armor-export_tooltip = 將艦船模型匯出為 OBJ 檔案
ui-armor-pen_check = 貫穿檢查
ui-armor-pen_check_tooltip = 設定用於貫穿比較的砲彈
ui-armor-trajectory = 彈道
ui-armor-trajectory_on = 彈道 [ON]
ui-armor-trajectory_range = 距離
ui-armor-trajectory_clear = 全部清除
ui-armor-arc_heading = 彈道 { $number }
ui-armor-arc_summary = { $hits } 次命中，{ $range } km 處 { $armor }mm
ui-armor-arc_delete = 刪除此彈道
ui-armor-arc_isolate_plates = 單獨顯示裝甲板
ui-armor-arc_isolate_plates_tooltip = 僅顯示此彈道穿過的裝甲板
ui-armor-arc_isolate_zones = 單獨顯示裝甲區域
ui-armor-arc_isolate_zones_tooltip = 顯示此彈道進入的裝甲區域的所有部分
ui-armor-arc_simulation_caveat = 此模擬基於逆向工程得到的資料，可能與遊戲計算彈道的方式不一致。
ui-armor-arc_no_shell = 未投射砲彈，僅顯示穿過的裝甲板
ui-armor-arc_detonated_in = 在 { $zone } 內爆炸
ui-armor-arc_detonated_past_armor = 穿過全部裝甲後爆炸
ui-armor-arc_ricochet = 在 { $plate } 處跳彈
ui-armor-arc_shatter = 在 { $plate } 處未擊穿
ui-armor-arc_stopped = 在 { $plate } 處停止
ui-armor-arc_overpen = 過度擊穿
ui-armor-arc_overpen_unarmed = 過度擊穿（引信未觸發）
ui-armor-arc_plate_unknown = 未記錄的裝甲板
ui-armor-angle_good = 小於 30 度
ui-armor-angle_mid = 30 至 45 度
ui-armor-angle_bad = 大於 45 度
ui-armor-trajectory_tooltip = 點擊裝甲以模擬砲彈彈道 (Ctrl+T)
ui-armor-trajectory_ship = 艦船：
ui-armor-trajectory_select_ship = 選擇艦船
ui-armor-trajectory_ship_tooltip = 選擇提供彈道砲彈的艦船
ui-armor-trajectory_add_ship = 新增射擊艦船
ui-armor-trajectory_add_ship_tooltip = 開啟貫穿檢查以搜尋並新增艦船
ui-armor-trajectory_shell = 砲彈：
ui-armor-trajectory_select_shell = 選擇砲彈
ui-armor-trajectory_shell_tooltip = 選擇用於彈道投射的砲彈
ui-armor-continue_ricochet = 跳彈後繼續
ui-armor-splash_mode = 爆風
ui-armor-splash_on = 爆風 [ON]
ui-armor-splash_no_data = 此艦船沒有爆風資料
ui-armor-splash_no_shells = 請將擁有 HE/SAP 砲彈的艦船加入比較列表
ui-armor-splash_tooltip_mode = 點擊裝甲以視覺化 HE 爆風傷害範圍
ui-armor-loading_armor = 正在載入艦船...
ui-armor-select_ship = 從列表中選擇艦船
ui-armor-pen_check_label = 貫穿檢查
ui-armor-all_btn = 全部
ui-armor-none_btn = 無
ui-armor-opaque = 不透明
ui-armor-upgrade = 升級：
ui-armor-camo = 迷彩：
ui-armor-camo_none = 原始（無）
ui-armor-camo_group_universal = 通用迷彩
ui-armor-camo_group_expendable = 消耗型迷彩
ui-armor-camo_group_other = 其他迷彩
ui-armor-show_armor_thickness = 顯示裝甲厚度圖例
ui-armor-plate_edges = 裝甲板邊緣
ui-armor-waterline = 水線
ui-armor-camera_rings = 攝影機環
ui-armor-realtime_readout = { $health } HP  -  { $hits } 次命中
ui-armor-show_camera_rings = 顯示攝影機環
ui-armor-camera_fov = 攝影機視野
ui-armor-camera_height = 攝影機高度
ui-armor-show_camera_zoom_path = 顯示縮放路徑
ui-armor-zoom_path_regular_fov = 一般視野
ui-armor-zoom_path_max_fov = 最大視野
ui-armor-camera_perspective = 攝影機視角
ui-armor-perspective_fov = 檢視視野
ui-armor-perspective_projection = 投影
ui-armor-perspective_proj_game = 遊戲
ui-armor-perspective_proj_center = 穿過中心
ui-armor-ship_center = 船舶中心
ui-armor-grid = 網格
ui-armor-length_marks = 長度刻度
ui-armor-length_readout = 長 { $length }，寬 { $beam }
ui-armor-length_metres = { $value } m
ui-armor-length_centimetres = { $value } cm
ui-armor-filter_options = 篩選選項...
ui-armor-filter_zones = 篩選區域...
ui-armor-filter_parts = 篩選部件...
ui-armor-tree_matches = 符合：{ $count }
ui-armor-tree_no_matches = 無符合項目
ui-armor-no_matching_options = 沒有符合篩選的選項
ui-armor-opacity = 不透明度
ui-armor-ship_hull = 艦船船體
ui-armor-opaque_hull = 不透明船體
ui-armor-lighting = 船體光照
ui-armor-lighting_enabled = 啟用光照
ui-armor-lighting_preset_ingame = 遊戲內
ui-armor-lighting_preset_flat = 平坦
ui-armor-lighting_preset_studio = 工作室
ui-armor-lighting_flat = 平坦光（環境光）
ui-armor-lighting_intensity = 光照強度
ui-armor-lighting_azimuth = 光源方位角
ui-armor-lighting_elevation = 光源仰角
ui-armor-lighting_rim = 補光
ui-armor-lighting_specular = 鏡面反射
ui-armor-lighting_shininess = 光澤度
ui-armor-lighting_key_color = 主光顏色
ui-armor-lighting_ambient_color = 環境光顏色
ui-armor-pop_out = 彈出視窗
ui-armor-zero_mm_plates = 0 mm 裝甲板
ui-armor-armor_opacity = 裝甲不透明度
ui-armor-ballistic_disclaimer = 彈道結果基於對模擬運作方式的逆向工程/估算
ui-armor-roll = 橫搖
ui-armor-ctrl_click_solo = Ctrl+點擊以單獨顯示
ui-armor-reset_plates = 重置裝甲板
ui-armor-visibility = 可見性
ui-armor-armor_options = 裝甲
ui-armor-no_ship_loaded = 未載入艦船
ui-armor-lod = LOD：
ui-armor-lod_highest = 0（最高）
ui-armor-camouflage = 迷彩
ui-armor-search_ships = 搜尋艦船...
ui-armor-ships = 艦船
ui-armor-compare = 比較
ui-armor-compare_tooltip = 開啟新的比較面板，或右鍵點擊艦船以比較該艦船
ui-armor-mirror_cameras_tooltip = 同步移動所有面板的攝影機
ui-armor-show_armor_by_default_tooltip = 開啟模型時顯示裝甲而非船體
ui-armor-xray_highlight = 在檢視中懸停時的透視醒目提示
ui-armor-xray_always = 一律
ui-armor-xray_while_held = 按住 { $key } 時
ui-armor-xray_off = 關閉
ui-armor-stack_panes = 堆疊面板
ui-armor-stack_panes_tooltip = 將比較面板上下排列，而不是左右並排
ui-armor-viewports = 檢視區
ui-armor-options_panel = 選項面板
ui-armor-options_pane = 面板 { $number } / { $count }
ui-armor-splash_boxes = 爆風框
ui-armor-cast_here_hint = 按住 { $key } 並點擊以在此處投射砲彈
ui-armor-trajectory_next_cast = 下一次投射
ui-armor-arc_pick_hint = 點擊標記以編輯其彈道。
ui-armor-arc_marker_tooltip = 彈道 { $number }。點擊以編輯，右鍵點擊以移除。
ui-armor-zone_toggle_tooltip = 顯示或隱藏此區域的所有裝甲板。按住 { $key } 可僅顯示此區域。
ui-armor-mode_solo_zone = 僅此區域
ui-armor-mode_cast_trajectory = 投射彈道
ui-armor-loading_catalog = 正在載入艦船目錄...
ui-armor-catalog_failed = 載入艦船目錄失敗：{ $reason }
ui-armor-legend_collapse = 收合
ui-armor-legend_hide = 隱藏圖例
ui-armor-armor_thickness_title = 裝甲厚度
ui-armor-initializing = 正在初始化 3D 檢視...
ui-armor-init_failed = 裝甲檢視初始化失敗：{ $reason }
ui-armor-loading_named_ship = 正在載入 { $ship }...
ui-armor-rendering = 渲染中...
ui-armor-options = 選項

## ui.armor.compass
ui-armor-compass-north = 北
ui-armor-compass-east = 東
ui-armor-compass-south = 南
ui-armor-compass-west = 西

## ui.armor.realtime
ui-armor-realtime-loading = 正在載入艦船裝甲...
ui-armor-realtime-waiting = 正在等待艦船資料...
ui-armor-realtime-incoming_fire = 受到攻擊
ui-armor-realtime-attacker_filter = 攻擊者篩選器
ui-armor-realtime-all_enemies = 所有敵人
ui-armor-realtime-show_secondaries = 顯示副砲武裝
ui-armor-realtime-auto_scroll = 自動捲動
ui-armor-realtime-seek_to_salvo = 跳至齊射
ui-armor-realtime-deselect = 取消選擇
ui-armor-realtime-select_all = 全部選擇
ui-armor-realtime-no_data = 砲彈資料已不可用
ui-armor-realtime-sim_agrees = 模擬結果一致
ui-armor-realtime-sim_unchecked = { $count } 發砲彈無法模擬
ui-armor-realtime-sim_rng_zone = 隨機區間（{ $angle } 度，位於 { $from }-{ $to }）
ui-armor-realtime-sim_disagrees = 模擬：{ $sim } / 伺服器：{ $server }
ui-armor-realtime-no_armor_hit = 未命中裝甲板
ui-armor-realtime-tracked = 已追蹤 { $salvos } 次齊射、{ $shells } 發砲彈
ui-armor-realtime-nothing_landed = 此艦船尚未被命中
ui-armor-realtime-shells = { $count }x
ui-armor-realtime-salvo_shell = { $caliber } mm { $shell }
ui-armor-realtime-salvo_range = { $km } km
ui-armor-realtime-unknown_attacker = 艦船 { $id }
ui-armor-realtime-server_outcome = 伺服器: { $outcome }
ui-armor-realtime-not_cast = 無法在船體內追蹤此砲彈: 未記錄其飛行或艦船位置
ui-armor-realtime-pick_hint = 點擊齊射或砲彈以在船體內追蹤其彈道

## ui.armor.plates
ui-armor-plates-heading = 命中的裝甲板 ({ $count })
ui-armor-plates-arrival = 著彈 { $velocity } m/s，飛行 { $time } 秒，落角 { $fall } 度
ui-armor-plates-overmatch = 過穿: 穿深 { $penetration } mm，{ $velocity } m/s
ui-armor-plates-penetrated = 擊穿: { $penetration } mm 對 等效 { $effective } mm，{ $velocity } m/s
ui-armor-plates-ricochet = { $angle } 度跳彈
ui-armor-plates-shatter = 碎裂: { $penetration } mm 對 等效 { $effective } mm
ui-armor-plates-fuse_armed = 引信已觸發
ui-armor-plates-detonation = 在 { $zone } 內爆炸，裝甲板 #{ $number } 之後 { $distance } m
ui-armor-plates-detonation_past_armor = 在所有裝甲之外爆炸，裝甲板 #{ $number } 之後 { $distance } m
ui-armor-plates-flat_penetrates = { $ammo } 擊穿: { $penetration } mm 對 { $thickness } mm
ui-armor-plates-flat_shatters = { $ammo } 碎裂: { $penetration } mm 對 { $thickness } mm
ui-armor-plates-server_angle = 伺服器 { $angle } 度

# -- 優勢等級標籤 --

## advantage
advantage-absolute = 絕對
advantage-strong = 強大
advantage-moderate = 中等
advantage-weak = 微弱

## ui.search
ui-search-undo = 復原對查詢的上一次更改
ui-search-edit_value = 編輯值
ui-search-add_filter = 新增篩選
ui-search-redo = 重做已復原的更改
ui-search-pill_select = 選擇
ui-search-pill_deselect = 取消選擇
ui-search-pill_group_all = 以「且」分組
ui-search-pill_group_any = 以「或」分組
ui-search-pill_ungroup = 解散分組
ui-search-pill_negate = 取反
ui-search-pill_flip_connector = 切換且/或
ui-search-pill_delete = 刪除
ui-search-match_count_truncated = { $count }+ 場比賽
ui-search-outcome_win = 勝利
ui-search-outcome_loss = 失敗
ui-search-outcome_draw = 平手
ui-search-outcome_unknown = 未知
ui-search-bool_true = 真
ui-search-bool_false = 假
ui-search-match_count = { $count } 場比賽
ui-search-row_actions_hint = 回放操作（或右鍵點擊該行）
ui-search-looking_up_values = 正在查詢取值...
ui-search-type_a_query = 輸入查詢後按 Enter
ui-search-parse_failed = 無法解析該查詢：{ $reason }
ui-search-searching = 正在搜尋...
ui-search-failed = 搜尋失敗：{ $reason }
ui-search-no_matches = 沒有匹配結果
ui-search-game_mode_gap_hint_one = 有 1 場已建立索引的比賽沒有記錄遊戲模式。請在設定中重新為所有回放建立索引以補上。
ui-search-game_mode_gap_hint = 有 { $count } 場已建立索引的比賽沒有記錄遊戲模式。請在設定中重新為所有回放建立索引以補上。
ui-search-open = 開啟
ui-search-open_missing = 回放檔案已不存在
ui-search-opened_directory = 已開啟 { $dir } 以顯示此回放
ui-search-open_no_directory = 無法判斷此回放屬於哪個目錄
ui-search-path_copied = 回放路徑已複製到剪貼簿
ui-search-preview_map = 預覽地圖
ui-search-preview_and_render = 停留以預覽，點擊以渲染
ui-search-free_text = 包含「{ $text }」
ui-search-quant_any = 任一
ui-search-quant_none = 沒有
ui-search-quant_ge = { $n } 個以上
ui-search-quant_eq = 剛好 { $n } 個
ui-search-quant_le = 最多 { $n } 個
ui-search-quant_gt = 超過 { $n } 個
ui-search-quant_lt = 少於 { $n } 個
ui-search-quant_ne = 不是 { $n } 個
ui-search-scope_self = 我方
ui-search-scope_ally = 友方
ui-search-scope_enemy = 敵方
ui-search-scope_div = 分隊

## ui.search.suggest
ui-search-suggest-preset_divmate_test_ship = 我的分隊中有人使用測試艦
ui-search-suggest-preset_no_enemy_cv = 沒有敵方航空母艦
ui-search-suggest-preset_all_enemies_died = 所有敵人都被擊沉
ui-search-suggest-preset_high_damage_enemies = 3 個以上敵人傷害超過 100k
ui-search-suggest-preset_stream_sniper = 包含直播狙擊者
ui-search-suggest-preset_i_survived = 我存活下來
ui-search-suggest-preset_i_disconnected = 我斷線了
ui-search-suggest-my_damage = 我的傷害
ui-search-suggest-enemy_ship = 敵方艦船
ui-search-suggest-allied_ship = 友方艦船
ui-search-suggest-player_in_match = 該玩家在比賽中
ui-search-suggest-someone_in_division = 我的分隊中有人
ui-search-suggest-scope_self = 我的
ui-search-suggest-scope_ally = 友方
ui-search-suggest-scope_enemy = 敵方
ui-search-suggest-scope_division = 分隊
ui-search-suggest-scope_anyone = 任何玩家

## ui.search.bar
ui-search-bar-hint = 篩選回放
ui-search-bar-and_word = 且
ui-search-bar-or_word = 或
ui-search-bar-not_word = 非
ui-search-bar-group_and = 分組為 AND
ui-search-bar-group_or = 分組為 OR
ui-search-bar-negate = 否定
ui-search-bar-delete = 刪除
ui-search-bar-clear_selection = 清除選取
ui-search-bar-ungroup = 點擊以解散此群組
ui-search-bar-flip_connector = 點擊以切換 AND 與 OR
ui-search-bar-context_preset = 預設
ui-search-bar-context_match = 比賽
ui-search-bar-context_roster = 名單
ui-search-bar-no_suggestions = 沒有相符項目

## ui.search.field
ui-search-field-outcome = 結果
ui-search-field-map = 地圖
ui-search-field-class = 艦種
ui-search-field-tier = 階級
ui-search-field-date = 日期
ui-search-field-player_present = 玩家
ui-search-field-group = 回放分組
ui-search-field-stat_damage = 傷害
ui-search-field-stat_kills = 擊沉
ui-search-field-stat_spotting = 偵察傷害
ui-search-field-stat_potential = 潛在傷害
ui-search-field-stat_received = 受到的傷害
ui-search-field-stat_pr = PR
ui-search-field-stat_mc = MC
ui-search-field-rating = 評分
ui-search-field-stat_survived = 存活
ui-search-field-stat_disconnected = 斷線
ui-search-field-game_type = 遊戲類型
ui-search-field-game_mode = 遊戲模式
ui-search-field-match_group = 比賽分組
ui-search-field-build = 組建
ui-search-field-results_available = 結果可用
ui-search-field-relation = 關係
ui-search-field-division = 分隊
ui-search-field-name = 名稱
ui-search-field-clan = 公會
ui-search-field-realm = 伺服器
ui-search-field-ship = 艦船
ui-search-field-ship_index = 艦船索引
ui-search-field-nation = 國家
ui-search-field-test_ship = 測試艦
ui-search-field-stream_sniper = 直播狙擊者
ui-search-field-sniper_login = 狙擊者 Twitch 帳號

## ui.search.op
ui-search-op-contains = 包含
ui-search-op-equals = 等於
ui-search-op-not_equals = 不等於
ui-search-op-eq = =
ui-search-op-ne = !=
ui-search-op-gt = >
ui-search-op-ge = >=
ui-search-op-lt = <
ui-search-op-le = <=
ui-search-op-is = 是
ui-search-op-is_not = 不是
ui-search-op-is_set = 已設定
ui-search-op-is_not_set = 未設定

## ui.search.value
ui-search-value-relation_self = 自己
ui-search-value-relation_ally = 友方
ui-search-value-relation_enemy = 敵方
ui-search-value-division_mine = 我的
ui-search-value-division_any = 任一
ui-search-value-division_none = 無
ui-search-value-class_air_carrier = 航空母艦
ui-search-value-class_battleship = 戰艦
ui-search-value-class_cruiser = 巡洋艦
ui-search-value-class_destroyer = 驅逐艦
ui-search-value-class_submarine = 潛艦
ui-search-value-class_auxiliary = 輔助艦
ui-search-value-rating_bad = 糟糕
ui-search-value-rating_below_average = 低於平均
ui-search-value-rating_average = 平均
ui-search-value-rating_good = 良好
ui-search-value-rating_very_good = 非常好
ui-search-value-rating_great = 優秀
ui-search-value-rating_unicum = 卓越
ui-search-value-rating_super_unicum = 超凡
ui-search-value-game_mode_test = 測試
ui-search-value-game_mode_standard = 標準
ui-search-value-game_mode_singlebase = 單一基地
ui-search-value-game_mode_domination = 區域佔領
ui-search-value-game_mode_tutorial = 教學
ui-search-value-game_mode_megabase = 大型基地
ui-search-value-game_mode_forts = 堡壘
ui-search-value-game_mode_standard_domination = 標準區域佔領
ui-search-value-game_mode_epicenter = 震央
ui-search-value-game_mode_assault_defense = 突擊防禦
ui-search-value-game_mode_pve = PVE
ui-search-value-game_mode_arms_race = 軍備競賽
ui-search-value-game_mode_epicenter_ring = 震央環
ui-search-value-game_mode_anti_standard = 反向標準
ui-search-value-game_mode_attack_defense = 攻防戰
ui-search-value-game_mode_torpedo_beat = 魚雷節奏
ui-search-value-game_mode_team_battle_royale = 團隊大逃殺
ui-search-value-game_mode_escape_to_portal = 逃往傳送門
ui-search-value-game_mode_domination_asymm = 非對稱區域佔領
ui-search-value-game_mode_key_battle = 關鍵戰鬥
ui-search-value-game_mode_portal_2021 = 傳送門 2021
ui-search-value-game_mode_team_battle_royale_2021 = 團隊大逃殺 2021
ui-search-value-game_mode_convoy_event = 護航活動
ui-search-value-game_mode_convoy_airship = 飛船護航
ui-search-value-game_mode_two_teams_battle_royale = 雙隊大逃殺
ui-search-value-game_mode_pinata_event = 皮納塔活動
ui-search-value-game_mode_respawns = 重生
ui-search-value-game_mode_respawns_sectors = 重生區域


## ui.search.column
ui-search-column-date = 日期
ui-search-column-map = 地圖
ui-search-column-mode = 模式
ui-search-column-ship = 你的艦船
ui-search-column-result = 結果
ui-search-column-damage = 傷害
ui-search-column-kills = 擊沉
ui-search-column-pr = PR
ui-search-column-mc = MC

## ui.settings.index
ui-settings-index-heading = 回放索引
ui-settings-index-description = 目錄中每個回放的可搜尋記錄
ui-settings-index-build = 建立索引
ui-settings-index-rebuild = 全部重新索引
ui-settings-index-rebuild_hint = 重新讀取每個回放，用於修正舊版解析出錯的條目。「建立索引」只讀取缺失的部分。
ui-settings-index-progress = 已讀取 { $done } / { $total }，{ $failed } 個無法讀取
ui-settings-index-progress_skipped = 已讀取 { $done } / { $total }，{ $skipped } 個已在索引中，{ $failed } 個無法讀取
ui-settings-index-built = 已為 { $indexed } 個回放建立索引，{ $failed } 個無法讀取
ui-settings-index-built_with_skipped = 已為 { $indexed } 個回放建立索引，{ $skipped } 個已在索引中，{ $failed } 個無法讀取

## ui.settings
ui-settings-loading = 正在載入設定...
ui-settings-load_failed = 載入設定失敗
ui-settings-not_set = （未設定）

## ui.settings.panel
ui-settings-panel-mcp = MCP 伺服器
ui-settings-panel-open = 開啟設定
ui-settings-panel-search_placeholder = 搜尋設定
ui-settings-panel-count_one = 1 項設定
ui-settings-panel-count = { $count } 項設定
ui-settings-panel-no_matches = 沒有符合搜尋的設定
ui-settings-panel-close = 關閉設定
ui-settings-panel-in_error = { $name }：有設定需要處理
ui-settings-panel-general = 一般
ui-settings-panel-appearance = 外觀
ui-settings-panel-game = World of Warships
ui-settings-panel-replays = 重播
ui-settings-panel-collaboration = 協作
ui-settings-panel-twitch = Twitch
ui-settings-panel-language_description = 介面語言
ui-settings-panel-data_sharing_description = 排位戰和隨機戰結束後傳送至 ShipBuilds.com 的內容
ui-settings-panel-code_integrity_description = 是否允許未經 Microsoft 簽署的程式碼載入此應用程式
ui-settings-panel-check_for_updates_description = 每次啟動應用程式時檢查是否有新版本
ui-settings-panel-data_folder = 應用程式資料資料夾
ui-settings-panel-theme_description = 介面色彩。將指標指向選項即可預覽。
ui-settings-panel-zoom_description = 所有文字與控制項的大小
ui-settings-panel-zoom = 縮放
ui-settings-panel-zoom_shortcuts = { $zoom_in } 放大，{ $zoom_out } 縮小，{ $reset } 恢復為 1x
ui-settings-panel-zoom_changed = 縮放：{ $factor }x
ui-settings-panel-cache_directory = 快取目錄
ui-settings-panel-cache_directory_description = 快取配置的儲存位置
ui-settings-panel-cached_builds = 已快取的配置
ui-settings-panel-current_path_description = 最近開啟的重播檔案
ui-settings-panel-show_raw_xp_description = 在結果表中新增原始 XP 欄
ui-settings-panel-show_observed_damage_description = 在結果表中新增觀測傷害欄
ui-settings-panel-show_entity_id_description = 在結果表中新增實體 ID 欄
ui-settings-panel-show_heals_description = 在結果表中新增治療欄
ui-settings-panel-auto_export_description = 將每場戰鬥的資料以所選格式寫入所選目錄中的檔案
ui-settings-panel-display_name_description = 工作階段中其他人看到的名稱
ui-settings-panel-get_token_description = 開啟可核發聊天權杖的登入頁面
ui-settings-panel-token = 登入權杖
ui-settings-panel-monitored_channel_description = 要監看的頻道。留空則監看您自己的頻道。
ui-settings-panel-armor_defaults = 已儲存的預設設定
ui-settings-panel-armor_saved_description = 從模型檢視器選項儲存

## ui.settings.licenses
ui-settings-licenses-title = 關於與授權條款
ui-settings-licenses-back = 返回設定
ui-settings-licenses-third_party_intro = 本應用程式包含以下列出的第三方元件。選擇一個項目可顯示使用它的套件及其完整授權條款文字。

## ui.settings.armor
ui-settings-armor-no_defaults = （沒有已儲存的預設設定）

## ui.app
ui-app-status_cache_checking = 正在檢查遊戲資料快取
ui-app-status_cache_validating = 正在校驗遊戲資料快取
ui-app-status_cache_downloading = 正在下載遊戲資料
ui-app-status_cache_planning = 正在查詢已釋出的遊戲資料
ui-app-drop_to_load = 鬆開以載入 { $file }
ui-app-drop_to_list = 放開以列出 { $folder } 中的重播
ui-app-drop_to_list_replays = 放開以列出 { $count } 個回放
ui-app-drop_to_list_folders = 放開以列出 { $count } 個資料夾中的回放
ui-app-drop_to_open_and_list = 放開以開啟 { $file } 並列出 { $folders } 中的回放
ui-app-drop_to_list_replays_and_folders = 放開以列出 { $count } 個回放及 { $folders } 中的回放
ui-app-drop_folder_count = { $count } 個資料夾
ui-app-reading_replay = 正在讀取 { $replay }
ui-app-reading_replays = 正在讀取 { $count } 個回放
ui-app-status_constants_generating = 正在更新 { $version } 的遊戲常數
ui-app-worker_constants = 遊戲常數背景工作
ui-app-worker_live_match = 戰鬥監視
ui-app-worker_replay_loader = 回放載入器
ui-app-worker_restarting = { $worker }已停止，正在重新啟動
ui-app-worker_stalled = { $worker }沒有回應
ui-app-worker_stopped = { $worker }已停止
ui-app-status_indexing = 正在為回放建立索引
ui-app-status_of = { $done } / { $total }
ui-app-status_tasks = 執行中的工作：{ $count }
ui-app-status_show_panel = 顯示面板：{ $panel }
ui-app-status_hide_panel = 隱藏面板：{ $panel }
ui-app-panel_left = 左側
ui-app-panel_right = 右側
ui-app-panel_bottom = 底部
ui-app-debug_build = 除錯版本

## ui.palette
ui-palette-title = 命令
ui-palette-placeholder = 輸入命令或搜尋...
ui-palette-go_to = 前往：{ $tab }
ui-palette-advanced_search = 進階搜尋...
ui-palette-set_theme = 主題：{ $theme }
ui-palette-games_i_died_in = 我被擊沉的比賽
ui-palette-games_i_won = 我獲勝的比賽
ui-palette-send_all_replays = 將所有回放傳送到 ShipBuilds
ui-palette-send_all_replays_again = 將所有回放傳送到 ShipBuilds（忽略傳送記錄）
ui-palette-search_players = 搜尋玩家...
ui-palette-my_matches_in_ship = 我使用某艦船的比賽...
ui-palette-open_ship_in_armor_viewer = 在模型檢視器中開啟艦船...
ui-palette-open_ship_in_shiptool = 在 ShipTool 中開啟艦船...
ui-palette-players_title = 搜尋玩家
ui-palette-my_ships_title = 我使用某艦船的比賽
ui-palette-armor_ships_title = 在模型檢視器中開啟艦船
ui-palette-shiptool_ships_title = 在 ShipTool 中開啟艦船
ui-palette-players_placeholder = 按名稱搜尋玩家...
ui-palette-my_ships_placeholder = 搜尋您使用過的艦船...
ui-palette-armor_ships_placeholder = 按名稱搜尋所有艦船...
ui-palette-armor_ship_row = { $name } -- 階級 { $tier } { $class }，{ $nation }
ui-palette-back_hint = Esc：返回
ui-palette-ships_loading = 正在載入艦船列表...
ui-palette-ships_unavailable = 沒有艦船列表：{ $reason }
ui-palette-matches_found = { $count } 個匹配結果
ui-palette-nothing_indexed = 尚未建立任何索引
ui-palette-sample_stats = 偵錯：在統計中顯示範例資料
ui-palette-restore_stats = 偵錯：在統計中顯示記錄的資料

## ui.replay.hover
ui-replay-hover-ship = 艦船
ui-replay-hover-map = 地圖
ui-replay-hover-mode = 模式
ui-replay-hover-blank_mode = 空白
ui-replay-hover-played = 遊戲時間
ui-replay-hover-damage = 傷害
ui-replay-hover-kills = 擊沉
ui-replay-hover-result = 結果
ui-replay-hover-division = 分隊
ui-replay-hover-twitch = Twitch 聊天室

## ui.replay.renderer
ui-replay-renderer-export_video = 渲染為影片
ui-replay-renderer-export_clipboard = 渲染到剪貼簿
ui-replay-renderer-export_saved = 影片已儲存至 { $path }
ui-replay-renderer-export_copied = 影片已複製到剪貼簿
ui-replay-renderer-export_not_written = 無法算繪影片：{ $reason }
ui-replay-renderer-pop_out = 在獨立視窗中開啟
ui-replay-renderer-export_path_unusable = 無法寫入該位置
ui-replay-renderer-batch_started = 正在將 { $count } 個回放渲染到 { $dir }
ui-replay-renderer-batch_finished = 已渲染 { $written } 個回放，{ $failed } 個無法渲染
ui-replay-renderer-batch_all_written = 已渲染 { $written } 個回放
ui-replay-renderer-batch_copied = 已渲染 { $written } 個回放並放入剪貼簿
ui-replay-renderer-batch_not_copied = 已渲染到 { $dir }，但剪貼簿拒絕接收：{ $reason }
ui-replay-renderer-batch_progress = 正在渲染 { $done } / { $total }：{ $replay }
ui-replay-renderer-baking = 正在讀取回放並重放戰鬥...
ui-replay-renderer-failed = 無法播放此回放
ui-replay-renderer-play = 播放
ui-replay-renderer-pause = 暫停

## ui.renderer.annotations
ui-renderer-annotations-title = 繪圖工具
ui-renderer-annotations-undo = 復原
ui-renderer-annotations-redo = 重做
ui-renderer-annotations-ship_hint = 尋找艦船
ui-renderer-annotations-no_ship = 未選擇艦船
ui-renderer-annotations-nib = 線條粗細
ui-renderer-annotations-thinner = 線條變細
ui-renderer-annotations-thicker = 線條變粗
ui-renderer-annotations-shortcuts_title = 鍵盤快捷鍵
ui-renderer-annotations-shortcuts_tools = 工具
ui-renderer-annotations-shortcuts_actions = 操作
ui-renderer-annotations-tool_select = 選擇
ui-renderer-annotations-tool_arrow = 箭頭
ui-renderer-annotations-tool_freehand = 自由繪製
ui-renderer-annotations-tool_line = 直線
ui-renderer-annotations-tool_circle = 圓形
ui-renderer-annotations-tool_rectangle = 矩形
ui-renderer-annotations-tool_triangle = 三角形
ui-renderer-annotations-tool_measurement = 測量
ui-renderer-annotations-tool_eraser = 橡皮擦
ui-renderer-annotations-action_undo = 復原
ui-renderer-annotations-action_multi_select = 多選
ui-renderer-annotations-action_stroke_width = 線條粗細
ui-renderer-annotations-action_delete = 刪除
ui-renderer-annotations-action_cancel = 取消 / 取消選擇
ui-renderer-annotations-toggle_fill = 再次點擊以切換填滿
ui-renderer-annotations-clear_all = 全部清除
ui-renderer-annotations-edit = 編輯繪圖
ui-renderer-annotations-size = 大小
ui-renderer-annotations-color = 顏色
ui-renderer-annotations-custom_ink = 自訂顏色
ui-renderer-annotations-filled = 填滿
ui-renderer-annotations-locked = 主持人已鎖定標註

## ui.armor.cast
ui-armor-cast-prompt = { $key }+點擊艦體以新增命中點
ui-armor-cast-empty = 尚無命中點
ui-armor-cast-miss_nothing_under_pointer = 指標下沒有裝甲板
ui-armor-cast-miss_thickness_unknown = 此模型的裝甲板厚度未知，無法投射
ui-armor-cast-miss_no_plate_on_path = 從此角度投射的砲彈彈道未穿過任何裝甲板

## ui.armor.pen
ui-armor-pen-search_hint = 搜尋要比較的艦船...
ui-armor-pen-add_a_ship = 在上方搜尋並新增艦船以比較貫穿力
ui-armor-pen-hover_a_plate = 將滑鼠懸停在艦船的裝甲板上進行檢查
ui-armor-pen-awaiting_plate = 請懸停裝甲板
ui-armor-pen-clear_all = 全部清除
ui-armor-pen-remove = 從比較中移除
ui-armor-pen-title = 貫穿力
ui-armor-pen-title_tooltip = 檢視艦船砲彈對裝甲板的效果
ui-armor-pen-ship = 射擊艦船
ui-armor-pen-firing_ship_tooltip = 使用此艦船的砲彈進行彈道投射
ui-armor-pen-use_for_trajectory = 用於彈道
ui-armor-pen-pick_ship = 選擇向此艦船射擊的艦船
ui-armor-pen-enter_thickness = 輸入裝甲板厚度
ui-armor-pen-no_shells = { $ship } 沒有主砲砲彈
ui-armor-pen-plate = 裝甲板
ui-armor-pen-mm = mm
ui-armor-pen-ifhe = IFHE
ui-armor-pen-ifhe_tooltip = HE 彈的慣性引信使 HE 彈貫穿力提高四分之一
ui-armor-pen-last_hovered = 上次懸停：{ $plate }
ui-armor-pen-penetrates = 擊穿
ui-armor-pen-shatters = 未擊穿
ui-armor-pen-angle_dependent = 取決於角度
ui-armor-pen-no_data = 沒有貫穿力資料

## ui.armor.splash
ui-armor-splash-hint = 在爆風模式下點擊裝甲，查看爆炸波及的區域
ui-armor-splash-half_extent = 爆炸半寬：{ $value } 模型單位
ui-armor-splash-triangles = 爆炸範圍內的裝甲三角形：{ $total }，擊穿：{ $penetrated }
ui-armor-splash-no_zones = 此處沒有重疊的爆風框
ui-armor-splash-zones = 爆炸範圍內的區域
ui-armor-splash-direct_hit = 直接命中
ui-armor-splash-hp = { $value } HP
ui-armor-splash-section = 爆風區域

## ui.armor.export
ui-armor-export-hull = 艦體
ui-armor-export-contents = 內容
ui-armor-export-contents_mesh = 網格
ui-armor-export-contents_armor = 裝甲
ui-armor-export-contents_both = 網格和裝甲
ui-armor-export-lod = 網格細節
ui-armor-export-lod_tooltip = 0 為最高細節。裝甲沒有細節級別，不受影響。
ui-armor-export-res = 紋理解析度
ui-armor-export-res_full = 原始
ui-armor-export-res_2048 = 2048 px
ui-armor-export-res_1024 = 1024 px
ui-armor-export-res_512 = 512 px
ui-armor-export-camos = 迷彩
ui-armor-export-camo_select_none = 全不選
ui-armor-export-camo_reset = 恢復預設
ui-armor-export-loading_details = 載入中...
ui-armor-export-size_estimate = 預計檔案大小：{ $size }
ui-armor-export-size_breakdown = 幾何體 { $geometry }（精確），紋理 { $textures }（估算）
ui-armor-export-size_incomplete = 無法估算 { $count } 個迷彩的大小，總大小可能不完整
ui-armor-export-exporting = 正在匯出 { $ship }...
ui-armor-export-close_button = 關閉
ui-armor-export-already_running = 已有匯出任務在執行
ui-armor-export-exported = 已匯出 { $ship }
ui-armor-export-exported_with_size = 已匯出 { $ship }（{ $size }）
ui-armor-export-vehicle_not_found = 未找到艦船
ui-armor-export-create_file_failed = 建立檔案失敗：{ $error }
ui-armor-export-export_failed = 匯出失敗：{ $error }

## ui.tactics
ui-tactics-a_peers_board = 工作階段中其他人共享的戰術板
ui-tactics-map = 地圖
ui-tactics-map_hint = 尋找地圖
ui-tactics-mode = 模式
ui-tactics-pick_a_map = 選擇要繪製的地圖
ui-tactics-drawing = 正在繪製地圖...
ui-tactics-reset_view = 重置檢視
ui-tactics-add_cap = 新增佔領點
ui-tactics-add_cap_tooltip = 在下次點擊地圖時放置佔領點
ui-tactics-selected_cap = 佔領點 { $letter }
ui-tactics-cap_radius = { $km } km
ui-tactics-cap_team = 隊伍
ui-tactics-cap_team_tooltip = 循環切換所選佔領點的隊伍
ui-tactics-team_neutral = 中立
ui-tactics-team_one = 隊伍 1（綠色）
ui-tactics-team_two = 隊伍 2（紅色）
ui-tactics-delete_cap = 刪除
ui-tactics-delete_cap_tooltip = 移除所選佔領點
ui-tactics-cap_replay_delete_tooltip = 無法刪除從回放載入的佔領點
ui-tactics-cap_narrow_tooltip = 縮小所選佔領點的半徑
ui-tactics-cap_widen_tooltip = 增大所選佔領點的半徑
ui-tactics-undo_tooltip = 復原上一次戰術板更改（{ $shortcut }）
ui-tactics-redo_tooltip = 重做上一次復原的戰術板更改（{ $shortcut } 或 { $alternate }）
ui-tactics-clear_caps = 清除新增的佔領點
ui-tactics-clear_added_caps_tooltip = 移除使用者新增的佔領點
ui-tactics-preset_name = 戰術板名稱
ui-tactics-preset_save = 儲存
ui-tactics-preset_delete = 刪除
ui-tactics-reset_board = 重置戰術板
ui-tactics-reset_board_tooltip = 恢復所選模式的佔領點並清除繪圖
ui-tactics-preset_needs_a_name = 請先為戰術板命名
ui-tactics-preset_needs_a_map = 請先為戰術板選擇地圖
ui-tactics-preset_saved = 已儲存 { $name }
ui-tactics-preset_save_failed = 無法儲存戰術板：{ $error }
ui-tactics-preset_load_failed = 無法開啟戰術板：{ $error }
ui-tactics-preset_delete_failed = 無法刪除戰術板：{ $error }
ui-tactics-presets = 已儲存的戰術板
ui-tactics-preset_open = 開啟
ui-tactics-clear_drawing = 清除繪圖
ui-tactics-scan_replays = 從回放填充佔領點
ui-tactics-scan_replays_tooltip = 讀取回放，獲取其所在模式的佔領點佈局
ui-tactics-scan_needs_game_data = 請先載入一個遊戲組建
ui-tactics-scan_running = 正在讀取回放...
ui-tactics-scan_progress = 正在讀取回放... { $read } / { $total }
ui-tactics-scan_done = 已讀取 { $total } 個回放，發現 { $added } 個新佈局
ui-tactics-session_boards_full = 工作階段中已開啟 { $limit } 個戰術板，因此此戰術板維持在目前的地圖
ui-tactics-placing = 正在放置 { $ship }
ui-tactics-side_friendly = 友方
ui-tactics-side_enemy = 敵方
ui-tactics-ink_white = 白色墨跡
ui-tactics-ink_gray = 灰色墨跡
ui-tactics-ink_red = 紅色墨跡
ui-tactics-ink_orange = 橙色墨跡
ui-tactics-ink_yellow = 黃色墨跡
ui-tactics-ink_green = 綠色墨跡
ui-tactics-ink_blue = 藍色墨跡
ui-tactics-ink_purple = 紫色墨跡
ui-tactics-ink_pink = 粉紅色墨跡
ui-tactics-no_map_art = 此組建不包含該地圖的影像
ui-tactics-ship_config = 艦船配置
ui-tactics-hull = 船體
ui-tactics-hull_default = 預設
ui-tactics-modifiers = 修正
ui-tactics-concealment_expert = 隱蔽專家
ui-tactics-gun_range_mod = 主砲射程改裝
ui-tactics-secondary_range_mod = 副砲射程改裝
ui-tactics-ship_ranges = 射程圈
ui-tactics-shape = 形狀
ui-tactics-shape_size = 大小
ui-tactics-shape_size_km = 大小 (公里)
ui-tactics-shape_filled = 填滿
ui-tactics-shape_smaller = 縮小形狀
ui-tactics-shape_larger = 放大形狀

## ui.armor.outcome
ui-armor-outcome-penetration = 擊穿
ui-armor-outcome-citadel = 核心區
ui-armor-outcome-ricochet = 跳彈
ui-armor-outcome-shatter = 未擊穿
ui-armor-outcome-overpenetration = 過度擊穿
ui-armor-outcome-underwater = 水下
ui-armor-outcome-unknown = 無法識別（{ $raw }）
ui-armor-outcome-stopped = 停在裝甲中

## ui.armor.sim
ui-armor-sim-overmatch_rules_out_ricochet = 口徑碾壓，不會跳彈
ui-armor-sim-always_ricochet_band = 跳彈，處於必定跳彈角度區間
ui-armor-sim-ricochet_angle_too_low = 角度過小，不可能跳彈

## ui.launcher
ui-launcher-rescan = 重新掃描
ui-launcher-add_install = 新增安裝...
ui-launcher-add_install_title = World of Warships 安裝目錄
ui-launcher-no_installs = 未找到已安裝的遊戲。請新增一個，或在下方安裝遊戲。
ui-launcher-managed_by_steam = 由 Steam 管理
ui-launcher-checking_updates = 正在檢查更新
ui-launcher-up_to_date = 已是最新
ui-launcher-update_available = 有可用更新：{ $parts }
ui-launcher-not_installed = 未安裝：{ $parts }
ui-launcher-check_failed = 檢查更新失敗：{ $error }
ui-launcher-recheck = 重新檢查
ui-launcher-download = 下載 / 更新...
ui-launcher-download_title = 組建的安裝位置
ui-launcher-download_hint = 全新安裝需要數十 GB。目前尚不支援增量更新，因此有變化的部分會完整重新下載。
ui-launcher-pause = 暫停
ui-launcher-cancel = 取消
ui-launcher-resume = 繼續
ui-launcher-paused = 下載已暫停，已暫存的檔案會保留。
ui-launcher-installed = 已安裝
ui-launcher-installed_skipped = 已安裝（跳過了 { $count } 個增量部分：尚不支援，已下載但未應用）
ui-launcher-download_failed = 下載失敗：{ $error }
ui-launcher-update_in_place = 更新會安裝到該安裝本身的資料夾中。請選擇 { $path } 進行更新。
ui-launcher-cancelled = 下載已取消。
ui-launcher-launch_failed = 啟動失敗：{ $error }
ui-launcher-use_for_toolkit = 供 Toolkit 使用
ui-launcher-not_set = 未設定
ui-launcher-browse = 瀏覽...
ui-launcher-install_heading = 安裝遊戲
ui-launcher-install_hint = 全新安裝需要數十 GB。
ui-launcher-install_title = 遊戲的安裝位置
ui-launcher-install = 安裝...
ui-launcher-checking_version = 正在檢查可用版本
ui-launcher-version = 版本 { $version }
ui-launcher-version_unreported = 未報告用戶端版本
ui-launcher-unavailable = 不可用：{ $error }
ui-launcher-retry = 重試

## ui.launcher.account
ui-launcher-account-sign_in = 登入
ui-launcher-account-add_title = 新增帳號
ui-launcher-account-again_title = 重新登入
ui-launcher-account-memory_only = 僅在本次工作階段中保持登入。登入資訊不會儲存，因此重新啟動後需要再次登入。
ui-launcher-account-password_note = 您的密碼只會傳送給 Wargaming，且絕不會被儲存。
ui-launcher-account-realm_fixed = 伺服器：{ $realm }
ui-launcher-account-email = 電子郵件
ui-launcher-account-password = 密碼
ui-launcher-account-code = 兩步驟驗證碼
ui-launcher-account-code_required = 需要兩步驟驗證碼。
ui-launcher-account-use_backup_code = 使用備用驗證碼
ui-launcher-account-submit_code = 提交驗證碼
ui-launcher-account-captcha = 此次登入需要驗證碼（CAPTCHA），但本啟動器目前無法顯示。請先透過官方 Wargaming Game Center 或在瀏覽器中登入 wargaming.net 一次，然後重試。

## ui.launcher.sign_in_error
ui-launcher-sign_in_error-wrong_credentials = 電子郵件或密碼錯誤。
ui-launcher-sign_in_error-two_factor_required = 此帳號需要雙重驗證核准。請在 Wargaming Auth 中或使用郵件中的驗證碼核准登入，然後再次登入。
ui-launcher-sign_in_error-two_factor_invalid = 雙重驗證碼錯誤或已過期。請輸入新的驗證碼。
ui-launcher-sign_in_error-captcha_required = Wargaming 要求完成驗證碼（CAPTCHA）。請先在官方 Game Center 或 wargaming.net 登入一次，然後再試一次。
ui-launcher-sign_in_error-rate_limited = 登入嘗試次數過多。請等待幾分鐘後再試一次。
ui-launcher-sign_in_error-account_banned = 此帳號已被停權。
ui-launcher-sign_in_error-network_unreachable = 無法連線至 Wargaming。請檢查您的網路連線。
ui-launcher-sign_in_error-service_unavailable = Wargaming 登入服務無法使用。請稍後再試。
ui-launcher-sign_in_error-session_expired = 已儲存的登入已過期。請再次登入。
ui-launcher-sign_in_error-keyring_locked = 系統金鑰圈已鎖定。請將其解鎖後再次登入。
ui-launcher-sign_in_error-not_saved = 已登入，但無法儲存登入資訊。重新啟動後需要再次登入。
ui-launcher-sign_in_error-unexpected = 登入失敗。詳細資訊請查看記錄。

## ui.launcher.realm
ui-launcher-realm-eu = 歐洲
ui-launcher-realm-na = 北美
ui-launcher-realm-asia = 亞洲

## ui.launcher.runner
ui-launcher-runner-looking = 正在尋找 Proton。
ui-launcher-runner-steam_proton = Steam 的 { $name }
ui-launcher-runner-umu = umu-run（{ $name }）
ui-launcher-runner-umu_default = umu-run（UMU-Proton）
ui-launcher-runner-native = 遊戲以原生方式執行。
ui-launcher-runner-first_run_prefix = 首次啟動會建立 Wine 前綴，需要幾秒鐘。
ui-launcher-runner-first_run_download = 首次啟動會下載 UMU-Proton 與 Steam 執行環境，約 700 MB。
ui-launcher-runner-choose = 選擇 Proton 資料夾
ui-launcher-runner-automatic = 自動尋找
ui-launcher-runner-detect = 重新尋找
ui-launcher-runner-needed = 在 Linux 上啟動需要 Proton。請安裝含 Proton 的 Steam，或安裝 umu-launcher。

## ui.replay.damage_share
ui-replay-damage_share-mode = 傷害佔對方艦船總量的比例
ui-replay-damage_share-dealt = { $ship }造成的傷害中有{ $pct }打在{ $other }身上。
ui-replay-damage_share-dealt_inverse = { $other }受到的傷害中有{ $pct }來自{ $ship }。
ui-replay-damage_share-received = { $ship }受到的傷害中有{ $pct }來自{ $other }。
ui-replay-damage_share-received_inverse = { $other }造成的傷害中有{ $pct }打在{ $ship }身上。
ui-replay-damage_share-hold = 按住{ $key }以反轉。
ui-replay-damage_share-release = 放開{ $key }以還原。

## ui.reindex
ui-reindex-context_one = 強制重建索引
ui-reindex-context_many = 強制為 { $count } 個回放重建索引
ui-reindex-palette_by_version = 強制重建回放索引：依版本
ui-reindex-palette_all = 強制重建回放索引：全部
ui-reindex-versions_title = 強制重建回放索引：依版本
ui-reindex-versions_placeholder = 篩選遊戲版本...
ui-reindex-version_row = { $version } -- { $count } 個回放
ui-reindex-no_versions = 清單中沒有回放標明遊戲版本
ui-reindex-confirm_title = 強制重建回放索引
ui-reindex-confirm_ok = 重建索引
ui-reindex-confirm_count = 將重新讀取 { $count } 個回放並重寫其索引項目。
ui-reindex-confirm_range = 遊戲時間從 { $oldest } 到 { $newest }。
ui-reindex-confirm_undated = 待讀取的 { $count } 個回放沒有可讀的戰鬥日期，不在該範圍內。
ui-reindex-confirm_downloads = 將先下載以下版本的遊戲資料：
ui-reindex-confirm_unobtainable = 無法取得以下版本的遊戲資料，因此略過其 { $count } 個回放：
ui-reindex-confirm_no_cache_dir = 未設定遊戲資料快取目錄，因此無法下載以下版本的遊戲資料，略過其 { $count } 個回放：
ui-reindex-confirm_no_build = { $count } 個回放未標明遊戲組建，已略過。
ui-reindex-confirm_no_version = { $count } 個回放未標明可讀的遊戲版本，已略過。
ui-reindex-build_row = { $version }（組建 { $build }）：{ $count } 個回放
ui-reindex-nothing_listed = 清單中沒有需要重建索引的回放。
ui-reindex-nothing_readable = 這些回放皆無法重建索引：沒有可用的遊戲資料（略過 { $count } 個）。
ui-reindex-no_game_data = 設定遊戲目錄之前無法讀取回放。
ui-reindex-busy_index = 索引作業已在執行。
ui-reindex-busy_cache = 遊戲資料作業已在執行。請在其完成後重試。
ui-reindex-planning = 正在檢查可用的遊戲資料...
ui-reindex-no_index = 回放索引未開啟，因此未重建任何索引。
ui-reindex-done = 已為 { $indexed } 個回放重建索引；略過 { $skipped } 個，{ $failed } 個無法讀取
ui-reindex-stopped = 重建索引已停止：已重建 { $indexed } 個，略過 { $skipped } 個，{ $failed } 個無法讀取
ui-reindex-stopped_before_reading = 在讀取任何回放之前已停止重建索引。

## ui.index_folder
ui-index_folder-tooltip = 為此清單中回放索引尚未包含的所有回放建立索引
ui-index_folder-confirm_title = 為回放建立索引
ui-index_folder-confirm_ok = 建立索引
ui-index_folder-confirm_count = 此清單中有 { $count } 個回放尚未加入回放索引，將讀取並新增至索引中。
ui-index_folder-note_live = 已建立索引的回放會新增至搜尋和玩家追蹤所讀取的回放索引中。
ui-index_folder-note_folder = 已建立索引的回放會以此目錄自身的來源 ({ $dir }) 新增至回放索引中。搜尋和玩家追蹤的玩家清單會包含這些回放。玩家追蹤的戰隊清單只讀取遊戲自身的回放。
ui-index_folder-nothing_listed = 此清單中沒有可建立索引的回放。
ui-index_folder-nothing_missing = 此清單中的所有回放均已建立索引。
ui-index_folder-unreadable_index = 無法讀取回放索引。
ui-index_folder-done = 已為 { $indexed } 個回放建立索引；略過 { $skipped } 個，{ $failed } 個無法讀取
ui-index_folder-stopped = 索引已停止：已索引 { $indexed } 個，略過 { $skipped } 個，{ $failed } 個無法讀取
ui-index_folder-stopped_before_reading = 索引在讀取任何回放之前已停止。

## ui.replay_folders
ui-replay_folders-install = 遊戲
ui-replay_folders-watched = 已監視：新增和變更的回放會顯示在此處
ui-replay_folders-unwatched = 未監視：使用重新整理重新讀取
ui-replay_folders-unseen = 自上次檢視以來有 { $count } 個新增或變更的回放
ui-replay_folders-missing = 此目錄已不存在：{ $dir }
ui-replay_folders-missing_hint = 在取消釘選之前，每次啟動時都會在此處開啟。
ui-replay_folders-pin = 釘選
ui-replay_folders-unpin = 取消釘選
ui-replay_folders-refresh = 重新整理
ui-replay_folders-close = 關閉
ui-replay_folders-pins_unreadable = 無法讀取已釘選的回放目錄。
ui-replay_folders-game_data_available = 列出的回放有可用的遊戲資料。
ui-replay_folders-browse = 瀏覽...
ui-replay_folders-recent_title = 開啟回放目錄
ui-replay_folders-recent_placeholder = 篩選最近的目錄...
ui-replay_folders-recent_missing = 此目錄已不存在，已從最近清單中移除：{ $dir }
ui-replay_folders-checking = 正在尋找 { $dir }...
ui-replay_folders-indexing = 正在為 { $dir } 建立索引
ui-replay_folders-indexing_in_background = 此目錄仍在建立索引
ui-replay_folders-search_while_indexing = 此目錄仍在建立索引，因此結果尚不完整。完成後請重新搜尋。
ui-replay_folders-source_failed = 無法為此目錄的回放建立索引：{ $error }
ui-replay_folders-restart_watch = 重新開始監視
ui-replay_folders-watch_stopped = { $folder } 的監視已停止：{ $cause }。
ui-replay_folders-watch_restarting = 將在 { $seconds } 秒後重新開始（第 { $attempt } 次，共 { $attempts } 次）。
ui-replay_folders-watch_gave_up = 已重試 { $attempts } 次皆失敗，不再自動重試。
ui-replay_folders-watch_restart_hint = 點擊紅點或在分頁選單中選擇「重新開始監視」即可立即重新開始。
ui-replay_folders-watch_resumed = { $folder } 的監視已恢復。
ui-replay_folders-watch_cause_backend = 檔案系統回報了錯誤：{ $error }
ui-replay_folders-watch_cause_gone = 目錄已被移動或刪除
ui-replay_folders-watch_cause_panicked = 內部錯誤：{ $error }
ui-replay_folders-watch_cause_exited = 監視意外結束
ui-replay_folders-dropped = 拖放
ui-replay_folders-dropped_numbered = 拖放 { $number }
ui-replay_folders-dropped_count = { $count } 個拖放的回放
ui-replay_folders-dropped_no_source = 拖放的回放會依其所在目錄建立索引。開啟目錄即可搜尋或建立索引。
ui-replay_folders-unreadable_replay = 無法讀取：{ $error }

## ui.models
ui-models-heading = 模型
ui-models-loading = 正在載入模型...
ui-models-unavailable = 模型無法使用
ui-models-search = 搜尋艦船與模型...
ui-models-open = 開啟
ui-models-export = 匯出模型
ui-models-copy_path = 複製模型路徑
ui-models-path_copied = 已複製 { $path }
ui-models-skin_replaces = 裝備 { $exterior } 時替換 { $model }
ui-models-geometry_absent = 不在遊戲檔案中
ui-models-geometry_none = 此模型沒有幾何體
ui-models-hardpoints_tooltip = 顯示掛載點
ui-models-common = 通用模型

## ui.models.group
ui-models-group-gameplay = 遊戲
ui-models-group-building = 建築
ui-models-group-character = 角色
ui-models-group-location = 場景
ui-models-group-port = 港口
ui-models-group-style = 風格
ui-models-group-other = 其他模型

## ui.models.building
ui-models-building-weapon = 建築武器
ui-models-building-unassigned = 未分配建築
ui-models-building-unspecified = 無類型建築

## ui.models.misc
ui-models-misc-crane = 起重機
ui-models-misc-anchor = 錨
ui-models-misc-deck_fitting = 甲板屬具
ui-models-misc-instrument = 儀器
ui-models-misc-propeller = 螺旋槳
ui-models-misc-searchlight = 探照燈
ui-models-misc-boat = 小艇
ui-models-misc-flag = 旗幟
ui-models-misc-insignia = 徽記
ui-models-misc-railing = 欄杆
ui-models-misc-hatch = 艙口
ui-models-misc-ventilation = 通風設備
ui-models-misc-antenna = 天線
ui-models-misc-weapon = 武器
ui-models-misc-stowage = 儲物
ui-models-misc-other = 其他

## ui.models.variant
ui-models-variant-dead = 殘骸
ui-models-variant-skin = 外觀

## ui.models.kind
ui-models-kind-ship = 艦船
ui-models-kind-gun = 火砲
ui-models-kind-director = 射擊指揮儀
ui-models-kind-finder = 測距儀
ui-models-kind-radar = 雷達
ui-models-kind-catapult = 彈射器
ui-models-kind-aircraft = 飛機
ui-models-kind-projectile = 彈藥
ui-models-kind-misc = 雜項
ui-models-kind-cap = 佔領點
ui-models-kind-other = 其他

## ui.models.sub_kind
ui-models-sub_kind-battleship = 戰艦
ui-models-sub_kind-cruiser = 巡洋艦
ui-models-sub_kind-destroyer = 驅逐艦
ui-models-sub_kind-aircraft_carrier = 航空母艦
ui-models-sub_kind-submarine = 潛艦
ui-models-sub_kind-auxiliary = 輔助艦
ui-models-sub_kind-main_battery = 主砲
ui-models-sub_kind-secondary_battery = 副砲
ui-models-sub_kind-anti_aircraft = 防空砲
ui-models-sub_kind-torpedo_tubes = 魚雷發射管
ui-models-sub_kind-depth_charge_launcher = 深水炸彈投射器
ui-models-sub_kind-missile_launcher = 飛彈發射器
ui-models-sub_kind-search_radar = 搜索雷達
ui-models-sub_kind-fire_control_radar = 射控雷達
ui-models-sub_kind-fighter = 戰鬥機
ui-models-sub_kind-bomber = 轟炸機
ui-models-sub_kind-dive_bomber = 俯衝轟炸機
ui-models-sub_kind-scout = 偵察機
ui-models-sub_kind-torpedo = 魚雷
ui-models-sub_kind-bomb = 炸彈
ui-models-sub_kind-rocket = 火箭
ui-models-sub_kind-depth_charge = 深水炸彈
ui-models-sub_kind-mine = 水雷
ui-models-sub_kind-missile = 飛彈
ui-models-sub_kind-jato = 起飛助推火箭
ui-models-sub_kind-shell = 砲彈

## ui.models.part
ui-models-part-hull = 艦體
ui-models-part-main_battery = 主砲
ui-models-part-secondaries = 副砲
ui-models-part-anti_aircraft = 防空
ui-models-part-torpedoes = 魚雷
ui-models-part-directors = 射擊指揮儀
ui-models-part-finders = 測距儀
ui-models-part-radars = 雷達
ui-models-part-depth_charges = 深水炸彈
ui-models-part-aircraft = 飛機
ui-models-part-other = 其他
ui-models-part-misc = 雜項

## ui.model_view
ui-model_view-thickness_from_building = 厚度來自 { $building }
ui-model_view-thickness_from = 厚度來自 { $ship } ({ $hp })
ui-model_view-thickness_unknown = 厚度未知: 沒有艦船搭載此模型
ui-model_view-thickness_other_ships = 其他艦船
ui-model_view-export_title = 匯出模型
ui-model_view-export_tooltip = 將模型匯出為 GLB 檔案
ui-model_view-part_model = 模型：{ $model }
ui-model_view-part_hardpoint = 掛載點：{ $hardpoint }

## ui.model_view.part_group
ui-model_view-part_group-hull = 艦體
ui-model_view-part_group-main_battery = 主砲
ui-model_view-part_group-secondary_battery = 副砲
ui-model_view-part_group-aa_guns = 防空
ui-model_view-part_group-torpedoes = 魚雷
ui-model_view-part_group-depth_charges = 深水炸彈
ui-model_view-part_group-fire_control = 火控
ui-model_view-part_group-radar = 雷達
ui-model_view-part_group-missiles = 飛彈發射器
ui-model_view-part_group-decorations = 裝飾
ui-model_view-part_group-misc = 雜項
ui-model_view-part_group-armor = 裝甲
ui-model_view-part_group-other = 其他

## ui.scene_problem
ui-scene_problem-cannot_show = 無法顯示 { $name }
ui-scene_problem-geometry_absent = 此模型的幾何資料不在你的遊戲檔案中。
ui-scene_problem-model_unreadable = 無法讀取此模型的資料。詳細資訊請見日誌。
ui-scene_problem-ship_unreadable = 無法讀取此艦船的資料。詳細資訊請見日誌。
ui-scene_problem-no_meshes = 此模型沒有網格。
ui-scene_problem-no_meshes_at_lod = 此模型在所選細節等級下沒有網格。
ui-scene_problem-no_visible_surface = 此模型沒有可繪製的可見表面。
ui-scene_problem-section_stand_in = 此船體分段沒有自己的幾何資料。艦船在另一個分段中繪製這部分船體。

## ui.armor.option_tabs
ui-armor-option_tabs-label = 選項分頁
ui-armor-option_tabs-ship = 艦船
ui-armor-option_tabs-layers = 圖層
ui-armor-option_tabs-analysis = 分析
ui-armor-option_tabs-configuration = 配置
ui-armor-option_tabs-hull_parts = 船體部件
ui-armor-option_tabs-tab_matches = { $tab }，{ $count } 項符合
ui-armor-option_tabs-empty = 此分頁中沒有適用於目前模型的內容。
ui-armor-option_tabs-no_choices = 此模型沒有可選擇的船體、模組、迷彩或細節等級。

## ui.armor.hidden_ship
ui-armor-hidden_ship-clan_rental = 公會租借
ui-armor-hidden_ship-other_class = 不是艦種

## ui.armor.textures
ui-armor-textures-title = 貼圖
ui-armor-textures-hint = 遊戲會讀取組建 res_mods 資料夾下的檔案，以取代封裝中相同路徑的檔案。右鍵點擊貼圖即可複製該路徑。
ui-armor-textures-under_pointer = 指標下：{ $material }
ui-armor-textures-reading = 正在讀取貼圖...
ui-armor-textures-summary = { $materials } 個材質，{ $textures } 個貼圖
ui-armor-textures-no_material = 無材質
ui-armor-textures-unresolved = 此路徑不在 assets.bin 中
ui-armor-textures-absent = 不在已安裝的封裝中
ui-armor-textures-unreadable = 無法讀取 DDS 標頭
ui-armor-textures-unknown_format = 無法辨識的格式
ui-armor-textures-details_one = { $format }，{ $size }，1 個檔案，{ $bytes }
ui-armor-textures-details = { $format }，{ $size }，{ $count } 個檔案，{ $bytes }
ui-armor-textures-role_albedo = 反照率
ui-armor-textures-role_normal = 法線
ui-armor-textures-role_metal_gloss = 金屬光澤
ui-armor-textures-role_ambient_occlusion = 環境光遮蔽
ui-armor-textures-role_detail = 細節
ui-armor-textures-role_name_matched = 反照率（依檔名找到）
ui-armor-textures-copy_path = 複製路徑
ui-armor-textures-copy_override = 複製 res_mods 路徑
ui-armor-textures-copy_overrides = 複製 { $count } 個 res_mods 路徑
ui-armor-textures-paths_copied = 已複製 { $count } 個路徑
ui-armor-textures-no_build = 遊戲資料尚未載入，因此無法得知此組建的模組資料夾。
ui-armor-textures-show_in_unpacker = 在資源解包器中顯示
ui-armor-textures-show_textures = 顯示貼圖
ui-armor-textures-extract = 匯出檔案...
ui-armor-textures-extract_title = 將貼圖檔案匯出至
ui-armor-textures-extracting = 正在匯出貼圖檔案...
ui-armor-textures-extracted = 已將 { $count } 個檔案匯出至 { $folder }
ui-armor-textures-extract_failed = 無法匯出貼圖檔案：{ $reason }
ui-armor-textures-replace_title = 取代現有檔案？
ui-armor-textures-replace_description = 其中 { $count } 個檔案已存在於 { $folder }。匯出將會取代它們。
ui-armor-textures-replace = 取代

## ui.settings.mcp
ui-settings-mcp-enabled = 啟用 MCP 伺服器
ui-settings-mcp-enabled_description = 透過 MCP 向 AI 助理提供重播和遊戲資料。在狀態列顯示控制按鈕。
ui-settings-mcp-start_on_launch = 隨程式啟動
ui-settings-mcp-start_on_launch_description = 開啟 WoWs Toolkit 時啟動伺服器。
ui-settings-mcp-bind_address = 繫結位址
ui-settings-mcp-bind_address_description = 伺服器監聽的回送 IP 位址，例如 127.0.0.1 或 ::1。只有這台電腦能連線到伺服器。
ui-settings-mcp-port = 連接埠
ui-settings-mcp-port_description = 監聽的 TCP 連接埠。首次啟用時隨機選擇並保留。
ui-settings-mcp-port_unset = 首次啟用時選擇
ui-settings-mcp-address_not_loopback = { $input } 不是回送位址。伺服器只在這台電腦上監聽，例如 127.0.0.1。
ui-settings-mcp-address_invalid = { $input } 不是 IP 位址
ui-settings-mcp-port_invalid = { $input } 不是 1 到 65535 之間的連接埠
ui-settings-mcp-port_in_use = 其他程式正在使用此連接埠。請選擇其他連接埠。

## ui.mcp
ui-mcp-started = MCP 伺服器執行於 { $url }
ui-mcp-copy_url = 複製 URL
ui-mcp-start_failed = 無法啟動 MCP 伺服器：{ $reason }
ui-mcp-port_in_use = { $address } 上的連接埠 { $port } 被其他程式佔用
ui-mcp-no_watchdog = 背景工作監督程式未執行
ui-mcp-status_running = MCP 伺服器執行於 { $url }。點擊停止。
ui-mcp-status_starting = MCP 伺服器正在 { $address } 上啟動
ui-mcp-status_failed = MCP 伺服器啟動失敗。點擊重試。
ui-mcp-status_stopped = MCP 伺服器已停止。點擊啟動。

## ui.launcher.opt_in
ui-launcher-opt_in-setting = 啟用遊戲啟動器
ui-launcher-opt_in-description = 在 Linux 上透過 Toolkit 以您的 Wargaming 帳號登入並遊玩 World of Warships。
ui-launcher-opt_in-title = 要啟用遊戲啟動器嗎？
ui-launcher-opt_in-notice_account = 使用啟動器功能即表示您知悉：此功能並非遊戲開發商所支援的使用情境。您將直接在本應用程式中輸入密碼；如果您不信任本應用程式，它可能被用來盜取您的帳號（儘管本應用程式的開發者無意這麼做）。
ui-launcher-opt_in-notice_reports = 此外，您確認：如果您使用此功能，您將不會向遊戲開發商回報圖形或穩定性方面的錯誤，除非您確實認為這是一個跨平台問題，並且並非「僅」影響 Linux。
ui-launcher-opt_in-continue = 繼續
ui-launcher-opt_in-cancel = 取消

## ui.launcher.keyring
ui-launcher-keyring-unencrypted = 由於沒有可用的 Secret Service 提供者（例如 GNOME Keyring、KeePassXC 或 KDE Wallet），您的登入資訊以未加密方式儲存在應用程式的資料夾中。
ui-launcher-keyring-how = 如何保持加密
ui-launcher-keyring-how_detail = 請安裝並啟動 Secret Service 提供者，例如 GNOME Keyring、KeePassXC 或 KDE Wallet，然後重新啟動 Toolkit。已儲存的登入會移至其中。
ui-launcher-keyring-unlock_hint = 系統金鑰圈可能會要求解鎖，以便 Toolkit 讀取您已儲存的登入。

## ui.launcher.play
ui-launcher-play-play = 開始遊戲
ui-launcher-play-play_through_steam = 透過 Steam 開始遊戲
ui-launcher-play-set_up_game = 設定遊戲
ui-launcher-play-set_up_runner = 設定 Proton
ui-launcher-play-sign_in_again = 重新登入
ui-launcher-play-update = 更新 { $size }
ui-launcher-play-updating = 正在更新 { $percent }%
ui-launcher-play-updating_unknown = 正在更新
ui-launcher-play-paused = 已暫停
ui-launcher-play-starting = 正在啟動...
ui-launcher-play-running = 執行中
ui-launcher-play-in_battle = 戰鬥中
ui-launcher-play-menu = 帳號與遊戲選項

## ui.launcher.popover
ui-launcher-popover-game = World of Warships
ui-launcher-popover-no_install = 尚未設定遊戲安裝。
ui-launcher-popover-update_available = 有可用更新
ui-launcher-popover-check_failed = 無法檢查更新
ui-launcher-popover-accounts = 帳號
ui-launcher-popover-no_accounts = 尚無帳號。新增一個帳號即可登入遊玩。
ui-launcher-popover-close_to_switch = 請關閉遊戲以切換帳號。
ui-launcher-popover-default = 預設
ui-launcher-popover-ready = 就緒
ui-launcher-popover-restoring = 正在檢查...
ui-launcher-popover-not_saved = 未儲存
ui-launcher-popover-unencrypted = 未加密儲存
ui-launcher-popover-keyring_locked = 金鑰圈已鎖定：請將其解鎖以還原已儲存的登入
ui-launcher-popover-forget = 移除
ui-launcher-popover-sign_out = 登出
ui-launcher-popover-expires_in = { $duration }後過期
ui-launcher-popover-expired = 工作階段已過期
ui-launcher-popover-expiry_unknown = 過期時間未知
ui-launcher-popover-make_default = 設為預設
ui-launcher-popover-account_actions = 帳號操作
ui-launcher-popover-play_anyway = 仍要開始遊戲
ui-launcher-popover-play_safe_mode = 以安全模式開始遊戲
ui-launcher-popover-add_account = 新增帳號...
ui-launcher-popover-game_settings = 遊戲設定...
ui-launcher-popover-installs = 安裝與設定...
ui-launcher-popover-open_replay = 在遊戲中開啟回放...

## ui.launcher.palette
ui-launcher-palette-play = 開始遊戲
ui-launcher-palette-play_as = 以 { $nickname }（{ $realm }）身分開始遊戲
ui-launcher-palette-game_settings = 遊戲設定
ui-launcher-palette-add_account = 新增帳號
ui-launcher-palette-update = 更新 World of Warships
ui-launcher-palette-open_replay = 在遊戲中開啟回放

## ui.launcher.installs
ui-launcher-installs-toolkit = Toolkit 的遊戲目錄
ui-launcher-installs-played = 預設帳號使用此安裝
ui-launcher-installs-play_from = 從此處開始遊戲
ui-launcher-installs-update = 更新

## ui.launcher.wgc_login
ui-launcher-wgc_login-label = 使用 Wargaming 登入
ui-launcher-wgc_login-about = 透過 Toolkit 的登入（而非 Steam）執行此 Steam 安裝。設定和回放與透過 Steam 執行時分開儲存。此模式下 Steam 錢包、成就和遊戲時數無法使用。
ui-launcher-wgc_login-steam_accounts = 透過 Steam 建立的帳號需要電子郵件和密碼。請先在 Wargaming 帳號管理中新增。
ui-launcher-wgc_login-steam_accounts_link = 如何新增電子郵件和密碼
ui-launcher-wgc_login-ready = Game Center 檔案已就緒
ui-launcher-wgc_login-fetchable = 開啟後將下載此組建的 WG Game Center 檔案（{ $size }）。
ui-launcher-wgc_login-downloading = 正在下載 Game Center 檔案：{ $percent }%
ui-launcher-wgc_login-downloading_unknown = 正在下載 Game Center 檔案
ui-launcher-wgc_login-checking = 正在尋找此組建的 Game Center 檔案...
ui-launcher-wgc_login-needs_sign_in = 請先將 Wargaming 帳號新增至 Toolkit。
ui-launcher-wgc_login-unavailable = 此組建無法使用：{ $error }

## ui.launcher.replay
ui-launcher-replay-no_install = 沒有安裝遊戲組建 { $build }。請保留或安裝該組建的副本以觀看此回放。

## ui.launcher.setup
ui-launcher-setup-title = 設定 World of Warships
ui-launcher-setup-game = 遊戲
ui-launcher-setup-proton = Proton
ui-launcher-setup-accounts = 帳號
ui-launcher-setup-replays = 回放
ui-launcher-setup-game_question = Toolkit 應使用哪個遊戲副本？
ui-launcher-setup-none_found = 在這台電腦上找不到已安裝的遊戲。
ui-launcher-setup-choose_folder = 選擇資料夾...
ui-launcher-setup-install_new = 安裝新副本...
ui-launcher-setup-proton_explain = 在 Linux 上，遊戲透過 Proton 執行：已安裝 Steam 時使用 Steam 的 Proton，否則使用 umu-run。
ui-launcher-setup-proton_ready = 已找到 Proton。
ui-launcher-setup-accounts_explain = 登入您遊玩的 Wargaming 帳號。「開始遊戲」會以預設帳號登入遊戲。
ui-launcher-setup-replays_explain = 當回放數量達到此值時，遊戲會刪除最舊的回放。
ui-launcher-setup-skip = 略過設定
ui-launcher-setup-back = 上一步
ui-launcher-setup-next = 下一步
ui-launcher-setup-done = 完成

## ui.launcher.game_settings
ui-launcher-game_settings-title = 遊戲設定
ui-launcher-game_settings-build = 組建 { $build }
ui-launcher-game_settings-display = 顯示
ui-launcher-game_settings-graphics = 圖形
ui-launcher-game_settings-sound = 音效
ui-launcher-game_settings-replays = 回放
ui-launcher-game_settings-locked = 遊戲正在執行。遊戲結束時會重寫這些檔案，因此在此之前它們為唯讀。
ui-launcher-game_settings-footer = 寫入遊戲的偏好設定檔案。會保留一份備份。
ui-launcher-game_settings-cancel = 取消
ui-launcher-game_settings-save = 儲存
ui-launcher-game_settings-saved = 遊戲設定已儲存。
ui-launcher-game_settings-no_install = 尚未設定遊戲安裝。
ui-launcher-game_settings-window_mode = 視窗模式
ui-launcher-game_settings-windowed = 視窗化
ui-launcher-game_settings-borderless = 無邊框
ui-launcher-game_settings-fullscreen = 全螢幕
ui-launcher-game_settings-monitor = 螢幕
ui-launcher-game_settings-monitor_name = 螢幕名稱
ui-launcher-game_settings-resolution = 全螢幕解析度
ui-launcher-game_settings-width = 寬度
ui-launcher-game_settings-height = 高度
ui-launcher-game_settings-refresh = 更新頻率
ui-launcher-game_settings-display_rate = 顯示頻率
ui-launcher-game_settings-vsync = 垂直同步
ui-launcher-game_settings-ui_scale = 介面縮放
ui-launcher-game_settings-battle_fps = 戰鬥中影格率上限
ui-launcher-game_settings-port_fps = 港口中影格率上限
ui-launcher-game_settings-unlimited = 無限制
ui-launcher-game_settings-skip_intro = 略過開場影片
ui-launcher-game_settings-master = 主音量
ui-launcher-game_settings-music_port = 港口音樂
ui-launcher-game_settings-music_battle = 戰鬥音樂
ui-launcher-game_settings-preset = 預設集
ui-launcher-game_settings-antialiasing = 反鋸齒
ui-launcher-game_settings-textures = 材質品質
ui-launcher-game_settings-filtering = 材質過濾
ui-launcher-game_settings-shadows = 陰影品質
ui-launcher-game_settings-graphics_note = 請在遊戲中變更圖形設定。遊戲會依據您的硬體進行檢查。
ui-launcher-game_settings-replays_kept = 保留的回放數量
ui-launcher-game_settings-replays_kept_zero = 請至少保留一個回放。
ui-launcher-game_settings-versioned = 依遊戲版本分類回放
ui-launcher-game_settings-replays_note = 寫入此組建的 res_mods。安全模式不會使用它。
ui-launcher-game_settings-absent = 遊戲檔案中未設定
ui-launcher-game_settings-unrecognized = 未知的值：{ $value }
ui-launcher-game_settings-not_a_number = { $field }：請輸入數字。

## ui.settings.launcher
ui-settings-launcher-installs = 遊戲安裝
ui-settings-launcher-installs_description = 啟動器為遊戲啟動與更新的安裝。
ui-settings-launcher-runner = Proton (Linux)
ui-settings-launcher-runner_description = 遊戲在 Linux 上的執行方式。會自動尋找 Steam 的 Proton Experimental 或最新的 Proton，否則使用 umu-run。
ui-settings-launcher-launch_args = 啟動參數
ui-settings-launcher-launch_args_description = 傳給遊戲的額外命令列參數，每行一個。
ui-settings-launcher-environment = 環境變數
ui-settings-launcher-environment_description = 遊戲的環境變數，KEY=VALUE，每行一個。
ui-settings-launcher-safe_mode_args = 安全模式參數
ui-settings-launcher-safe_mode_args_description = 以安全模式啟動遊戲時加入的參數，每行一個。
