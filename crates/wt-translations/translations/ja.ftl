# You are encouraged to suggest edits to this file on GitHub by either filing
# an issue, editing the file directly and creating a pull request, or just
# mentioning it in the Discord.
## meta
meta-language_name = 日本語

# -- 言語選択ダイアログ --

## dialog
dialog-continue_in_english = 英語で続行
dialog-continue_in_language = 日本語で続ける
dialog-select_language = 言語を選択
dialog-machine_translation_warning = ほとんどの翻訳は機械生成であり、不正確な場合があります。

# -- タブタイトル --

## ui.tabs
ui-tabs-unpacker = リソース展開
ui-tabs-settings = 設定
ui-tabs-replay_parser = リプレイインスペクター
ui-tabs-player_tracker = プレイヤー追跡
ui-tabs-armor_viewer = 装甲ビューアー
ui-tabs-model_viewer = モデルビューアー
ui-tabs-stats = 統計
ui-tabs-directory_stats = { $dir } の統計
ui-tabs-close_directory_stats = ディレクトリ統計を閉じる
ui-tabs-search = 検索
ui-tabs-search_these_replays_unavailable = これらのリプレイはまだインデックス化されていません
ui-tabs-zoom_in_panel = 拡大
ui-tabs-zoom_out_panel = 縮小
ui-tabs-close_panel = 閉じる
ui-tabs-close_tab = 閉じる
ui-tabs-close_other_tabs = 他を閉じる
ui-tabs-close_tabs_to_the_right = 右側を閉じる

# -- 共通ボタン --

## ui.buttons
ui-buttons-yes = はい
ui-buttons-no = いいえ
ui-buttons-send_replays = リプレイを送信
ui-buttons-send_build_data = ビルドデータのみ送信
ui-buttons-send_nothing = 何も送信しない
ui-buttons-switch_to_replays = リプレイ送信に切り替え
ui-buttons-keep_current = 現在の設定を維持
ui-buttons-stop = 停止
ui-buttons-copy = コピー
ui-buttons-cancel = キャンセル
ui-buttons-clear = クリア
ui-buttons-continue_ = 続行
ui-buttons-install_update = アップデートをインストール
ui-buttons-view_release = リリースを表示
ui-buttons-clear_settings = 設定をクリア
ui-buttons-more_options = その他のオプション
ui-buttons-create_issue = Issue を作成
ui-buttons-discord = Discord
ui-buttons-github = GitHub
ui-buttons-view_github = GitHub でプロジェクトを表示
ui-buttons-choose = 選択...
ui-buttons-reset = リセット
ui-buttons-select_all = 全て選択
ui-buttons-download = ダウンロード
ui-buttons-retry = 再試行
ui-buttons-refresh_persisted_data_confirm = 更新

# -- メニュー項目 --

## ui.menu
ui-menu-file = ファイル
ui-menu-check_updates = アップデートを確認
ui-menu-about = 情報
ui-menu-quit = 終了

# -- ウィンドウタイトル --

## ui.windows
ui-windows-build_consent = ビルド収集の同意
ui-windows-replay_migration = リプレイ共有に切り替え
ui-windows-crash_detected = アプリケーションのクラッシュを検出
ui-windows-update_available = アップデートあり
ui-windows-network_warning = ネットワーク警告
ui-windows-download_game_data = ゲームデータのダウンロード
ui-windows-error = エラー
ui-windows-software_encode = GPU ビデオエンコーダーなし
ui-windows-about = 情報
ui-windows-refresh_persisted_data = 保存済みリプレイデータを更新
ui-windows-tactics_board = 作戦ボード
ui-windows-minimize = 最小化
ui-windows-maximize = 最大化
ui-windows-restore = 元に戻す
ui-windows-close = 閉じる

# -- ダイアログ / メッセージテキスト --

## ui.dialogs
ui-dialogs-build_consent_message = ランク戦とランダム戦のプレイヤービルド情報を開発者に送信しますか？このデータ収集はコミュニティがプレイヤービルドを一括分析するのに役立ちます。設定からいつでもオプトアウトできます。
ui-dialogs-replay_migration_message = Toolkit は完全なリプレイを共有できるようになりました。ビルドデータのみの場合よりもはるかに詳細なマッチメイキング分析が可能になります。テスト艦でプレイした戦闘は共有されません。生のリプレイの共有に切り替えますか？
ui-dialogs-crash_message = WoWs Toolkit が前回の実行時にクラッシュしたようです。この問題を報告する場合は、GitHub で Issue を投稿するか、Discord サーバーに参加して以下の情報を提供してください。
ui-dialogs-crash_clear_settings = アプリケーションの永続化データが原因である可能性がある場合は、下のボタンを押して設定をクリアしてみてください。全ての設定がクリアされます。追跡中のプレイヤーは保持されます。リプレイと WoWs データは安全です。
ui-dialogs-update_message = WoWs Toolkit のバージョン { $tag } が利用可能です
ui-dialogs-update_notes = リリースノート
ui-dialogs-update_asset_pending = Windows 版のダウンロードはまだ利用できません。ビルド状況はリリースページで確認してください。
ui-dialogs-update_windows_only = アップデートが利用可能ですが、現時点では Windows のみサポートされています。
ui-dialogs-p2p_warning = コラボレーションセッションは P2P ネットワークを使用します。セッション内の他のユーザーがあなたの IP アドレスを見ることができる場合があります。
ui-dialogs-software_encode_message = 対応する GPU ビデオエンコーダーが見つからないため、このエクスポートはソフトウェアでエンコードされます。処理は大幅に遅くなり、長い戦闘では数分かかることがあります。
ui-dialogs-download_game_data_intro = 一部のリプレイにはインストールされていないゲームデータが必要です。ダウンロードするビルドを選択してください。
ui-dialogs-download_build_row = バージョン { $version }（ビルド { $build }）
ui-dialogs-download_replays_needing = { $count } 件のリプレイに必要
ui-dialogs-download_availability_exact = 公開済み
ui-dialogs-download_availability_nearest = 完全一致なし、最も近いのは { $version }（ビルド { $build }）
ui-dialogs-download_availability_unpublished = 未公開
ui-dialogs-download_availability_unreachable = 公開済みですが、ファイル一覧を読み取れませんでした
ui-dialogs-download_plan_pending = 取得が必要なものを確認中...
ui-dialogs-download_objects_to_fetch = チェックしたビルドで取得するオブジェクト数: { $count }
ui-dialogs-download_plan_failed = 取得が必要なものを判別できませんでした
ui-dialogs-download_plan_no_cache_dir = ゲームデータのキャッシュディレクトリが設定されていません。設定で選択してから再試行してください。
ui-dialogs-refresh_persisted_data_message = リプレイディレクトリ内の全リプレイを再解析し、保存データを書き直します。ゲームビルドと一致しないリプレイデータマッピングで記録されたエントリはそのまま残りますが、Personal Rating とゲームモードは全リプレイについて現在の期待値から再計算されます。時間がかかる場合があります。
ui-dialogs-file_dialog_failed = ファイルダイアログを開けませんでした: { $reason }

# -- ラベル --

## ui.labels
ui-labels-error_occurred = エラーが発生しました：
ui-labels-made_by = landaire 作。
ui-labels-credits = Trackpad、TTaro、lkolbly の貢献に感謝します。
ui-labels-pr_credits = パーソナルレーティング (PR) の計算データと計算式は WoWs Numbers より提供されています。
ui-labels-suppress_warning = 今後表示しない
ui-labels-more_info = 詳細情報
ui-labels-app_version = バージョン { $version }
ui-labels-project_license = WoWs Toolkit は { $license } ライセンスの下で配布されています。
ui-labels-third_party_notices = サードパーティの表示
ui-labels-notices_search_hint = 表示を検索
ui-labels-no_matching_notices = 検索に一致する表示はありません。

# -- トーストメッセージ --

## ui.messages
ui-messages-bug_report_info_copied = バグ報告情報をコピーしました。
ui-messages-update_downloading = アップデートをダウンロード中: { $read } / { $total }
ui-messages-update_downloading_unknown = アップデートをダウンロード中: { $read } 完了
ui-messages-app_up_to_date = アプリケーションは最新です
ui-messages-checking_app_updates = アプリのアップデートを確認中...
ui-messages-update_starting = アップデートをダウンロード中...
ui-messages-update_check_failed = アプリのアップデート確認に失敗しました
ui-messages-game_data_loaded = ゲームデータを正常に読み込みました
ui-messages-no_replays_detected = リプレイが検出されません。WoWs ディレクトリは正しく設定されていますか？
ui-messages-game_data_builds_downloaded = { $count } ビルドのゲームデータをダウンロードしました
ui-messages-game_data_download_failed = ゲームデータをダウンロードできませんでした
ui-messages-game_data_up_to_date = ゲームデータは最新です
ui-messages-game_data_updates_available = { $count } 件のキャッシュ済みビルドに新しいデータがあります
ui-messages-game_data_cache_valid = ゲームデータキャッシュは有効で、リモートリポジトリと一致しています
ui-messages-game_data_cache_invalid = { $count } 件のキャッシュ済みビルドが古いか破損しています
ui-messages-replay_load_failed = リプレイの読み込みに失敗しました
ui-messages-twitch_token_invalid = Twitch トークンが無効または期限切れです。設定で更新してください。
ui-messages-wows_dir_invalid = World of Warships ディレクトリが無効です。設定でパスを確認してください。
ui-messages-wows_dir_not_set = World of Warships のディレクトリが設定されていません
ui-messages-constants_version_mismatch =
    リプレイデータマッピングファイルのバージョンがゲームバージョンと一致しません。
    戦闘後の結果が正確でない可能性があります。プロジェクトメンテナーがサーバー上のマッピングを更新するまでお待ちください。
ui-messages-session_started = セッションが開始されました
ui-messages-user_joined = { $name } が参加しました
ui-messages-user_timeout = { $name } がタイムアウトしました
ui-messages-user_left = { $name } が退出しました
ui-messages-session_ended = セッションが終了しました
ui-messages-session_error = セッションエラー: { $msg }
ui-messages-connected_to_session = セッションに接続しました
ui-messages-host_opened_replay = ホストがリプレイを開きました: { $name }
ui-messages-host_closed_replay = ホストがリプレイを閉じました
ui-messages-session_rejected = セッションが拒否されました: { $reason }
ui-messages-drop_not_a_replay = リプレイファイルではありません
ui-messages-drop_not_replays = これらはリプレイファイルではありません
ui-messages-drop_skipped = リプレイでもフォルダーでもないドロップされたファイル { $count } 件をスキップしました
ui-messages-log_copied = { $file } から { $bytes } KB をクリップボードにコピーしました
ui-messages-log_copy_failed = ログファイルを読み取れませんでした: { $error }
ui-messages-log_not_found = { $dir } にログファイルが見つかりません

# -- 統計名 --

## stat
stat-damage = ダメージ
stat-spotting_damage = 観測ダメージ
stat-frags = 撃沈
stat-raw_xp = 生 XP
stat-base_xp = 基本 XP
stat-win_rate = 勝率
stat-personal_rating = パーソナルレーティング
stat-match_contribution = 戦闘貢献度
stat-capture_points = 占領ポイント
stat-planes_shot_down = 撃墜した航空機
stat-anti_air_damage = 対空ダメージ
stat-survival_rate = 生存率
stat-torpedoes_spotted = 発見した魚雷
stat-avg_pr = 平均 PR
stat-avg_prefix = 平均 { $name }

## chart
chart-combined = 合計
chart-combined_suffix = (合計)
chart-rolling_average_suffix = (移動平均)

# -- 確認ダイアログ --

## confirm
confirm-open_in_game = World of Warships を起動します。続行しますか？
confirm-clear_all_session_stats = 全てのセッション統計がクリアされます。続行しますか？
confirm-set_as_session_stats = 現在のセッション統計が置き換えられます。続行しますか？

# -- エラーメッセージ --

## error
error-file_watcher_creation = ファイルウォッチャーの作成に失敗しました: { $error }
error-replay_dir_watch = リプレイディレクトリの監視に失敗しました: { $error }

# -- 設定 --

## ui.settings.app
ui-settings-app-check_for_updates = 起動時にアップデートを確認
ui-settings-app-enable_logging = ログを有効にする（再起動後に有効）
ui-settings-app-enable_logging_tooltip = アプリケーションデータディレクトリ内のファイルにデバッグログを書き込みます。トラブルシューティングに便利です。
ui-settings-app-data_sharing_mode = データ共有（ランク戦とランダム戦）
ui-settings-app-theme = テーマ
ui-settings-app-theme_system = システム
ui-settings-app-theme_dark = ダーク
ui-settings-app-theme_light = ライト
ui-settings-app-code_integrity = 未署名コードをブロック（再起動後に有効）
ui-settings-app-code_integrity_tooltip = Microsoft が署名していないコードの読み込みを拒否し、オーバーレイ、キャプチャツール、オーディオユーティリティをこのアプリケーションから排除します。これらは描画中のクラッシュやフリーズの一般的な原因です。グラフィックドライバーは Windows を通じて署名されているため、引き続き動作します。セッション開始後は無効にできないため、変更は次回の起動時に適用されます。「自動」は、ブロック対象となるサードパーティ製の入力方式がインストールされている場合にこの機能をスキップします。なお、この設定に関わらず他の保護機能は有効のままです。全て無効にするには --no-hardening を付けて実行してください。
ui-settings-app-code_integrity_automatic = 自動
ui-settings-app-code_integrity_always = 常に
ui-settings-app-code_integrity_never = しない
ui-settings-app-language = 言語
ui-settings-app-open_data_dir_tooltip = アプリケーションデータフォルダ（設定、ログ、キャッシュデータ）をファイルマネージャーで開きます。
ui-settings-app-proxy_url = HTTP プロキシ
ui-settings-app-proxy_url_hint_live = host:port 形式。空欄の場合はシステムのプロキシ設定を使用します

## ui.settings.wows
ui-settings-wows-description = World of Warships のインストールパス
ui-settings-wows-directory_hint = World of Warships ディレクトリ
ui-settings-wows-browse = 参照...

## ui.settings.wows.cache
ui-settings-wows-cache-heading = ゲームデータキャッシュ
ui-settings-wows-cache-description = 古いビルドのリプレイも開けるよう抽出したゲームデータ
ui-settings-wows-cache-auto_dump = 古いリプレイの互換性のためにゲームデータをキャッシュ
ui-settings-wows-cache-browse = 参照
ui-settings-wows-cache-open_folder = フォルダを開く
ui-settings-wows-cache-delete_old = 古いバージョンを削除
ui-settings-wows-cache-stats = ゲームデータキャッシュ：{ $count }バージョンで{ $size }
ui-settings-wows-cache-check_updates = アップデートを確認
ui-settings-wows-cache-actions = キャッシュ操作
ui-settings-wows-cache-actions_tooltip = アップデートの確認またはキャッシュ済みビルドの検証
ui-settings-wows-cache-update_all = 全て更新
ui-settings-wows-cache-updates_available = { $count } 件のキャッシュ済みビルドに新しいデータがあります
ui-settings-wows-cache-validate = キャッシュを検証
ui-settings-wows-cache-repair = 修復
ui-settings-wows-cache-repair_needed = { $count } 件のキャッシュ済みビルドが古いか破損しています

## ui.settings.replay
ui-settings-replay-show_raw_xp = 生 XP を表示
ui-settings-replay-show_entity_id = エンティティ ID を表示
ui-settings-replay-show_observed_damage = 観測ダメージを表示
ui-settings-replay-enable_previews = ホバー時にミニマッププレビューを表示
ui-settings-replay-enable_previews_tooltip = リプレイにカーソルを合わせると、バックグラウンドで読み込み・再生してループするミニマッププレビューを作成します。低速なストレージではオフにしてください。
ui-settings-replay-auto_export_data = データ自動エクスポート
ui-settings-replay-export_path_hint = データ自動エクスポートディレクトリ
ui-settings-replay-export_path_missing = このディレクトリは存在しません。データをエクスポートできません。
ui-settings-replay-index_all_replays = 全リプレイをインデックス化
ui-settings-replay-show_heals = 回復を表示
ui-settings-replay-current_path = 現在のリプレイパス

## ui.settings.session
ui-settings-session-display_name = 表示名
ui-settings-session-suppress_ip_warning = P2P IP アドレスの警告を表示しない
ui-settings-session-ip_warning_tooltip = コラボレーションセッションは P2P ネットワークを使用します。セッション内の他のユーザーがあなたの IP アドレスを見ることができる場合があります。
ui-settings-session-disable_auto_open = セッションウィンドウの自動オープンを無効にする
ui-settings-session-auto_open_tooltip = 有効にすると、セッションウィンドウ（リプレイ、戦術ボード）が自動的に開かなくなります。セッションポップオーバーから手動で開くことができます。

## ui.settings.twitch
ui-settings-twitch-get_token = ログイントークンを取得
ui-settings-twitch-get_token_tooltip = Chatterino のログインページを使用しています。必要な権限を持つトークン（基本的にチャット権限を持つモデレータートークン）を提供し、WoWs Toolkit 開発者が同じ結果をもたらす独自のログインページをホストする必要をなくします。
ui-settings-twitch-paste_token_no_token = トークンを貼り付け（トークン未設定）
ui-settings-twitch-paste_token_tooltip = クリップボードから認証情報を読み取ります
ui-settings-twitch-paste_token_valid = トークンを貼り付け（トークン有効）
ui-settings-twitch-paste_token_unvalidated = トークンを確認中
ui-settings-twitch-paste_token_invalid = トークンを貼り付け（トークン無効 / 期限切れ）
ui-settings-twitch-monitored_channel = 監視チャンネル（デフォルト: 自分）
ui-settings-twitch-signed_in_as = { $who } としてサインイン中

# -- リプレイインスペクター --

## ui.replay
ui-replay-copy_bug_report_info = バグ報告情報をコピー
ui-replay-match_age = { $age } 前
ui-replay-listing_caption = リプレイ
ui-replay-loading_game_data_version = { $version } のゲームデータを読み込み中...
ui-replay-spectator = 観戦者
ui-replay-loading = 読み込み中...
ui-replay-preview_loading = プレビューを作成中...
ui-replay-preview_no_game_data = クライアントバージョン { $value } のゲームデータがありません
ui-replay-preview_unreadable_because = このリプレイは読み取れませんでした: { $value }
ui-replay-preview_unwalkable = このリプレイの戦闘を走査できませんでした: { $value }
ui-replay-preview_unknown_version = このリプレイは読み取れないクライアントバージョンを報告しています: { $value }
ui-replay-preview_no_map = { $value } のマップデータがありません
ui-replay-preview_render_failed = プレビューを描画できませんでした: { $value }
ui-replay-incomplete_results = 不完全な試合結果
ui-replay-incomplete_results_tooltip = リプレイにはまだ試合終了結果がありません。試合が終了して結果がリプレイに追加されると、データは自動的に再読み込みされます。
ui-replay-no_chat = このリプレイではチャットメッセージが送信されていません
ui-replay-chat_copy_all = 全てコピー
ui-replay-chat_save_to_file = ファイルに保存
ui-replay-chat_save_title = ゲームチャットを保存
ui-replay-export_results_title = 結果をエクスポート
ui-replay-results_exported = 結果をエクスポートしました
ui-replay-chat_saved = チャットを { $path } に保存しました
ui-replay-chat_save_failed = チャットを保存できませんでした: { $error }
ui-replay-team_damage = チームダメージ:{" "}
ui-replay-team_share = チーム合計の{ $pct }（{ $team }）
ui-replay-hide_my_stats = テスト艦の統計を非表示
ui-replay-open_manually = リプレイファイルを手動で開く...
ui-replay-open_directory = リプレイディレクトリを開く...
ui-replay-search_these_replays = これらのリプレイを検索
ui-replay-import_constants = constants.json ファイルをインポート...
ui-replay-refresh_persisted_data = 保存済みリプレイデータを更新...
ui-replay-contributing = { $total } 件中 { $done } 件のリプレイを提供中
ui-replay-contributed = { $total } 件中 { $sent } 件のリプレイを提供しました
ui-replay-contribute_nothing_shared = 先に設定で共有する内容を選択してください
ui-replay-contribute_none_listed = 提供対象のリプレイがありません
ui-replay-contribute_not_ready = ゲームデータと設定の読み込みが完了するとリプレイを提供できます
ui-replay-contribute_already_running = 提供処理は既に実行中です
ui-replay-auto_export_failed = 戦闘を書き出せませんでした: { $error }
ui-replay-auto_exported = 戦闘を { $file } に書き出しました
ui-replay-opened_new_battle = 新しい戦闘を開きました: { $battle }
ui-replay-new_battle_listed = 新しい戦闘: { $battle }
ui-replay-results_arrived = { $battle } の戦闘結果が届きました
ui-replay-constants_written = { $count } ビルドの結果マッピングをダウンロードしました
ui-replay-constants_import_title = constants.json をインポート
ui-replay-constants_imported = ビルド { $build } の結果マッピングをインポートしました
ui-replay-constants_import_failed = マッピングをインポートできませんでした: { $reason }
ui-replay-constants_no_build = 先にゲームビルドを読み込んでください
ui-replay-constants_latest_written = 最新の結果マッピングをダウンロードしました
ui-replay-constants_check_failed = 結果マッピングを確認できませんでした: { $reason }
ui-replay-constants_build_label = ビルド { $build }
ui-replay-constants_generating = { $version } のゲーム定数を更新中...
ui-replay-constants_generated = { $version } のゲーム定数を { $seconds } 秒で生成しました
ui-replay-constants_generation_failed = { $version } のゲーム定数を更新できませんでした: { $reason }。{ $fallback }
ui-replay-constants_fallback_kept = 保存済みの定数を引き続き使用します。
ui-replay-constants_fallback_published = ビルド { $build } 用に公開された定数を使用します。
ui-replay-constants_fallback_embedded = 内蔵の定数を使用します。
ui-replay-copy_latest_log = 最新ログをクリップボードにコピー
ui-replay-load_alt_perspective = 相手チームの視点を読み込む...
ui-replay-load_alt_perspective_failed = リプレイの統合に失敗しました: { $error }
ui-replay-load_alt_perspective_limit = この戦闘は既に他の { $limit } 件の記録から読み込まれています
ui-replay-consumable_charges_remaining = { $total } 回中 { $remaining } 回残り
ui-replay-consumable_charges_unlimited = { $used } 回使用（使用回数無制限）
ui-replay-consumable_header_remaining = 残り
ui-replay-consumable_header_total = 合計
ui-replay-consumable_charges_infinite = ∞
ui-replay-autoload_latest = 最新リプレイを自動読み込み
ui-replay-autoload_latest_tooltip = ゲームが書き込みを終えたらすぐに試合を開く
ui-replay-anonymize_names = プレイヤー名を匿名化
ui-replay-anonymize_names_tooltip = プレイヤー名とクランタグの代わりに生成した名前を表示します。エクスポートには実際の名前が残ります。
ui-replay-group_win_rate_all_modes = グループの勝率に全てのゲームモードを含める
ui-replay-group_win_rate_all_modes_tooltip = 既定では、グループの見出しはランダム戦とランク戦のみを集計します。有効にすると全てのゲームモードを集計し、勝率は色分けされません。色はランダム戦を基準に調整されているためです。
ui-replay-select_replay = リプレイを選択
ui-replay-drop_hint = リプレイをここにドロップすると開き、フォルダーをドロップするとそのリプレイを一覧表示します
ui-replay-scanning = リプレイをスキャン中...
ui-replay-scanning_count = リプレイを読み込み中: { $done } / { $total }
ui-replay-no_replays_in = { $folder } にリプレイはありません
ui-replay-drop_folder_hint = フォルダーをウィンドウにドロップすると専用のタブに一覧表示されます
ui-replay-new_tag = 新着
ui-replay-loading_game_data = ゲームデータを読み込み中...
ui-replay-game_data_failed = ゲームデータの読み込みに失敗しました: { $error }
ui-replay-group_label = リプレイのグループ化
ui-replay-column_filters = 列フィルター
ui-replay-row_elements = 行の表示項目
ui-replay-row_element_personal_rating = パーソナルレーティング
ui-replay-row_element_match_contribution = 試合貢献度
ui-replay-row_element_division = 分艦隊
ui-replay-row_element_twitch = Twitchチャット
ui-replay-row_element_damage = ダメージ
ui-replay-row_element_kills = 撃沈
ui-replay-row_element_time = 戦闘日時
ui-replay-listing_options = 一覧オプション
ui-replay-open = 開く
ui-replay-open_file = リプレイファイルを開く...
ui-replay-check_game_data = ゲームデータを確認
ui-replay-check_game_data_tooltip = このディレクトリのリプレイに必要なゲームデータを確認
ui-replay-close_side_panel = パネルを閉じる
ui-replay-nda = NDA
ui-replay-chat = チャット
ui-replay-game_chat = ゲームチャット
ui-replay-save_to_file = ファイルに保存
ui-replay-chat_copied = チャットをコピーしました
ui-replay-copy_message = メッセージをコピー
ui-replay-actions = アクション
ui-replay-row_actions_hint = プレイヤー操作 (または行を右クリック)
ui-replay-section_match = 試合
ui-replay-section_export = エクスポート
ui-replay-section_debug = デバッグ
ui-replay-timeline = 試合タイムライン
ui-replay-row_not_indexed = 未インデックス
ui-replay-hover_survived = 生存
ui-replay-hover_sunk = 撃沈
ui-replay-hover_not_indexed = 未インデックス
ui-replay-row_kills = { $count } 撃沈
ui-replay-row_survived = 生存
ui-replay-row_sunk = 撃沈
ui-replay-row_division = 分艦隊: { $members }
ui-replay-export_results_json = 結果: JSON
ui-replay-export_results_cbor = 結果: CBOR
ui-replay-export_results_csv = 結果: CSV
ui-replay-timeline_filter_all = 全て
ui-replay-timeline_filter_none = なし
ui-replay-timeline_search_hint = 艦船またはプレイヤーを検索
ui-replay-timeline_no_events = 重要なイベントは検出されませんでした。
ui-replay-timeline_no_matches = 現在のフィルターに一致するイベントがありません。
ui-replay-timeline_parsing = イベントを解析中...
ui-replay-timeline_failed = 試合タイムラインの抽出に失敗しました: { $error }
ui-replay-timeline_retry = 再試行
ui-replay-timeline_export_tooltip = フィルター適用後に表示されているイベントが対象です。
ui-replay-timeline_copied = タイムラインをコピーしました
ui-replay-timeline_event_copied = イベントをコピーしました
ui-replay-timeline_saved = タイムラインを { $path } に保存しました
ui-replay-timeline_save_failed = タイムラインを保存できませんでした: { $error }
ui-replay-timeline_kind_health_lost = 大ダメージ
ui-replay-timeline_kind_death = 撃沈
ui-replay-timeline_kind_cap_contested = 占領ポイント争奪
ui-replay-timeline_kind_cap_flipped = 占領ポイント確保
ui-replay-timeline_kind_cap_being_captured = 占領ポイント占領中
ui-replay-timeline_kind_radar = レーダー使用
ui-replay-timeline_kind_advantage = アドバンテージ変化
ui-replay-timeline_kind_disconnected = 切断

## ui.replay.context
ui-replay-context-open_in_new_tab = 新しいタブで開く
ui-replay-context-copy_cell = セルをコピー
ui-replay-context-copy_row = 行をコピー
ui-replay-context-copy_table = 表をコピー
ui-replay-context-pin_column = 列を固定
ui-replay-context-unpin_column = 列の固定を解除
ui-replay-context-copied = コピーしました
ui-replay-context-copy_path = パスをコピー
ui-replay-context-copy_replays = { $count } 件のリプレイをコピー
ui-replay-context-copy_paths = { $count } 件のパスをコピー
ui-replay-context-show_in_explorer = エクスプローラーで表示
ui-replay-context-show_replay_controls = リプレイコントロールを表示
ui-replay-context-open_in_game = ゲームで開く
ui-replay-context-render_replay = リプレイをレンダリング
ui-replay-context-set_session_stats = セッション統計として設定 ({ $count } リプレイ)
ui-replay-context-add_session_stats = セッション統計に追加 ({ $count } リプレイ)
ui-replay-context-render_to_video = { $count ->
    [1] 動画にエクスポート
   *[other] { $count } 件のリプレイを動画にエクスポート
}
ui-replay-context-render_to_clipboard = { $count ->
    [1] クリップボードにエクスポート
   *[other] { $count } 件のリプレイをクリップボードにエクスポート
}

## ui.replay.column
ui-replay-column-player_name = プレイヤー名
ui-replay-column-base_xp = 基本 XP
ui-replay-column-ship_name = 艦船名
ui-replay-column-hits = 命中
ui-replay-column-heals = 回復
ui-replay-column-heals_tooltip = このプレイヤーで観測された修理班の発動回数。画面に描画されていない艦船（一度も発見されなかった敵艦など）では不正確な場合があります。
ui-replay-column-heals_no_repair_tooltip = この艦船は修理班を搭載していません。
ui-replay-column-kills = 撃沈
ui-replay-column-observed_damage = 観測ダメージ
ui-replay-column-actual_damage = 実ダメージ
ui-replay-column-potential_damage = 潜在ダメージ
ui-replay-column-time_lived = 生存時間
ui-replay-column-fires = 火災
ui-replay-column-floods = 浸水
ui-replay-column-citadels = バイタルヒット
ui-replay-column-crits = クリティカル
ui-replay-column-received_damage = 被ダメージ
ui-replay-column-distance_traveled = 移動距離
ui-replay-column-skills = スキル
ui-replay-column-personal_rating = PR
ui-replay-column-match_contribution = MC

## ui.rating
ui-rating-personal_rating = パーソナルレーティング
ui-rating-match_contribution = 戦闘貢献度
ui-rating-versus_average = 平均比
ui-rating-damage = ダメージ
ui-rating-kills = 撃沈
ui-rating-wins = 勝利
ui-rating-team_damage_share = チーム与ダメージ内の割合
ui-rating-tanking = 被弾誘引
ui-rating-spotting = 発見
ui-rating-objectives = 目標
ui-rating-anti_air = 対空
ui-rating-torpedo_warnings = 魚雷警告
ui-rating-critical_bursts = 集中ダメージ
ui-rating-unrated_early_exit = MCなし: 戦闘終了前に記録が終了しました
ui-rating-mc_unrated_battle_type = MCなし: 戦闘貢献度はランダム戦とランク戦にのみ適用されます

## ui.rating.detail
ui-rating-detail-versus_expected = { $actual } (期待値 { $expected })
ui-rating-detail-wins = { $actual } 勝 (期待値 { $expected })
ui-rating-detail-weighted_damage = { $actual } (期待値 { $expected })、駆逐艦と潜水艦へのダメージは x1.2
ui-rating-detail-team_damage_share = 敵へのチーム与ダメージのうち { $share }%、均等な割合は { $even }%
ui-rating-detail-tanking = 潜在ダメージ { $potential }、艦種平均の { $class }x、チーム均等割合の { $team }x
ui-rating-detail-spotting = 発見ダメージ { $damage } (艦種平均 { $baseline })、発見した艦船のHP { $ships } (平均 { $ships_baseline })
ui-rating-detail-objectives = 獲得ポイント: 占領 { $capture }、占領阻止 { $block }、占領リセット { $reset }、終了時保持 { $battle_end }、保持 { $hold } (4分の1で計算)
ui-rating-detail-anti_air_planes = 攻撃機 { $strike } 機、戦闘機・偵察機 { $support } 機
ui-rating-detail-anti_air_damage = 対空ダメージ { $damage }
ui-rating-detail-torpedo_warnings = 脅威となる魚雷を { $threatening } 本発見
ui-rating-detail-critical_bursts = 集中ダメージ { $count } 回: { $bursts }。記録プレイヤー付近の艦船のみ確認可能
ui-rating-detail-critical_burst = { $seconds } 秒で { $ship } の { $share }%
ui-rating-detail-critical_burst_at_once = 一度に { $ship } の { $share }%

## ui.personal_rating
ui-personal_rating-unrated_hover = パーソナルレーティングはランダム戦とランク戦にのみ適用されます。

## ui.rating.basis
ui-rating-basis-static = 静的
ui-rating-basis-dynamic = 動的
ui-rating-basis-static_explained = 静的: 固定定数または艦種の基準値と比較
ui-rating-basis-dynamic_explained = 動的: wows-numbers の期待値またはこの戦闘の他プレイヤーと比較

## ui.replay.group
ui-replay-group-date = 日付
ui-replay-group-ship = 艦船
ui-replay-group-none = なし

## ui.replay.player
ui-replay-player-hidden_profile = プレイヤーのプロフィールは非公開です
ui-replay-player-never_connected = プレイヤーは試合に接続しませんでした

## ui.replay.build
ui-replay-build-open_shiptool = ShipToolでビルドを開く
ui-replay-build-open_in_ship_builder = WoWs SBでビルドを開く
ui-replay-build-copy_link = ビルドリンクをコピー
ui-replay-build-copy_short_link = 短縮ビルドリンクをコピー
ui-replay-build-link_copied = ビルドリンクをコピーしました
ui-replay-build-open_wows_numbers = WoWs Numbers ページを開く
ui-replay-build-not_spotted = この艦船は一度も発見されませんでした。ビルド情報は利用できません。

## ui.replay.sections
ui-replay-sections-achievements = アチーブメント
ui-replay-sections-ribbons = リボン
ui-replay-sections-modules = モジュール:
ui-replay-sections-modules_none = モジュールなし
ui-replay-sections-signals = 信号旗:
ui-replay-sections-loadout = 装備構成:
ui-replay-sections-loadout_none = 装備構成なし
ui-replay-sections-captain_skills = 艦長スキル:
ui-replay-sections-captain_skills_none = 艦長スキルなし
ui-replay-sections-consumables = 消耗品:
ui-replay-sections-fire_chance = 実効発火率
ui-replay-sections-fire_chance_expected = 期待値
ui-replay-sections-fire_chance_fires = 火災 { $fires } 件
ui-replay-sections-fire_chance_eligible_hits = 発火の可能性があった命中 { $hits } 発
ui-replay-sections-fire_chance_ships = 対象艦 { $ships } 隻
ui-replay-sections-fire_chance_expected_fires = 期待値 火災 { $fires } 件
ui-replay-sections-fire_chance_per_ship = 対象艦ごと
ui-replay-sections-fire_chance_no_eligible_hits = 発火の可能性があった命中なし
ui-replay-sections-fire_chance_formula = 攻撃側の発火率計算式
ui-replay-sections-fire_chance_formula_base = 基本 burnProb
ui-replay-sections-fire_chance_formula_clamped = クランプ済み
ui-replay-sections-fire_chance_breakdown = 内訳
ui-replay-sections-fire_chance_shells_fired = 発射した HE 弾
ui-replay-sections-fire_chance_he_hits = 艦船への HE 命中
ui-replay-sections-fire_chance_no_target_ship = 対象艦を特定できない HE 命中
ui-replay-sections-fire_chance_eligible = 発火の可能性あり
ui-replay-sections-fire_chance_ribbons = SetFire リボン
ui-replay-sections-fire_chance_ribbons_credited = 砲弾に紐付け済み
ui-replay-sections-fire_chance_ribbons_uncredited = 紐付けなし
ui-replay-sections-fire_chance_unattributed_no_burn_state = 照合できる延焼状態の変化なし
ui-replay-sections-fire_chance_unattributed_already_credited = 近くの命中は先行の火災に使用済み
ui-replay-sections-fire_chance_unattributed_secondary_contest = 自艦の副砲でも同様に発火させ得た
ui-replay-sections-fire_chance_unattributed_hit_contest = 近くの自艦の命中はいずれも他と判別できなかった
ui-replay-sections-fire_chance_unattributed_all_excluded = 近くの自艦の命中はいずれも除外済み
ui-replay-sections-fire_chance_unattributed_none_could_burn = 近くの自艦の命中に発火の可能性なし
ui-replay-sections-fire_chance_unattributed_no_hit = 対象期間内に自艦の命中なし
ui-replay-sections-fire_chance_not_applicable = 対象艦は撃沈済みのため対象外
ui-replay-sections-fire_chance_copied = 発火率の内訳をコピーしました
ui-replay-sections-fire_chance_click_to_copy = クリックでこの内訳をコピー
ui-replay-sections-fire_chance_exclusion_already_burning = その区画は既に延焼中
ui-replay-sections-fire_chance_exclusion_victim_build_unknown = 対象艦のビルド不明、発火区画が統合されている可能性あり
ui-replay-sections-fire_chance_exclusion_damage_control_active = 応急工作班が発動中
ui-replay-sections-fire_chance_exclusion_damage_control_unknown = 応急工作班の状態不明
ui-replay-sections-fire_chance_exclusion_observation_gap = 観測の欠落
ui-replay-sections-fire_chance_exclusion_consumable_unreliable = 消耗品の発動モデルが不正確
ui-replay-sections-fire_chance_exclusion_victim_fate_unknown = 対象艦の結末不明
ui-replay-sections-fire_chance_exclusion_hit_type_does_not_roll = この命中種別は発火判定を行わない
ui-replay-sections-fire_chance_exclusion_no_geometry = 発火区画のジオメトリなし
ui-replay-sections-fire_chance_exclusion_impact_unplaceable = 照合した艦上に着弾位置を特定できなかった
ui-replay-sections-fire_chance_exclusion_victim_pose_unknown = 着弾時の対象艦の位置が不明
ui-replay-sections-fire_chance_exclusion_ambiguous_hit = 自艦の他の命中と判別できない
ui-replay-sections-damage_events = ダメージイベント

## ui.replay.debug
ui-replay-debug-view_raw_metadata = 生のプレイヤーメタデータを表示
ui-replay-debug-raw_metadata = 生のメタデータ
ui-replay-debug-raw_results = 生の結果
ui-replay-debug-mapped_results = マッピング済みの結果
ui-replay-debug-player_metadata = プレイヤーメタデータ
ui-replay-debug-search = 検索
ui-replay-debug-previous_match = 前の一致 (Shift+Enter)
ui-replay-debug-next_match = 次の一致 (Enter)

## ui.replay.controls
ui-replay-controls-window_title = リプレイコントロール
ui-replay-controls-unavailable = コントロールが利用できません（commands.scheme.xml がゲームファイルに見つかりません）。

# -- コラボレーション / セッション --

## ui.collab
ui-collab-session = セッション
ui-collab-starting = セッションを開始中...
ui-collab-session_active = セッションがアクティブ
ui-collab-connected_to_session = セッションに接続済み
ui-collab-session_token = セッショントークン:
ui-collab-toggle_visibility = トークンの表示/非表示を切り替え
ui-collab-copy_token = トークンをコピー
ui-collab-token_copied = トークンをクリップボードにコピーしました
ui-collab-copy_web_link = Webリンクをコピー
ui-collab-web_link_copied = Webリンクをクリップボードにコピーしました
ui-collab-copy_localhost_link = localhostリンクをコピー
ui-collab-localhost_copied = localhostリンクをクリップボードにコピーしました
ui-collab-connected_count = { $count } 人接続中
ui-collab-promote_cohost = コホストに昇格
ui-collab-permissions = 権限
ui-collab-lock_annotations = 注釈をロック
ui-collab-lock_settings = 設定をロック
ui-collab-reset_overrides = クライアントのオーバーライドをリセット
ui-collab-reset_overrides_tooltip = クライアントの表示設定の変更を全てリセット
ui-collab-leave = 退出
ui-collab-role_host = ホスト
ui-collab-role_cohost = コホスト
ui-collab-you = (あなた)
ui-collab-display_name = 表示名:
ui-collab-display_name_hint = あなたの名前...
ui-collab-enter_display_name = 先に表示名を入力してください
ui-collab-host_session = セッションをホスト
ui-collab-start_session = セッションを開始
ui-collab-join_session = セッションに参加
ui-collab-paste_and_join = トークンを貼り付けて参加
ui-collab-clipboard_empty = クリップボードが空です
ui-collab-invalid_token = 無効なトークン: { $error }
ui-collab-no_runtime = セッションを開始できませんでした: ランタイムがありません
ui-collab-shared_windows = 共有ウィンドウ
ui-collab-watching = 観戦中
ui-collab-waiting_for_frames = 戦闘データの到着を待機中
ui-collab-open = 開く
ui-collab-open_for_everyone = 全員に開く
ui-collab-tactics_board = 戦術ボード

# -- 戦闘結果テキスト --

## battle
battle-victory = 勝利
battle-defeat = 敗北
battle-draw = 引き分け
battle-battle_starts_in = 戦闘開始まで

# -- 終了タイプの説明 --

## finish_type
finish_type-unknown = 戦闘終了
finish_type-extermination = 敵艦全滅
finish_type-base_captured = 拠点占領
finish_type-timeout = 時間切れ
finish_type-failure = 作戦失敗
finish_type-technical = テクニカルフィニッシュ
finish_type-score = スコア上限到達
finish_type-score_on_timeout = 時間切れ時のスコア差
finish_type-pve_success = 作戦成功
finish_type-pve_failed = 作戦失敗
finish_type-score_zero = スコア消滅
finish_type-score_excess = スコア上限超過

# -- 統計タブ --

## ui.stats
ui-stats-overview = 概要
ui-stats-sample_notice = サンプルデータです (記録された試合ではありません)
ui-stats-div_all = 全て
ui-stats-div_solo = ソロ
ui-stats-div_div = 分艦隊
ui-stats-filters = フィルター
ui-stats-own_filters = このチャートを個別にフィルター
ui-stats-own_filters_tooltip = 他のペインの表示を変えずにこのチャートを絞り込みます
ui-stats-mode_label = モード:
ui-stats-clear = クリア
ui-stats-pr = PR:
ui-stats-mc = MC:
ui-stats-best_frags = 最高: { $ship } ({ $count } 撃沈)
ui-stats-max_damage = 最大ダメージ: { $ship } ({ $damage })
ui-stats-copy_markdown = Markdown としてコピー
ui-stats-copy_image = 画像としてコピー
ui-stats-copy_image_done = チャートをクリップボードにコピーしました
ui-stats-copy_image_failed = チャートをコピーできませんでした
ui-stats-copy_csv = CSV としてコピー
ui-stats-remove_games_hint = { $ship } の全ゲームを削除 (Ctrl+Click で確認をスキップ)
ui-stats-settings = 設定
ui-stats-options = オプション
ui-stats-ships = 艦船
ui-stats-filter_ships = チャートの艦船をフィルター
ui-stats-all_ships = 全て
ui-stats-no_ships = なし
ui-stats-search_ships = 艦船を検索...
ui-stats-no_ship_matches = この検索に一致する艦船はありません
ui-stats-search_statistics = 統計を検索
ui-stats-no_statistic_matches = この検索に一致する統計はありません
ui-stats-ship_selection_count = { $total } 隻中 { $selected } 隻を選択
ui-stats-all_ships_tooltip = 一致する艦船を全て選択
ui-stats-no_ships_tooltip = 一致する艦船の選択を解除
ui-stats-limit_to_recent = 最近の試合に限定
ui-stats-limit_to_recent_tooltip = 制限は「艦船」と「チャート」では艦船ごと、「概要」では全試合に適用されます。
ui-stats-division_label = 分艦隊:
ui-stats-add_chart = チャートを追加
ui-stats-clear_tooltip = 記録された全ての試合を削除
ui-stats-clear_failed = クリアできませんでした: { $reason }
ui-stats-clear_ship_confirm = もう一度押すとこの艦船の試合を削除します
ui-stats-copy_table = この表をコピー
ui-stats-copied = クリップボードにコピーしました
ui-stats-no_games = 現在のフィルターに該当する試合の記録はありません
ui-stats-nothing_to_plot = 現在のフィルターでは描画するデータがありません
ui-stats-column_ship = 艦船
ui-stats-column_battles = 戦闘数
ui-stats-column_win_rate = 勝率
ui-stats-column_avg_damage = 平均ダメージ
ui-stats-column_avg_frags = 平均撃沈数
ui-stats-frags = { $count } 撃沈
ui-stats-statistic = 統計
ui-stats-open_directory_stats = ディレクトリ統計
ui-stats-open_directory_stats_tooltip = このディレクトリとサブディレクトリ内の全リプレイの統計を開く
ui-stats-directory_stats_unavailable = ゲームデータを読み込み中です
ui-stats-directory_reading_task = ディレクトリ統計用にリプレイを読み込み中...
ui-stats-directory_walking = リプレイを検索中...
ui-stats-directory_scanning = { $total } 件中 { $done } 件のリプレイを確認中...
ui-stats-directory_reading = { $total } 件中 { $done } 件のリプレイを読み込み中...
ui-stats-directory_reload = 再読み込み
ui-stats-directory_reload_tooltip = このディレクトリのリプレイを再度読み込む
ui-stats-directory_skipped = { $count } 件のリプレイをスキップ
ui-stats-directory_skipped_detail =
    読み込み不可: { $unreadable }
    ビルドのゲームデータなし: { $missing_build }
    戦闘結果なし: { $no_results }
ui-stats-chart = チャート
ui-stats-combine_ships = 艦船を合算
ui-stats-running_average = 移動平均
ui-stats-value_labels = 値ラベル
ui-stats-reset_view = 表示をリセット
ui-stats-keep_current_layout = 現在のレイアウトを維持
ui-stats-session_title = セッション統計
ui-stats-session_manual = 手動
ui-stats-session_since_opened = アプリ起動以降
ui-stats-session_manual_tooltip = セッションには、プレイした戦闘と、リプレイ一覧から設定または追加したリプレイが含まれます。起動をまたいで保持されます。
ui-stats-session_since_opened_tooltip = セッションには、アプリを起動した { $time } 以降に開始したすべての戦闘が含まれます。手動セッションは保持され、手動に切り替えると戻ります。
ui-stats-session_battles = { $count } 戦
ui-stats-session_chip_manual = セッション: { $battles }
ui-stats-session_chip_since = { $time } 以降のセッション: { $battles }
ui-stats-session_empty_title = このセッションにはまだ戦闘がありません
ui-stats-session_empty_manual = プレイした戦闘は終了時にセッションに追加されます。リプレイ一覧のメニューからリプレイを設定または追加することもできます。
ui-stats-session_empty_since = { $time } 以降に開始した戦闘は終了時にセッションに追加されます。
ui-stats-session_set_add_unavailable = セッションには { $time } 以降の戦闘が含まれています。リプレイを設定または追加するには手動に切り替えてください。
ui-stats-clear_unavailable_since = アプリ起動以降のセッションは保存されません。保存済みのセッションをクリアするには手動に切り替えてください。
ui-stats-charts_save_failed = グラフの設定を保存できませんでした: { $reason }
ui-stats-layout_save_failed = グラフのレイアウトを保存できませんでした: { $reason }
ui-stats-axis_game = 試合
ui-stats-chart_hover_game = 第{ $number }試合、{ $time }
ui-stats-series_average = { $stat } (平均)
ui-stats-series_running = { $stat } (移動平均)
ui-stats-series_more = 他 { $count } 件
ui-stats-mode_random = ランダム戦
ui-stats-mode_ranked = ランク戦
ui-stats-mode_other = その他
ui-stats-win_rate_rated_only = 勝率はランダム戦とランク戦のみを集計します
ui-stats-win_rate_breakdown_line = { $mode }: { $rate }（{ $battles } 戦）
ui-stats-matches = 試合
ui-stats-column_time = 時間
ui-stats-column_mode = モード
ui-stats-column_result = 結果
ui-stats-keep_game = この試合を統計に含める
ui-stats-leave_out_game = この試合を統計から除外
ui-stats-leave_out_ship = { $ship } の試合をすべて除外
ui-stats-leave_out_ship_hint = { $ship } を統計から除外
ui-stats-keep_all_games = すべての試合を含める
ui-stats-keep_all_games_tooltip = 手動で除外した試合をすべて再び含める
ui-stats-left_out_only = 除外のみ
ui-stats-match_count = { $total } 試合中 { $shown } 試合、{ $left_out } 試合除外
ui-stats-no_matches = 現在のフィルターに一致する試合はありません
ui-stats-search_matches = 艦艇で検索...
ui-stats-ship_filter_tooltip = 統計に含める艦艇を選択
ui-stats-keep_ships_tooltip = 一致する艦艇をすべて含める
ui-stats-leave_out_ships_tooltip = 一致する艦艇をすべて統計から除外
ui-stats-dates = 日付
ui-stats-all_dates = すべての日付
ui-stats-clear_dates = すべての日付を表示
ui-stats-games_left_out = { $count } 試合を除外
ui-stats-reset_filters = フィルターをリセット
ui-stats-reset_filters_tooltip = すべての試合を表示: 件数制限、期間、分隊、モード、除外した艦艇と試合をすべて解除

## ui.stats.table
ui-stats-table-min = 最小
ui-stats-table-max = 最大
ui-stats-table-total = 合計
ui-stats-table-average = 平均

# -- プレイヤー追跡 --

## ui.player_tracker
ui-player_tracker-clear_stats = 統計をクリア
ui-player_tracker-populate_from_replays = リプレイからデータを読み込む
ui-player_tracker-find_matches = 試合を検索
ui-player_tracker-open_wows_numbers = wows-numbers で開く
ui-player_tracker-open_shipbuilds = shipbuilds でビルドを開く
ui-player_tracker-subtab_players = プレイヤー
ui-player_tracker-filter_hint = プレイヤーまたはクランでフィルター...
ui-player_tracker-time_period = 期間
ui-player_tracker-allies = 味方
ui-player_tracker-enemies = 敵
ui-player_tracker-live_no_directory = 戦闘を監視するには World of Warships のディレクトリを設定してください
ui-player_tracker-live_checking = 進行中の戦闘を確認中...
ui-player_tracker-live_none = 進行中の戦闘はありません
ui-player_tracker-live_loading_build = このビルドの艦船データを読み込み中です。完了すると名前と艦種が表示されます。
ui-player_tracker-waiting_for_index = リプレイインデックスを待機中
ui-player_tracker-loading_clans = クランを読み込み中...
ui-player_tracker-loading_players = プレイヤーを読み込み中...
ui-player_tracker-index_failed = インデックスを読み取れませんでした: { $reason }
ui-player_tracker-no_players = この期間にインデックス化されたプレイヤーはいません
ui-player_tracker-note_not_saved = 保存されていません: { $reason }
ui-player_tracker-subtab_current_match = 現在の試合
ui-player_tracker-encounters_hover = 合計 { $total } 回遭遇、選択期間内では { $range } 回。最終遭遇: { $last }。
ui-player_tracker-never_encountered = 遭遇履歴なし
ui-player_tracker-copy_wg_id = クリックで WG ID をコピー
ui-player_tracker-copied_wg_id = WG ID { $id } をコピーしました
ui-player_tracker-aliases_hover = 別名: { $names }
ui-player_tracker-notes_hint = このプレイヤーに関するメモ
ui-player_tracker-arena_count = { $count } 件の試合を記録
ui-player_tracker-last_encountered_exact = 最終遭遇 { $timestamp }
ui-player_tracker-subtab_clans = クラン
ui-player_tracker-clan_no_data = クランとの遭遇はまだ記録されていません。
ui-player_tracker-find_clan_matches = このタグを含む試合を検索します。インデックスにはクラン専用のフィールドがないため、タグを含むプレイヤー名も一致します。
ui-player_tracker-clan_sightings_hover = このうち { $range } 件が選択期間内です。
ui-player_tracker-clan_member_matches = { $count } 件の試合で確認
ui-player_tracker-show_division_mates = 分艦隊のメンバーを表示
ui-player_tracker-show_division_mates_hover = 分艦隊を組んでプレイした試合も数に含めます。自分が招待して出会ったプレイヤーであるため、デフォルトではオフです。同じプレイヤーとの他の試合は常に数に含まれます。
ui-player_tracker-seen_in_twitch_chat = Twitch チャットで確認
ui-player_tracker-seen_in_twitch_chat_hover = 自分との試合中に、名前が Twitch チャットのログイン名と一致したプレイヤーのみを表示します。
ui-player_tracker-twitch_seen_hover = 自分との { $count } 試合で Twitch チャットに { $logins } として確認
ui-player_tracker-win_rate_overall = 全体
ui-player_tracker-win_rate_ship = 艦船
ui-player_tracker-view_compact = コンパクト
ui-player_tracker-view_detailed = 詳細
ui-player_tracker-ship_stats = 艦船統計
ui-player_tracker-stats_resolving = 試合の参加者一覧を読み込み中...
ui-player_tracker-stats_fetching = プレイヤー統計を取得中...
ui-player_tracker-stats_unavailable = 統計を利用できません: { $reason }
ui-player_tracker-stats_hidden = このプレイヤーのプロフィールは非公開です
ui-player_tracker-stats_no_data = このプレイヤーの統計はありません
ui-player_tracker-pr_overall_hover = 全艦船を通じたアカウントの PR
ui-player_tracker-pr_ship_hover = この艦船での PR
ui-player_tracker-win_rate_hover_line = { $label }: { $battles } 戦で { $rate }
ui-player_tracker-win_rate_hover_line_damage = { $label }: { $battles } 戦で { $rate }、平均ダメージ { $damage }
ui-player_tracker-team_average_win_rate = 平均勝率: { $rate }
ui-player_tracker-team_players = { $count } 人のプレイヤー
ui-player_tracker-debug_replay_picker = デバッグ: リプレイを現在の試合として扱う
ui-player_tracker-debug_replay_picker_hover = 完了したリプレイの参加者一覧をこのタブに読み込み、戦闘外でも統計処理を確認できるようにします。
ui-player_tracker-debug_replay_none = リプレイを選択

## ui.player_tracker.column
ui-player_tracker-column-win_rate = 勝率
ui-player_tracker-column-personal_rating = PR
ui-player_tracker-column-battles = 戦闘数
ui-player_tracker-column-avg_damage = ダメージ
ui-player_tracker-column-encounters = 遭遇
ui-player_tracker-column-clan = クラン
ui-player_tracker-column-player_name = プレイヤー名
ui-player_tracker-column-total_encounters = 総遭遇回数
ui-player_tracker-column-encounters_in_range = 期間内遭遇回数
ui-player_tracker-column-last_encountered = 最終遭遇
ui-player_tracker-column-members = メンバー
ui-player_tracker-column-matches = 試合数
ui-player_tracker-column-sightings = 目撃回数

## ui.player_tracker.period
ui-player_tracker-period-past_hour = 過去1時間
ui-player_tracker-period-past_six_hours = 過去6時間
ui-player_tracker-period-past_day = 過去24時間
ui-player_tracker-period-past_week = 過去1週間
ui-player_tracker-period-past_month = 過去1ヶ月
ui-player_tracker-period-all_time = 全期間

# -- Mod管理 --

## ui.twitch
ui-twitch-possible_name = 配信名の候補: { $name }
ui-twitch-seen_minutes = 確認: 試合開始から { $minutes } 分後
ui-twitch-click_to_copy = クリックでコピー
ui-twitch-copied = { $name } をコピーしました

# -- リプレイレンダラー --

## ui.renderer
ui-renderer-preparing_export = エクスポートを準備中...
ui-renderer-encoding = エンコード中
ui-renderer-muxing = 多重化中

## ui.renderer.advantage
ui-renderer-advantage-breakdown = アドバンテージ内訳
ui-renderer-advantage-team_eliminated = チームが全滅しました
ui-renderer-advantage-score_projection = スコア予測
ui-renderer-advantage-fleet_power = 艦隊戦力
ui-renderer-advantage-strategic_threat = 戦略的脅威
ui-renderer-advantage-total = 合計
ui-renderer-advantage-hp_incomplete = HP/艦船データが不完全。スコア要因のみ表示

## ui.renderer.context
ui-renderer-context-show_trail = 航跡を表示
ui-renderer-context-disable_other_trails = 他の全ての航跡を無効化
ui-renderer-context-ranges = 射程
ui-renderer-context-detection = 被発見距離
ui-renderer-context-main_battery = 主砲
ui-renderer-context-secondary = 副砲
ui-renderer-context-torpedo = 魚雷
ui-renderer-context-radar = レーダー
ui-renderer-context-hydro = ソナー
ui-renderer-context-enable_all = 全て有効
ui-renderer-context-disable_all = 全て無効
ui-renderer-context-disable_other_ranges = 他の全ての射程を無効化
ui-renderer-context-enable_all_ranges = 全艦船の射程を有効化
ui-renderer-context-realtime_armor = リアルタイム装甲を表示

## ui.renderer.controls
ui-renderer-controls-jump_to_start = 開始位置へ
ui-renderer-controls-previous_event = 前のイベント (Shift+Left)
ui-renderer-controls-back_10s = 10秒戻る (Left)
ui-renderer-controls-forward_10s = 10秒進む (Right)
ui-renderer-controls-next_event = 次のイベント (Shift+Right)
ui-renderer-controls-jump_to_end = 終了位置へ
ui-renderer-controls-speed = 再生速度

## ui.renderer.settings
ui-renderer-settings-title = 表示設定
ui-renderer-settings-hp_bars = HP バー
ui-renderer-settings-armament = 兵装
ui-renderer-settings-dead_ships = 撃沈された艦船
ui-renderer-settings-dead_ship_names = 撃沈された艦船名
ui-renderer-settings-player_names = プレイヤー名
ui-renderer-settings-ship_ranges = 艦船の射程
ui-renderer-settings-ship_names = 艦船名
ui-renderer-settings-camera_direction = 砲塔の方向
ui-renderer-settings-heat_trail = ヒートトレイル
ui-renderer-settings-speed_trails = 速度トレイル
ui-renderer-settings-dead_ship_trails = 撃沈艦の航跡
ui-renderer-settings-buildings = 建物
ui-renderer-settings-capture_points = 占領ポイント
ui-renderer-settings-consumables = 消耗品
ui-renderer-settings-planes = 航空機
ui-renderer-settings-smoke = 煙幕
ui-renderer-settings-torpedoes = 魚雷
ui-renderer-settings-tracers = 曳光弾
ui-renderer-settings-battle_result = 戦闘結果
ui-renderer-settings-buff_counters = バフカウンター
ui-renderer-settings-chat_label = チャット
ui-renderer-settings-kill_feed = 撃沈フィード
ui-renderer-settings-score_label = スコア
ui-renderer-settings-score_timers = スコアタイマー
ui-renderer-settings-team_advantage = チームアドバンテージ
ui-renderer-settings-team_rosters = チーム編成
ui-renderer-settings-stats_panel = 統計パネル
ui-renderer-settings-timer = タイマー
ui-renderer-settings-export_settings = エクスポート設定
ui-renderer-settings-prefer_cpu = CPU エンコーダーを優先
ui-renderer-settings-prefer_cpu_tooltip = 動画エクスポートに GPU ではなくソフトウェア (CPU) エンコーダーを使用
ui-renderer-settings-include_pre_battle = 戦闘開始前を含める
ui-renderer-settings-include_pre_battle_tooltip = エクスポートする動画に戦闘開始前のスポーンとカウントダウンを含めます
ui-renderer-settings-export_timing = 動画のタイミング
ui-renderer-settings-export_timing_length = 長さに合わせる
ui-renderer-settings-export_timing_speed = 速度
ui-renderer-settings-export_timing_result_speed = 約{ $speed }xで再生されます
ui-renderer-settings-export_timing_result_length = 動画の長さは{ $length }です
ui-renderer-settings-codec = コーデック
ui-renderer-settings-codec_auto = 自動
ui-renderer-settings-save_defaults = デフォルトを保存
ui-renderer-settings-save_defaults_done = レンダラー起動時の既定値として保存しました
ui-renderer-settings-save_defaults_failed = 既定値を保存できませんでした
ui-renderer-settings-event_timeline = イベントタイムライン

# -- ファイル展開 --

## ui.unpacker
ui-unpacker-assets_bin = Assets.bin
ui-unpacker-loading_assets = Assets.bin を読み込み中...
ui-unpacker-load_failed = 読み込みに失敗: { $error }
ui-unpacker-no_game_data = ゲームデータが読み込まれていません。設定で World of Warships ディレクトリを設定してください。
ui-unpacker-filter_files = ファイルをフィルター...
ui-unpacker-search_in_files = ファイル内検索
ui-unpacker-search_hint = ファイル内を検索...
ui-unpacker-path_filter_hint = パスフィルター (例: *.xml)
ui-unpacker-queue_all_files = 全ファイルをキューに追加
ui-unpacker-view_as_json = JSON として表示
ui-unpacker-extract_as_json = JSON として抽出
ui-unpacker-view_contents = 内容を表示
ui-unpacker-version = バージョン:
ui-unpacker-decode_prototypes = プロトタイプを JSON としてデコード
ui-unpacker-decode_prototypes_tooltip = 抽出時にサポートされている Assets.bin プロトタイプ (.visual, .model, .mfm) を JSON にデコード
ui-unpacker-extraction_queue = 抽出キュー
ui-unpacker-collapse_folders = フォルダーツリーを折りたたむ
ui-unpacker-expand_folders = フォルダーツリーを展開
ui-unpacker-clear_all = 全てクリア
ui-unpacker-browse = 参照
ui-unpacker-output_dir_hint = 出力ディレクトリ...
ui-unpacker-remove_from_queue = キューから削除
ui-unpacker-queue_folder = フォルダ全体をキューに追加
ui-unpacker-queue_file = このファイルをキューに追加
ui-unpacker-extraction_output = 展開先
ui-unpacker-queue_empty = 一覧のファイルまたはフォルダーにチェックを入れてキューに追加します
ui-unpacker-packages = パッケージ
ui-unpacker-item_count = { $count } 項目
ui-unpacker-search_title = 検索: { $query }
ui-unpacker-search_in = { $source } 内
ui-unpacker-image_decode_failed = この画像をデコードできませんでした: { $reason }
ui-unpacker-find = 検索
ui-unpacker-find_previous = 前の一致
ui-unpacker-find_next = 次の一致
ui-unpacker-find_close = 検索を閉じる
ui-unpacker-find_count = { $current } / { $total }
ui-unpacker-find_none = 一致なし
ui-unpacker-find_match_case = 大文字と小文字を区別
ui-unpacker-queue = キュー
ui-unpacker-reveal_in_tree = パッケージツリーで表示
ui-unpacker-hit_offset = オフセット { $offset }
ui-unpacker-queued_count = { $count } 件キュー済み
ui-unpacker-counting_files = ファイルを数えています...
ui-unpacker-extracting = 展開中: { $done } / { $total }
ui-unpacker-extract_failed = 展開に失敗しました: { $reason }
ui-unpacker-extracted = { $count } 個のファイルを展開しました
ui-unpacker-cancelled = { $count } 個のファイルの後でキャンセルしました
ui-unpacker-extract = { $count ->
    [0] 展開
   *[other] { $count } 項目を展開
}
ui-unpacker-choose_directory = 先に展開先ディレクトリを選択してください
ui-unpacker-dump_parameters = GameParams をエクスポート
ui-unpacker-dump_parameters_tooltip = このビルドの GameParams.data をエクスポート
ui-unpacker-load_a_build = 先にビルドを読み込んでください
ui-unpacker-parameters_written = パラメーターを書き出しました
ui-unpacker-save_parameters_title = ゲームパラメーターを保存
ui-unpacker-writing = 書き出し中...
ui-unpacker-prototype_written = プロトタイプを書き出しました
ui-unpacker-whole_tree = ツリー全体
ui-unpacker-base_only = 基本パラメーターのみ

## ui.unpacker.column
ui-unpacker-column-name = 名前
ui-unpacker-column-size = サイズ
ui-unpacker-column-type_col = タイプ

# -- 装甲ビューアー --

## ui.armor
ui-armor-load_settings = 装甲ビューアーを使用するには、設定でゲームデータを読み込んでください。
ui-armor-load_failed = 艦船データの読み込みに失敗: { $error }
ui-armor-empty = 空
ui-armor-mirror_cameras = カメラをミラー
ui-armor-sync_options = オプションを同期
ui-armor-show_armor_by_default = デフォルトで装甲を表示
ui-armor-sync_tooltip = 全ペイン間で装甲/船体の表示と表示オーバーレイ（船体中心、喫水線、カメラリング）を同期
ui-armor-common_settings = 共通設定
ui-armor-tier_filter = Tier { $lowest } から { $highest }
ui-armor-fortifications = 要塞
ui-armor-show_hidden_ships = 非表示の艦艇を表示
ui-armor-show_hidden_ships_tooltip = クラン貸与艦など、一覧から除外されている艦艇も表示します
ui-armor-compare_split = 新しい分割で比較
ui-armor-open_in_new_tab = 新しいタブで開く
ui-armor-export_model = 艦船モデルをエクスポート
ui-armor-export_disclaimer = これらの3Dモデルとテクスチャは Wargaming の知的財産であり、これらのモデルの使用は各地域の法律に準拠する必要があります。
ui-armor-export_button = エクスポート
ui-armor-hull_toggle = 船体
ui-armor-splash_tooltip = スプラッシュボックスの表示を切り替え
ui-armor-show_hidden = 非表示を表示
ui-armor-show_hidden_tooltip = (場合によっては)非表示のパネルの表示を切り替え
ui-armor-gaps = 隙間
ui-armor-gaps_tooltip = 装甲パネルが接続されていない境界エッジをハイライト(潜在的な隙間)
ui-armor-display = 表示
ui-armor-marker_opacity = マーカーの不透明度
ui-armor-export_tooltip = 艦船モデルを OBJ ファイルにエクスポート
ui-armor-pen_check = 貫通チェック
ui-armor-trajectory = 弾道
ui-armor-trajectory_range = 距離
ui-armor-trajectory_clear = 全てクリア
ui-armor-arc_heading = 弾道 { $number }
ui-armor-arc_summary = { $hits } 命中、{ $range } km で { $armor }mm
ui-armor-arc_delete = この弾道を削除
ui-armor-arc_isolate_plates = 装甲板を分離表示
ui-armor-arc_isolate_plates_tooltip = この弾道が通過した装甲板のみを表示
ui-armor-arc_isolate_zones = 区画を分離表示
ui-armor-arc_isolate_zones_tooltip = この弾道が入った装甲区画の全体を表示
ui-armor-arc_simulation_caveat = このシミュレーションはリバースエンジニアリングしたデータに基づくため、ゲームの弾道計算と一致しない場合があります。
ui-armor-arc_no_shell = 砲弾が指定されていないため、通過した装甲板のみを表示しています
ui-armor-arc_detonated_in = { $zone } 内で炸裂
ui-armor-arc_detonated_past_armor = 全装甲を通過後に炸裂
ui-armor-arc_ricochet = { $plate } で跳弾
ui-armor-arc_shatter = { $plate } で不貫通
ui-armor-arc_stopped = { $plate } で停止
ui-armor-arc_overpen = 全貫通
ui-armor-arc_overpen_unarmed = 全貫通 (信管未作動)
ui-armor-arc_plate_unknown = 未記録の装甲板
ui-armor-angle_good = 30 度未満
ui-armor-angle_mid = 30 から 45 度
ui-armor-angle_bad = 45 度超
ui-armor-trajectory_ship = 艦船:
ui-armor-trajectory_select_ship = 艦船を選択
ui-armor-trajectory_ship_tooltip = 弾道計算に使う砲弾の艦船を選択
ui-armor-trajectory_add_ship = 射撃艦を追加
ui-armor-trajectory_add_ship_tooltip = 貫通チェックを開いて艦船を検索・追加
ui-armor-trajectory_shell = 砲弾:
ui-armor-trajectory_select_shell = 砲弾を選択
ui-armor-trajectory_shell_tooltip = 弾道計算に使う砲弾を選択
ui-armor-continue_ricochet = 跳弾後も継続
ui-armor-splash_mode = スプラッシュ
ui-armor-splash_no_data = この艦船にはスプラッシュデータがありません
ui-armor-splash_no_shells = 比較リストに HE/SAP 砲弾を持つ艦船を追加してください
ui-armor-splash_tooltip_mode = 装甲をクリックして HE スプラッシュダメージ範囲を可視化
ui-armor-select_ship = リストから艦船を選択
ui-armor-all_btn = 全て
ui-armor-none_btn = なし
ui-armor-opaque = 不透明
ui-armor-upgrade = アップグレード:
ui-armor-camo_none = 標準（なし）
ui-armor-camo_group_universal = 汎用迷彩
ui-armor-camo_group_expendable = 消耗型迷彩
ui-armor-camo_group_other = その他の迷彩
ui-armor-show_armor_thickness = 装甲厚の凡例を表示
ui-armor-plate_edges = プレートの境界
ui-armor-waterline = 喫水線
ui-armor-camera_rings = カメラリング
ui-armor-realtime_readout = { $health } HP  -  { $hits } 命中
ui-armor-show_camera_rings = カメラリングを表示
ui-armor-camera_fov = カメラ FOV
ui-armor-camera_height = カメラの高さ
ui-armor-show_camera_zoom_path = ズームパスを表示
ui-armor-zoom_path_regular_fov = 通常 FOV
ui-armor-zoom_path_max_fov = 最大 FOV
ui-armor-camera_perspective = カメラ視点
ui-armor-perspective_fov = 視野 FOV
ui-armor-perspective_projection = 投影
ui-armor-perspective_proj_game = ゲーム
ui-armor-perspective_proj_center = 中心を通る
ui-armor-ship_center = 船体中心
ui-armor-grid = グリッド
ui-armor-length_marks = 長さの目盛り
ui-armor-length_readout = 全長 { $length }、全幅 { $beam }
ui-armor-length_metres = { $value } m
ui-armor-length_centimetres = { $value } cm
ui-armor-filter_options = オプションを絞り込む...
ui-armor-filter_zones = 区画を絞り込む...
ui-armor-filter_parts = 部品を絞り込む...
ui-armor-tree_matches = 一致: { $count }
ui-armor-tree_no_matches = 一致なし
ui-armor-no_matching_options = フィルターに一致するオプションはありません
ui-armor-opacity = 不透明度
ui-armor-opaque_hull = 不透明な船体
ui-armor-lighting = 船体ライティング
ui-armor-lighting_enabled = ライティングを有効にする
ui-armor-lighting_preset_ingame = ゲーム内
ui-armor-lighting_preset_flat = フラット
ui-armor-lighting_preset_studio = スタジオ
ui-armor-lighting_flat = フラット（アンビエント）
ui-armor-lighting_intensity = ライト強度
ui-armor-lighting_azimuth = ライト方位角
ui-armor-lighting_elevation = ライト仰角
ui-armor-lighting_rim = リムライト
ui-armor-lighting_specular = スペキュラー
ui-armor-lighting_shininess = シャイネス
ui-armor-lighting_key_color = キーライト色
ui-armor-lighting_ambient_color = アンビエント色
ui-armor-zero_mm_plates = 0 mm プレート
ui-armor-armor_opacity = 装甲の不透明度
ui-armor-ballistic_disclaimer = 弾道結果はシミュレーションの仕組みのリバースエンジニアリング/推定に基づいています
ui-armor-roll = ロール
ui-armor-reset_plates = プレートをリセット
ui-armor-armor_options = 装甲
ui-armor-no_ship_loaded = 艦船が読み込まれていません
ui-armor-lod = LOD:
ui-armor-lod_highest = 0 (最高)
ui-armor-camouflage = 迷彩
ui-armor-ships = 艦船
ui-armor-compare = 比較
ui-armor-compare_tooltip = 新しい比較ペインを開きます。艦船を右クリックするとその艦船と比較します
ui-armor-mirror_cameras_tooltip = 全ペインのカメラを連動させる
ui-armor-show_armor_by_default_tooltip = モデルを船体ではなく装甲を表示した状態で開く
ui-armor-xray_highlight = ビュー内ホバー時のX線ハイライト
ui-armor-xray_always = 常に
ui-armor-xray_while_held = { $key } を押している間
ui-armor-xray_off = オフ
ui-armor-stack_panes = ペインを縦に並べる
ui-armor-stack_panes_tooltip = 比較ペインを横並びではなく縦に並べます
ui-armor-viewports = ビューポート
ui-armor-options_panel = オプションパネル
ui-armor-options_pane = ペイン { $number } / { $count }
ui-armor-splash_boxes = スプラッシュボックス
ui-armor-cast_here_hint = { $key } を押しながらクリックでここに砲弾を発射
ui-armor-trajectory_next_cast = 次の発射
ui-armor-arc_pick_hint = マーカーをクリックすると弾道を編集できます。
ui-armor-arc_marker_tooltip = 弾道 { $number }。クリックで編集、右クリックで削除します。
ui-armor-zone_toggle_tooltip = この区画のすべての装甲板を表示または非表示にします。{ $key } を押しながらクリックすると、この区画のみを表示します。
ui-armor-mode_solo_zone = 区画のみ表示
ui-armor-mode_cast_trajectory = 弾道を発射
ui-armor-loading_catalog = 艦船カタログを読み込み中...
ui-armor-legend_collapse = 折りたたむ
ui-armor-legend_hide = 凡例を非表示
ui-armor-armor_thickness_title = 装甲厚
ui-armor-initializing = 3D ビューポートを初期化中...
ui-armor-init_failed = 装甲ビューポートの初期化に失敗しました: { $reason }
ui-armor-loading_named_ship = { $ship } を読み込み中...
ui-armor-options = オプション

## ui.armor.compass
ui-armor-compass-north = 北
ui-armor-compass-east = 東
ui-armor-compass-south = 南
ui-armor-compass-west = 西

## ui.armor.realtime
ui-armor-realtime-incoming_fire = 被弾
ui-armor-realtime-attacker_filter = 攻撃者フィルター
ui-armor-realtime-all_enemies = 全ての敵
ui-armor-realtime-show_secondaries = 副砲を表示
ui-armor-realtime-seek_to_salvo = 斉射位置へシーク
ui-armor-realtime-no_data = 砲弾データはもう利用できません
ui-armor-realtime-sim_agrees = シミュレーション一致
ui-armor-realtime-sim_unchecked = { $count } 発の砲弾をシミュレートできませんでした
ui-armor-realtime-sim_rng_zone = RNGゾーン ({ $from }-{ $to } 中 { $angle } 度)
ui-armor-realtime-sim_disagrees = シミュレーション: { $sim } / サーバー: { $server }
ui-armor-realtime-no_armor_hit = 装甲パネルへの命中なし
ui-armor-realtime-tracked = { $salvos } 斉射、{ $shells } 発の砲弾を追跡
ui-armor-realtime-nothing_landed = この艦船にはまだ命中弾がありません
ui-armor-realtime-shells = { $count }x
ui-armor-realtime-salvo_shell = { $caliber } mm { $shell }
ui-armor-realtime-salvo_range = { $km } km
ui-armor-realtime-unknown_attacker = 艦船 { $id }
ui-armor-realtime-server_outcome = サーバー: { $outcome }
ui-armor-realtime-not_cast = この砲弾は船体内を追跡できません: 弾道または艦の位置が記録されていません
ui-armor-realtime-pick_hint = 斉射または砲弾をクリックすると船体内の弾道を表示します

## ui.armor.plates
ui-armor-plates-heading = 命中した装甲板 ({ $count })
ui-armor-plates-arrival = 着弾時 { $velocity } m/s、飛翔 { $time } 秒、落角 { $fall } 度
ui-armor-plates-overmatch = オーバーマッチ: 貫通力 { $penetration } mm、{ $velocity } m/s
ui-armor-plates-penetrated = 貫通: { $penetration } mm 対 実効 { $effective } mm、{ $velocity } m/s
ui-armor-plates-ricochet = { $angle } 度で跳弾
ui-armor-plates-shatter = 破砕: { $penetration } mm 対 実効 { $effective } mm
ui-armor-plates-fuse_armed = 信管作動
ui-armor-plates-detonation = { $zone } 内で炸裂、装甲板 #{ $number } から { $distance } m
ui-armor-plates-detonation_past_armor = 全装甲の外側で炸裂、装甲板 #{ $number } から { $distance } m
ui-armor-plates-flat_penetrates = { $ammo } 貫通: { $penetration } mm 対 { $thickness } mm
ui-armor-plates-flat_shatters = { $ammo } 破砕: { $penetration } mm 対 { $thickness } mm
ui-armor-plates-server_angle = サーバー { $angle } 度

# -- アドバンテージレベルラベル --

## advantage
advantage-absolute = 絶対的
advantage-strong = 強い
advantage-moderate = 中程度
advantage-weak = 弱い

## ui.search
ui-search-edit_value = 値を編集
ui-search-add_filter = フィルターを追加
ui-search-pill_select = 選択
ui-search-pill_deselect = 選択解除
ui-search-pill_group_all = and でグループ化
ui-search-pill_group_any = or でグループ化
ui-search-pill_ungroup = グループ解除
ui-search-pill_negate = 否定
ui-search-pill_flip_connector = and/or を切り替え
ui-search-pill_delete = 削除
ui-search-match_count_truncated = { $count }+ 件の試合
ui-search-outcome_win = 勝利
ui-search-outcome_loss = 敗北
ui-search-outcome_draw = 引き分け
ui-search-outcome_unknown = 不明
ui-search-bool_true = はい
ui-search-bool_false = いいえ
ui-search-match_count = { $count } 件の試合
ui-search-row_actions_hint = リプレイ操作 (または行を右クリック)
ui-search-looking_up_values = 値を検索中...
ui-search-type_a_query = クエリを入力して Enter を押してください
ui-search-parse_failed = クエリを解析できませんでした: { $reason }
ui-search-searching = 検索中...
ui-search-failed = 検索に失敗しました: { $reason }
ui-search-no_matches = 一致なし
ui-search-game_mode_gap_hint = インデックス済みの試合 { $count } 件にゲームモードが記録されていません。設定の「全リプレイを再インデックス化」で補完してください。
ui-search-open = 開く
ui-search-open_missing = リプレイファイルは既に存在しません
ui-search-free_text = "{ $text }" を含む
ui-search-quant_any = いずれか
ui-search-quant_none = なし
ui-search-quant_ge = { $n } 以上
ui-search-quant_eq = ちょうど { $n }
ui-search-quant_le = { $n } 以下
ui-search-quant_gt = { $n } より多い
ui-search-quant_lt = { $n } 未満
ui-search-quant_ne = { $n } 以外
ui-search-scope_self = 自分の
ui-search-scope_ally = 味方の
ui-search-scope_enemy = 敵の
ui-search-scope_div = 分艦隊の

## ui.search.suggest
ui-search-suggest-preset_divmate_test_ship = 分艦隊のメンバーがテスト艦でプレイした
ui-search-suggest-preset_no_enemy_cv = 敵に空母がいない
ui-search-suggest-preset_all_enemies_died = 敵が全滅した
ui-search-suggest-preset_high_damage_enemies = 10 万超の敵が 3 隻以上
ui-search-suggest-preset_stream_sniper = ストリームスナイパーを含む
ui-search-suggest-preset_i_survived = 自分が生存した
ui-search-suggest-preset_i_disconnected = 自分が切断した
ui-search-suggest-my_damage = 自分のダメージ
ui-search-suggest-enemy_ship = 敵の艦船
ui-search-suggest-allied_ship = 味方の艦船
ui-search-suggest-player_in_match = 試合に参加していたプレイヤー
ui-search-suggest-someone_in_division = 分艦隊のメンバー
ui-search-suggest-scope_self = 自分の
ui-search-suggest-scope_ally = 味方の
ui-search-suggest-scope_enemy = 敵の
ui-search-suggest-scope_division = 分艦隊の
ui-search-suggest-scope_anyone = 任意のプレイヤーの

## ui.search.field
ui-search-field-outcome = 結果
ui-search-field-map = マップ
ui-search-field-class = 艦種
ui-search-field-tier = ティア
ui-search-field-date = 日付
ui-search-field-player_present = プレイヤー
ui-search-field-group = リプレイグループ
ui-search-field-stat_damage = ダメージ
ui-search-field-stat_kills = 撃沈
ui-search-field-stat_spotting = 観測ダメージ
ui-search-field-stat_potential = 潜在ダメージ
ui-search-field-stat_received = 被ダメージ
ui-search-field-stat_pr = PR
ui-search-field-rating = レーティング
ui-search-field-stat_survived = 生存
ui-search-field-stat_disconnected = 切断
ui-search-field-game_type = ゲームタイプ
ui-search-field-game_mode = ゲームモード
ui-search-field-match_group = マッチグループ
ui-search-field-build = ビルド
ui-search-field-results_available = 結果の有無
ui-search-field-relation = 関係
ui-search-field-division = 分艦隊
ui-search-field-name = 名前
ui-search-field-clan = クラン
ui-search-field-realm = サーバー
ui-search-field-ship = 艦船
ui-search-field-ship_index = 艦船インデックス
ui-search-field-nation = 国家
ui-search-field-test_ship = テスト艦
ui-search-field-stream_sniper = ストリームスナイパー
ui-search-field-sniper_login = スナイパーの Twitch ログイン名

## ui.search.op
ui-search-op-contains = 含む
ui-search-op-equals = 一致
ui-search-op-not_equals = 不一致
ui-search-op-eq = =
ui-search-op-ne = !=
ui-search-op-gt = >
ui-search-op-ge = >=
ui-search-op-lt = <
ui-search-op-le = <=
ui-search-op-is = は
ui-search-op-is_not = 以外
ui-search-op-is_set = 設定あり
ui-search-op-is_not_set = 設定なし

## ui.search.value
ui-search-value-relation_self = 自分
ui-search-value-relation_ally = 味方
ui-search-value-relation_enemy = 敵
ui-search-value-division_mine = 自分の分艦隊
ui-search-value-division_any = 任意
ui-search-value-division_none = なし
ui-search-value-class_air_carrier = 空母
ui-search-value-class_battleship = 戦艦
ui-search-value-class_cruiser = 巡洋艦
ui-search-value-class_destroyer = 駆逐艦
ui-search-value-class_submarine = 潜水艦
ui-search-value-class_auxiliary = 補助艦艇
ui-search-value-rating_bad = 悪い
ui-search-value-rating_below_average = 平均以下
ui-search-value-rating_average = 平均
ui-search-value-rating_good = 良い
ui-search-value-rating_very_good = とても良い
ui-search-value-rating_great = 素晴らしい
ui-search-value-rating_unicum = ユニカム
ui-search-value-rating_super_unicum = スーパーユニカム
ui-search-value-game_mode_test = テスト
ui-search-value-game_mode_standard = 標準戦
ui-search-value-game_mode_singlebase = シングルベース
ui-search-value-game_mode_domination = 制圧戦
ui-search-value-game_mode_tutorial = チュートリアル
ui-search-value-game_mode_megabase = メガベース
ui-search-value-game_mode_forts = 要塞
ui-search-value-game_mode_standard_domination = 標準制圧戦
ui-search-value-game_mode_epicenter = エピセンター
ui-search-value-game_mode_assault_defense = 強襲防衛
ui-search-value-game_mode_pve = PVE
ui-search-value-game_mode_arms_race = 軍拡競争
ui-search-value-game_mode_epicenter_ring = エピセンターリング
ui-search-value-game_mode_anti_standard = アンチスタンダード
ui-search-value-game_mode_attack_defense = 攻撃防衛
ui-search-value-game_mode_torpedo_beat = トーピードビート
ui-search-value-game_mode_team_battle_royale = チームバトルロイヤル
ui-search-value-game_mode_escape_to_portal = ポータルへの脱出
ui-search-value-game_mode_domination_asymm = 非対称制圧戦
ui-search-value-game_mode_key_battle = キーバトル
ui-search-value-game_mode_portal_2021 = ポータル 2021
ui-search-value-game_mode_team_battle_royale_2021 = チームバトルロイヤル 2021
ui-search-value-game_mode_convoy_event = 船団護衛イベント
ui-search-value-game_mode_convoy_airship = 船団護衛（飛行船）
ui-search-value-game_mode_two_teams_battle_royale = 2 チームバトルロイヤル
ui-search-value-game_mode_pinata_event = ピニャータイベント
ui-search-value-game_mode_respawns = リスポーン
ui-search-value-game_mode_respawns_sectors = リスポーン（セクター）

## ui.search.column
ui-search-column-date = 日付
ui-search-column-map = マップ
ui-search-column-mode = モード
ui-search-column-ship = 自艦
ui-search-column-result = 結果
ui-search-column-damage = ダメージ
ui-search-column-kills = 撃沈
ui-search-column-pr = PR
ui-search-column-mc = MC

## ui.settings.index
ui-settings-index-heading = リプレイインデックス
ui-settings-index-description = ディレクトリ内の全リプレイの検索用記録
ui-settings-index-build = インデックスを作成
ui-settings-index-rebuild = 全て再インデックス
ui-settings-index-rebuild_hint = 古い解析で誤った行を修正するため、全リプレイを再度読み込みます。「インデックスを作成」は不足分のみを読み込みます。
ui-settings-index-progress = { $done } / { $total } 読み込み済み、{ $failed } 件失敗
ui-settings-index-progress_skipped = { $done } / { $total } 読み込み済み、{ $skipped } 件登録済み、{ $failed } 件失敗
ui-settings-index-built = { $indexed } 件のリプレイをインデックス化、{ $failed } 件は読み込めませんでした
ui-settings-index-built_with_skipped = { $indexed } 件のリプレイをインデックス化、{ $skipped } 件は登録済み、{ $failed } 件は読み込めませんでした

## ui.settings
ui-settings-loading = 設定を読み込み中...
ui-settings-load_failed = 設定の読み込みに失敗しました
ui-settings-not_set = (未設定)

## ui.settings.panel
ui-settings-panel-mcp = MCPサーバー
ui-settings-panel-open = 設定を開く
ui-settings-panel-search_placeholder = 設定を検索
ui-settings-panel-count = { $count } 件の設定
ui-settings-panel-no_matches = 検索に一致する設定はありません
ui-settings-panel-close = 設定を閉じる
ui-settings-panel-in_error = { $name }: 確認が必要な設定があります
ui-settings-panel-general = 一般
ui-settings-panel-appearance = 外観
ui-settings-panel-game = World of Warships
ui-settings-panel-replays = リプレイ
ui-settings-panel-collaboration = 共同作業
ui-settings-panel-twitch = Twitch
ui-settings-panel-language_description = インターフェースの言語
ui-settings-panel-data_sharing_description = ランク戦およびランダム戦の後に ShipBuilds.com へ送信される内容
ui-settings-panel-code_integrity_description = Microsoft が署名していないコードをこのアプリケーションに読み込めるかどうか
ui-settings-panel-check_for_updates_description = アプリケーションの起動時に毎回新しいリリースを確認する
ui-settings-panel-data_folder = アプリデータフォルダー
ui-settings-panel-theme_description = インターフェースの色。選択肢にポインターを合わせるとプレビューします。
ui-settings-panel-zoom_description = すべてのテキストとコントロールのサイズ
ui-settings-panel-zoom = ズーム
ui-settings-panel-zoom_shortcuts = { $zoom_in } で拡大、{ $zoom_out } で縮小、{ $reset } で 1x に戻す
ui-settings-panel-zoom_changed = ズーム: { $factor }x
ui-settings-panel-cache_directory = キャッシュディレクトリ
ui-settings-panel-cache_directory_description = キャッシュされたビルドの保存場所
ui-settings-panel-cached_builds = キャッシュ済みビルド
ui-settings-panel-current_path_description = 最後に開いたリプレイファイル
ui-settings-panel-show_raw_xp_description = 結果テーブルに素の XP の列を追加する
ui-settings-panel-show_observed_damage_description = 結果テーブルに観測ダメージの列を追加する
ui-settings-panel-show_entity_id_description = 結果テーブルにエンティティ ID の列を追加する
ui-settings-panel-show_heals_description = 結果テーブルに回復量の列を追加する
ui-settings-panel-auto_export_description = 各戦闘のデータを選択した形式とディレクトリのファイルに書き出す
ui-settings-panel-display_name_description = セッション内で他の参加者に表示される名前
ui-settings-panel-get_token_description = チャットトークンを発行するサインインページを開く
ui-settings-panel-token = ログイントークン
ui-settings-panel-monitored_channel_description = 監視するチャンネル。空欄にすると自分のチャンネルを監視します。
ui-settings-panel-armor_defaults = 保存済み既定値
ui-settings-panel-armor_saved_description = モデルビューアーのオプションから保存

## ui.settings.licenses
ui-settings-licenses-title = 情報とライセンス
ui-settings-licenses-back = 設定に戻る
ui-settings-licenses-third_party_intro = このアプリケーションには以下のサードパーティ製コンポーネントが含まれています。項目を選択すると、それを使用するクレートとライセンスの全文が表示されます。

## ui.settings.armor
ui-settings-armor-no_defaults = (保存済みの既定値なし)

## ui.app
ui-app-status_cache_checking = ゲームデータキャッシュを確認中
ui-app-status_cache_validating = ゲームデータキャッシュを検証中
ui-app-status_cache_downloading = ゲームデータをダウンロード中
ui-app-status_cache_planning = 公開されているゲームデータを問い合わせ中
ui-app-drop_to_load = ドロップして { $file } を読み込む
ui-app-drop_to_list = ドロップして { $folder } のリプレイを一覧表示
ui-app-drop_to_list_replays = ドロップして { $count } 件のリプレイを一覧表示
ui-app-drop_to_list_folders = ドロップして { $count } 個のフォルダー内のリプレイを一覧表示
ui-app-drop_to_open_and_list = ドロップして { $file } を開き、{ $folders } 内のリプレイを一覧表示
ui-app-drop_to_list_replays_and_folders = ドロップして { $count } 件のリプレイと { $folders } 内のリプレイを一覧表示
ui-app-drop_folder_count = { $count } 個のフォルダー
ui-app-reading_replay = { $replay } を読み込み中
ui-app-reading_replays = { $count } 件のリプレイを読み込み中
ui-app-status_constants_generating = { $version } のゲーム定数を更新中
ui-app-worker_constants = ゲーム定数ワーカー
ui-app-worker_live_match = 戦闘監視
ui-app-worker_replay_loader = リプレイローダー
ui-app-worker_restarting = { $worker } が停止したため再起動しています
ui-app-worker_stalled = { $worker } が応答していません
ui-app-worker_stopped = { $worker } が停止しました
ui-app-status_indexing = リプレイをインデックス化中
ui-app-status_of = { $done } / { $total }
ui-app-status_tasks = { $count } 件のタスクを実行中
ui-app-status_show_panel = パネルを表示: { $panel }
ui-app-status_hide_panel = パネルを隠す: { $panel }
ui-app-panel_left = 左
ui-app-panel_right = 右
ui-app-panel_bottom = 下
ui-app-debug_build = デバッグビルド

## ui.palette
ui-palette-title = コマンド
ui-palette-placeholder = コマンドを入力または検索...
ui-palette-go_to = 移動: { $tab }
ui-palette-advanced_search = 詳細検索...
ui-palette-set_theme = テーマ: { $theme }
ui-palette-games_i_died_in = 自分が撃沈された試合
ui-palette-games_i_won = 自分が勝利した試合
ui-palette-send_all_replays = 全リプレイを ShipBuilds に送信
ui-palette-send_all_replays_again = 全リプレイを ShipBuilds に送信 (送信記録を無視)
ui-palette-search_players = プレイヤーを検索...
ui-palette-my_matches_in_ship = 艦船ごとの自分の試合...
ui-palette-open_ship_in_armor_viewer = 艦船をモデルビューアーで開く...
ui-palette-open_ship_in_shiptool = 艦船をShipToolで開く...
ui-palette-players_title = プレイヤーを検索
ui-palette-my_ships_title = 艦船ごとの自分の試合
ui-palette-armor_ships_title = 艦船をモデルビューアーで開く
ui-palette-shiptool_ships_title = 艦船をShipToolで開く
ui-palette-players_placeholder = 名前でプレイヤーを検索...
ui-palette-my_ships_placeholder = プレイしたことのある艦船を検索...
ui-palette-armor_ships_placeholder = 全艦船を名前で検索...
ui-palette-armor_ship_row = { $name } -- Tier { $tier } { $class }、{ $nation }
ui-palette-back_hint = Esc: 戻る
ui-palette-ships_loading = 艦船リストを読み込み中...
ui-palette-ships_unavailable = 艦船リストなし: { $reason }
ui-palette-matches_found = { $count } 件一致
ui-palette-sample_stats = デバッグ: 統計にサンプルデータを表示
ui-palette-restore_stats = デバッグ: 統計に記録データを表示

## ui.replay.hover
ui-replay-hover-ship = 艦船
ui-replay-hover-map = マップ
ui-replay-hover-mode = モード
ui-replay-hover-blank_mode = 空白
ui-replay-hover-played = プレイ日時
ui-replay-hover-damage = ダメージ
ui-replay-hover-kills = 撃沈
ui-replay-hover-result = 結果
ui-replay-hover-division = 分艦隊
ui-replay-hover-twitch = Twitchチャット

## ui.replay.renderer
ui-replay-renderer-export_video = 動画にレンダリング
ui-replay-renderer-export_clipboard = クリップボードにレンダリング
ui-replay-renderer-export_saved = 動画を { $path } に保存しました
ui-replay-renderer-export_copied = 動画をクリップボードにコピーしました
ui-replay-renderer-export_not_written = 動画をレンダリングできませんでした: { $reason }
ui-replay-renderer-pop_out = 別ウィンドウで開く
ui-replay-renderer-export_path_unusable = その場所には書き込めません
ui-replay-renderer-batch_started = { $count } 件のリプレイを { $dir } にレンダリング中
ui-replay-renderer-batch_finished = { $written } 件のリプレイをレンダリングしました。{ $failed } 件はレンダリングできませんでした
ui-replay-renderer-batch_all_written = { $written } 件のリプレイをレンダリングしました
ui-replay-renderer-batch_copied = { $written } 件のリプレイをレンダリングし、クリップボードにコピーしました
ui-replay-renderer-batch_not_copied = { $dir } にレンダリングしましたが、クリップボードへのコピーは拒否されました: { $reason }
ui-replay-renderer-batch_progress = レンダリング中 { $done } / { $total }: { $replay }
ui-replay-renderer-baking = リプレイを読み込み、戦闘を解析中...
ui-replay-renderer-failed = このリプレイは再生できませんでした
ui-replay-renderer-play = 再生
ui-replay-renderer-pause = 一時停止

## ui.renderer.annotations
ui-renderer-annotations-title = 描画ツール
ui-renderer-annotations-undo = 元に戻す
ui-renderer-annotations-redo = やり直す
ui-renderer-annotations-ship_hint = 艦船を検索
ui-renderer-annotations-no_ship = 艦船が選択されていません
ui-renderer-annotations-nib = 線の太さ
ui-renderer-annotations-thinner = 線を細く
ui-renderer-annotations-thicker = 線を太く
ui-renderer-annotations-shortcuts_title = キーボードショートカット
ui-renderer-annotations-shortcuts_tools = ツール
ui-renderer-annotations-shortcuts_actions = 操作
ui-renderer-annotations-tool_select = 選択
ui-renderer-annotations-tool_arrow = 矢印
ui-renderer-annotations-tool_freehand = フリーハンド
ui-renderer-annotations-tool_line = 直線
ui-renderer-annotations-tool_circle = 円
ui-renderer-annotations-tool_rectangle = 長方形
ui-renderer-annotations-tool_triangle = 三角形
ui-renderer-annotations-tool_measurement = 計測
ui-renderer-annotations-tool_eraser = 消しゴム
ui-renderer-annotations-action_undo = 元に戻す
ui-renderer-annotations-action_multi_select = 複数選択
ui-renderer-annotations-action_stroke_width = 線の太さ
ui-renderer-annotations-action_delete = 削除
ui-renderer-annotations-action_cancel = キャンセル / 選択解除
ui-renderer-annotations-toggle_fill = もう一度クリックすると塗りつぶしを切り替えます
ui-renderer-annotations-clear_all = すべて消去
ui-renderer-annotations-edit = 描画を編集
ui-renderer-annotations-size = サイズ
ui-renderer-annotations-color = 色
ui-renderer-annotations-custom_ink = カスタム色
ui-renderer-annotations-filled = 塗りつぶし
ui-renderer-annotations-locked = ホストが注釈をロックしています

## ui.armor.cast
ui-armor-cast-prompt = { $key }+クリックで船体に着弾点を追加
ui-armor-cast-empty = 着弾点はまだありません
ui-armor-cast-miss_nothing_under_pointer = カーソルの下に装甲板がありません
ui-armor-cast-miss_thickness_unknown = このモデルの装甲板は装甲厚が不明なため、発射できません
ui-armor-cast-miss_no_plate_on_path = この角度からの砲弾の弾道は装甲板と交差しません

## ui.armor.pen
ui-armor-pen-search_hint = 比較する艦船を検索...
ui-armor-pen-add_a_ship = 上で艦船を検索・追加して貫通力を比較します
ui-armor-pen-hover_a_plate = 艦船の装甲板にカーソルを合わせて確認します
ui-armor-pen-awaiting_plate = 装甲板にカーソルを合わせてください
ui-armor-pen-clear_all = 全てクリア
ui-armor-pen-remove = 比較から削除
ui-armor-pen-firing_ship_tooltip = この艦船の砲弾を弾道計算に使用
ui-armor-pen-use_for_trajectory = 弾道計算に使用
ui-armor-pen-no_shells = { $ship } には主砲弾がありません
ui-armor-pen-ifhe = IFHE
ui-armor-pen-ifhe_tooltip = 榴弾用慣性信管は HE の貫通力を 4 分の 1 引き上げます
ui-armor-pen-penetrates = 貫通
ui-armor-pen-shatters = 不貫通
ui-armor-pen-angle_dependent = 角度による
ui-armor-pen-no_data = 貫通データなし

## ui.armor.splash
ui-armor-splash-hint = スプラッシュモードで装甲をクリックすると、爆発が及ぶ区画を確認できます
ui-armor-splash-half_extent = 爆発範囲の半幅: { $value } モデル単位
ui-armor-splash-triangles = 爆発範囲内の装甲三角形: { $total }、貫通: { $penetrated }
ui-armor-splash-no_zones = この地点に重なるスプラッシュボックスはありません
ui-armor-splash-zones = 爆発範囲内の区画
ui-armor-splash-direct_hit = 直撃
ui-armor-splash-hp = { $value } HP
ui-armor-splash-section = スプラッシュゾーン

## ui.armor.export
ui-armor-export-hull = 船体
ui-armor-export-contents = 内容
ui-armor-export-contents_mesh = メッシュ
ui-armor-export-contents_armor = 装甲
ui-armor-export-contents_both = メッシュと装甲
ui-armor-export-lod = メッシュの詳細度
ui-armor-export-lod_tooltip = 0 が最高の詳細度です。装甲には詳細度がなく、影響を受けません。
ui-armor-export-res = テクスチャ解像度
ui-armor-export-res_full = フル
ui-armor-export-res_2048 = 2048 px
ui-armor-export-res_1024 = 1024 px
ui-armor-export-res_512 = 512 px
ui-armor-export-camos = 迷彩
ui-armor-export-camo_select_none = 選択なし
ui-armor-export-camo_reset = 既定値に戻す
ui-armor-export-loading_details = 読み込み中...
ui-armor-export-size_estimate = 推定ファイルサイズ: { $size }
ui-armor-export-size_breakdown = ジオメトリ { $geometry } (正確)、テクスチャ { $textures } (推定)
ui-armor-export-size_incomplete = { $count } 個の迷彩のサイズを算出できませんでした。サイズは不完全な可能性があります
ui-armor-export-exporting = { $ship } をエクスポート中...
ui-armor-export-already_running = エクスポートは既に実行中です
ui-armor-export-exported = { $ship } をエクスポートしました
ui-armor-export-exported_with_size = { $ship } をエクスポートしました ({ $size })
ui-armor-export-vehicle_not_found = 艦艇が見つかりません
ui-armor-export-export_failed = エクスポートに失敗しました: { $error }

## ui.tactics
ui-tactics-a_peers_board = このセッションの他の参加者が共有したボード
ui-tactics-map = マップ
ui-tactics-map_hint = マップを検索
ui-tactics-mode = モード
ui-tactics-pick_a_map = 描画するマップを選択
ui-tactics-drawing = マップを描画中...
ui-tactics-reset_view = 表示をリセット
ui-tactics-add_cap = 占領エリアを追加
ui-tactics-add_cap_tooltip = 次にマップをクリックした位置に占領エリアを配置
ui-tactics-selected_cap = 占領エリア { $letter }
ui-tactics-cap_radius = { $km } km
ui-tactics-cap_team = チーム
ui-tactics-cap_team_tooltip = 選択した占領エリアのチームを切り替え
ui-tactics-team_neutral = 中立
ui-tactics-team_one = チーム 1 (緑)
ui-tactics-team_two = チーム 2 (赤)
ui-tactics-delete_cap = 削除
ui-tactics-delete_cap_tooltip = 選択した占領エリアを削除
ui-tactics-cap_replay_delete_tooltip = リプレイから読み込んだ占領エリアは削除できません
ui-tactics-cap_narrow_tooltip = 選択した占領エリアの半径を縮小
ui-tactics-cap_widen_tooltip = 選択した占領エリアの半径を拡大
ui-tactics-undo_tooltip = ボードの最後の変更を元に戻す ({ $shortcut })
ui-tactics-redo_tooltip = 元に戻したボードの変更をやり直す ({ $shortcut } または { $alternate })
ui-tactics-clear_caps = 追加した占領エリアをクリア
ui-tactics-clear_added_caps_tooltip = ユーザーが追加した占領エリアを削除
ui-tactics-preset_name = ボード名
ui-tactics-preset_save = 保存
ui-tactics-preset_delete = 削除
ui-tactics-reset_board = ボードをリセット
ui-tactics-reset_board_tooltip = 選択したモードの占領エリアに戻し、描画をクリア
ui-tactics-preset_needs_a_name = 先にボード名を入力してください
ui-tactics-preset_needs_a_map = 先にボードのマップを設定してください
ui-tactics-preset_saved = { $name } を保存しました
ui-tactics-preset_save_failed = ボードを保存できませんでした: { $error }
ui-tactics-preset_load_failed = ボードを開けませんでした: { $error }
ui-tactics-preset_delete_failed = ボードを削除できませんでした: { $error }
ui-tactics-presets = 保存済みボード
ui-tactics-preset_open = 開く
ui-tactics-clear_drawing = 描画をクリア
ui-tactics-scan_replays = リプレイから占領エリアを取得
ui-tactics-scan_replays_tooltip = リプレイを読み込み、プレイされたモードの占領エリア配置を取得します
ui-tactics-scan_needs_game_data = 先にゲームビルドを読み込んでください
ui-tactics-scan_running = リプレイを読み込み中...
ui-tactics-scan_progress = リプレイを読み込み中... { $read } / { $total }
ui-tactics-scan_done = { $total } 件のリプレイを読み込み、新しい配置を { $added } 件見つけました
ui-tactics-session_boards_full = このセッションではすでに { $limit } 枚のボードが開いているため、このボードはマップを変更できません
ui-tactics-placing = { $ship } を配置中
ui-tactics-side_friendly = 味方
ui-tactics-side_enemy = 敵
ui-tactics-ink_white = 白インク
ui-tactics-ink_gray = 灰色インク
ui-tactics-ink_red = 赤インク
ui-tactics-ink_orange = オレンジインク
ui-tactics-ink_yellow = 黄色インク
ui-tactics-ink_green = 緑インク
ui-tactics-ink_blue = 青インク
ui-tactics-ink_purple = 紫インク
ui-tactics-ink_pink = ピンクインク
ui-tactics-no_map_art = このビルドにはそのマップの画像が含まれていません
ui-tactics-ship_config = 艦艇設定
ui-tactics-hull = 船体
ui-tactics-hull_default = 既定
ui-tactics-modifiers = 補正
ui-tactics-concealment_expert = 隠蔽処理専門家
ui-tactics-gun_range_mod = 主砲射程改良
ui-tactics-secondary_range_mod = 副砲射程改良
ui-tactics-ship_ranges = 射程円
ui-tactics-shape = 図形
ui-tactics-shape_size = サイズ
ui-tactics-shape_size_km = サイズ (km)
ui-tactics-shape_filled = 塗りつぶし
ui-tactics-shape_smaller = 図形を小さくする
ui-tactics-shape_larger = 図形を大きくする

## ui.armor.outcome
ui-armor-outcome-penetration = 貫通
ui-armor-outcome-citadel = 防郭
ui-armor-outcome-ricochet = 跳弾
ui-armor-outcome-shatter = 不貫通
ui-armor-outcome-overpenetration = 全貫通
ui-armor-outcome-underwater = 水中
ui-armor-outcome-unknown = 不明 ({ $raw })
ui-armor-outcome-stopped = 装甲内で停止

## ui.armor.sim
ui-armor-sim-overmatch_rules_out_ricochet = オーバーマッチのため跳弾なし
ui-armor-sim-always_ricochet_band = 跳弾 (確定跳弾角)
ui-armor-sim-ricochet_angle_too_low = 角度が浅いため跳弾なし

## ui.launcher
ui-launcher-rescan = 再スキャン
ui-launcher-add_install = インストールを追加...
ui-launcher-add_install_title = World of Warships のインストールディレクトリ
ui-launcher-no_installs = インストールが見つかりません。追加するか、下でゲームをインストールしてください。
ui-launcher-managed_by_steam = Steam で管理
ui-launcher-checking_updates = アップデートを確認中
ui-launcher-up_to_date = 最新
ui-launcher-update_available = アップデートあり: { $parts }
ui-launcher-not_installed = 未インストール: { $parts }
ui-launcher-check_failed = アップデートの確認に失敗: { $error }
ui-launcher-recheck = 再確認
ui-launcher-download = ダウンロード / アップデート...
ui-launcher-download_title = ビルドのインストール先
ui-launcher-download_hint = 新規インストールは数十 GB です。差分アップデートはまだ非対応のため、変更された部分は全体を再ダウンロードします。
ui-launcher-pause = 一時停止
ui-launcher-cancel = キャンセル
ui-launcher-resume = 再開
ui-launcher-paused = ダウンロードを一時停止しました。ダウンロード済みファイルは保持されます。
ui-launcher-installed = インストール済み
ui-launcher-installed_skipped = インストール済み ({ $count } 個の差分パーツをスキップ: 未対応のため、ダウンロード済みですが未適用)
ui-launcher-download_failed = ダウンロードに失敗しました: { $error }
ui-launcher-update_in_place = アップデートはそのインストール自体のフォルダーに入ります。更新するには { $path } を選択してください。
ui-launcher-cancelled = ダウンロードをキャンセルしました。
ui-launcher-launch_failed = 起動に失敗しました: { $error }
ui-launcher-use_for_toolkit = ツールキットで使用
ui-launcher-install_heading = ゲームをインストール
ui-launcher-install_hint = 新規インストールは数十 GB です。
ui-launcher-install_title = ゲームのインストール先
ui-launcher-install = インストール...
ui-launcher-checking_version = 利用可能なバージョンを確認中
ui-launcher-version = バージョン { $version }
ui-launcher-version_unreported = クライアントのバージョンが報告されていません
ui-launcher-unavailable = 利用不可: { $error }
ui-launcher-retry = 再試行

## ui.launcher.account
ui-launcher-account-sign_in = サインイン
ui-launcher-account-add_title = アカウントを追加
ui-launcher-account-again_title = 再度サインイン
ui-launcher-account-memory_only = このセッションのみサインインしています。保存されないため、再起動後に再度サインインが必要です。
ui-launcher-account-password_note = パスワードは Wargaming にのみ送信され、保存されることはありません。
ui-launcher-account-realm_fixed = 地域: { $realm }
ui-launcher-account-email = メールアドレス
ui-launcher-account-password = パスワード
ui-launcher-account-code = 2 段階認証コード
ui-launcher-account-code_required = 2 段階認証コードが必要です。
ui-launcher-account-use_backup_code = バックアップコードを使用
ui-launcher-account-submit_code = コードを送信
ui-launcher-account-captcha = このログインには CAPTCHA が必要ですが、このランチャーではまだ表示できません。公式の Wargaming Game Center またはブラウザーで wargaming.net に一度サインインしてから、再試行してください。

## ui.launcher.sign_in_error
ui-launcher-sign_in_error-wrong_credentials = メールアドレスまたはパスワードが正しくありません。
ui-launcher-sign_in_error-two_factor_required = このアカウントには二要素認証の承認が必要です。Wargaming Auth またはメールで届いたコードでサインインを承認してから、もう一度サインインしてください。
ui-launcher-sign_in_error-two_factor_invalid = 二要素認証コードが正しくないか、期限切れです。新しいコードを入力してください。
ui-launcher-sign_in_error-captcha_required = Wargaming が CAPTCHA を求めています。公式の Game Center または wargaming.net で一度サインインしてから、もう一度お試しください。
ui-launcher-sign_in_error-rate_limited = サインインの試行回数が多すぎます。数分待ってから、もう一度お試しください。
ui-launcher-sign_in_error-account_banned = このアカウントは BAN されています。
ui-launcher-sign_in_error-network_unreachable = Wargaming に接続できません。インターネット接続を確認してください。
ui-launcher-sign_in_error-service_unavailable = Wargaming のサインインサービスを利用できません。しばらくしてからもう一度お試しください。
ui-launcher-sign_in_error-session_expired = 保存済みのサインインの有効期限が切れました。もう一度サインインしてください。
ui-launcher-sign_in_error-keyring_locked = システムキーリングがロックされています。ロックを解除してから、もう一度サインインしてください。
ui-launcher-sign_in_error-not_saved = サインインしましたが、保存できませんでした。再起動後に再度サインインが必要です。
ui-launcher-sign_in_error-unexpected = サインインに失敗しました。詳細はログを確認してください。

## ui.launcher.realm
ui-launcher-realm-eu = ヨーロッパ
ui-launcher-realm-na = 北米
ui-launcher-realm-asia = アジア

## ui.launcher.runner
ui-launcher-runner-looking = Proton を探しています。
ui-launcher-runner-steam_proton = Steam の { $name }
ui-launcher-runner-umu = umu-run（{ $name }）
ui-launcher-runner-umu_default = umu-run（UMU-Proton）
ui-launcher-runner-native = ゲームはネイティブで実行されます。
ui-launcher-runner-first_run_prefix = 初回起動時に Wine プレフィックスを作成します。数秒かかります。
ui-launcher-runner-first_run_download = 初回起動時に UMU-Proton と Steam ランタイム（約 700 MB）をダウンロードします。
ui-launcher-runner-choose = Proton フォルダーを選択
ui-launcher-runner-automatic = 自動検出
ui-launcher-runner-detect = 再検索
ui-launcher-runner-needed = Linux で起動するには Proton が必要です。Proton 付きの Steam、または umu-launcher をインストールしてください。

## ui.replay.damage_share
ui-replay-damage_share-mode = 相手艦の合計に対するダメージの割合
ui-replay-damage_share-dealt = { $ship }が与えたダメージの{ $pct }は{ $other }に対するものです。
ui-replay-damage_share-dealt_inverse = { $other }が受けたダメージの{ $pct }は{ $ship }によるものです。
ui-replay-damage_share-received = { $ship }が受けたダメージの{ $pct }は{ $other }によるものです。
ui-replay-damage_share-received_inverse = { $other }が与えたダメージの{ $pct }は{ $ship }に対するものです。
ui-replay-damage_share-hold = { $key }を押し続けると反転します。
ui-replay-damage_share-release = { $key }を離すと元に戻ります。

## ui.reindex
ui-reindex-context = { $count } 件のリプレイを強制的に再インデックス化
ui-reindex-palette_by_version = リプレイを強制的に再インデックス化: バージョン別
ui-reindex-palette_all = リプレイを強制的に再インデックス化: すべて
ui-reindex-versions_title = リプレイを強制的に再インデックス化: バージョン別
ui-reindex-versions_placeholder = ゲームバージョンを絞り込み...
ui-reindex-version_row = { $version } -- { $count } 件のリプレイ
ui-reindex-no_versions = 一覧のリプレイにゲームバージョンを示すものがありません
ui-reindex-confirm_title = リプレイを強制的に再インデックス化
ui-reindex-confirm_ok = 再インデックス化
ui-reindex-confirm_count = { $count } 件のリプレイを再度読み込み、インデックスのエントリを書き直します。
ui-reindex-confirm_range = プレイ期間: { $oldest } から { $newest }。
ui-reindex-confirm_undated = 読み込む { $count } 件のリプレイには読み取り可能な戦闘日時がなく、この期間に含まれません。
ui-reindex-confirm_downloads = 次のバージョンのゲームデータを先にダウンロードします:
ui-reindex-confirm_unobtainable = 次のバージョンのゲームデータは取得できないため、{ $count } 件のリプレイをスキップします:
ui-reindex-confirm_no_cache_dir = ゲームデータのキャッシュディレクトリが設定されていないため、次のバージョンのゲームデータをダウンロードできず、{ $count } 件のリプレイをスキップします:
ui-reindex-confirm_no_build = { $count } 件のリプレイにはゲームビルドの記載がないため、スキップします。
ui-reindex-confirm_no_version = { $count } 件のリプレイには読み取り可能なゲームバージョンの記載がないため、スキップします。
ui-reindex-build_row = { $version } (ビルド { $build }): { $count } 件のリプレイ
ui-reindex-nothing_listed = 一覧に再インデックス化するリプレイがありません。
ui-reindex-nothing_readable = これらのリプレイはいずれも再インデックス化できません: ゲームデータがありません ({ $count } 件スキップ)。
ui-reindex-no_game_data = ゲームディレクトリを設定するまでリプレイを読み込めません。
ui-reindex-busy_index = インデックス処理はすでに実行中です。
ui-reindex-busy_cache = ゲームデータの処理がすでに実行中です。終了後に再度お試しください。
ui-reindex-planning = 利用可能なゲームデータを確認しています...
ui-reindex-no_index = リプレイのインデックスが開かれていないため、何も再インデックス化されませんでした。
ui-reindex-done = { $indexed } 件のリプレイを再インデックス化しました。{ $skipped } 件スキップ、{ $failed } 件は読み込めませんでした
ui-reindex-stopped = 再インデックス化を中止しました: { $indexed } 件再インデックス化、{ $skipped } 件スキップ、{ $failed } 件は読み込めませんでした
ui-reindex-stopped_before_reading = リプレイを読み込む前に再インデックス化を中止しました。

## ui.index_folder
ui-index_folder-tooltip = このリスト内で、リプレイインデックスにまだ含まれていないリプレイをすべてインデックス化します
ui-index_folder-confirm_title = リプレイをインデックス化
ui-index_folder-confirm_ok = インデックス化
ui-index_folder-confirm_count = このリスト内の { $count } 件のリプレイはまだリプレイインデックスに含まれていないため、読み込んで追加します。
ui-index_folder-note_live = インデックス化されたリプレイは、検索とプレイヤー追跡が読み込むリプレイインデックスに追加されます。
ui-index_folder-note_folder = インデックス化されたリプレイは、このディレクトリ独自のソース ({ $dir }) としてリプレイインデックスに追加されます。検索とプレイヤー追跡のプレイヤー一覧にはこれらが含まれます。プレイヤー追跡のクラン一覧はゲーム自体のリプレイのみを読み込みます。
ui-index_folder-nothing_listed = このリストにはインデックス化するリプレイがありません。
ui-index_folder-nothing_missing = このリスト内のリプレイはすべてインデックス化済みです。
ui-index_folder-unreadable_index = リプレイインデックスを読み込めませんでした。
ui-index_folder-done = { $indexed } 件のリプレイをインデックス化しました。{ $skipped } 件をスキップ、{ $failed } 件は読み込めませんでした
ui-index_folder-stopped = インデックス化を停止しました: { $indexed } 件インデックス化、{ $skipped } 件スキップ、{ $failed } 件読み込み不可
ui-index_folder-stopped_before_reading = リプレイを読み込む前にインデックス化が停止しました。

## ui.replay_folders
ui-replay_folders-install = ゲーム
ui-replay_folders-watched = 監視中: 新規および変更されたリプレイがここに表示されます
ui-replay_folders-unwatched = 未監視: 再読み込みするには更新を使用してください
ui-replay_folders-unseen = 前回の表示以降に { $count } 件の新規または変更されたリプレイがあります
ui-replay_folders-missing = このディレクトリは存在しません: { $dir }
ui-replay_folders-missing_hint = ピン留めを解除するまで、起動のたびにここで開かれます。
ui-replay_folders-pin = ピン留め
ui-replay_folders-unpin = ピン留めを解除
ui-replay_folders-refresh = 更新
ui-replay_folders-close = 閉じる
ui-replay_folders-pins_unreadable = ピン留めされたリプレイディレクトリを読み込めませんでした。
ui-replay_folders-game_data_available = 一覧のリプレイのゲームデータを利用できます。
ui-replay_folders-browse = 参照...
ui-replay_folders-recent_title = リプレイディレクトリを開く
ui-replay_folders-recent_placeholder = 最近のディレクトリを絞り込み...
ui-replay_folders-recent_missing = このディレクトリは存在しないため、最近の一覧から削除されました: { $dir }
ui-replay_folders-checking = { $dir } を確認しています...
ui-replay_folders-indexing = { $dir } をインデックス化中
ui-replay_folders-indexing_in_background = このディレクトリはまだインデックス化中です
ui-replay_folders-search_while_indexing = このディレクトリはまだインデックス化中のため、結果はまだ完全ではありません。完了後にもう一度検索してください。
ui-replay_folders-source_failed = このディレクトリのリプレイをインデックス化できません: { $error }
ui-replay_folders-restart_watch = 監視を再開
ui-replay_folders-watch_stopped = { $folder } の監視が停止しました: { $cause }。
ui-replay_folders-watch_restarting = { $seconds } 秒後に再開します (試行 { $attempt }/{ $attempts })。
ui-replay_folders-watch_gave_up = 再開に { $attempts } 回失敗したため、これ以上試行しません。
ui-replay_folders-watch_restart_hint = 赤い点をクリックするか、タブのメニューから「監視を再開」を選ぶと、今すぐ再開できます。
ui-replay_folders-watch_resumed = { $folder } の監視を再開しました。
ui-replay_folders-watch_cause_backend = ファイルシステムがエラーを報告しました: { $error }
ui-replay_folders-watch_cause_gone = フォルダーが移動または削除されました
ui-replay_folders-watch_cause_panicked = 内部エラー: { $error }
ui-replay_folders-watch_cause_exited = 監視が予期せず終了しました
ui-replay_folders-dropped = ドロップ
ui-replay_folders-dropped_numbered = ドロップ { $number }
ui-replay_folders-dropped_count = ドロップされたリプレイ { $count } 件
ui-replay_folders-dropped_no_source = ドロップされたリプレイはそれぞれのディレクトリと共にインデックスされます。検索やインデックスを行うにはディレクトリを開いてください。
ui-replay_folders-unreadable_replay = 読み込めませんでした: { $error }

## ui.models
ui-models-heading = モデル
ui-models-loading = モデルを読み込み中...
ui-models-unavailable = モデルを利用できません
ui-models-search = 艦艇とモデルを検索...
ui-models-open = 開く
ui-models-export = モデルをエクスポート
ui-models-copy_path = モデルパスをコピー
ui-models-path_copied = { $path } をコピーしました
ui-models-skin_replaces = { $exterior } の装備時に { $model } の代わりに使用されます
ui-models-geometry_absent = ゲームファイルにありません
ui-models-geometry_none = このモデルにはジオメトリがありません
ui-models-hardpoints_tooltip = 取り付け位置を表示
ui-models-common = 共通モデル

## ui.models.group
ui-models-group-gameplay = ゲームプレイ
ui-models-group-building = 建造物
ui-models-group-character = キャラクター
ui-models-group-location = ロケーション
ui-models-group-port = 港
ui-models-group-style = スタイル
ui-models-group-other = その他のモデル

## ui.models.building
ui-models-building-weapon = 建造物の兵装
ui-models-building-unassigned = 未割り当ての建造物
ui-models-building-unspecified = 種別のない建造物

## ui.models.misc
ui-models-misc-crane = クレーン
ui-models-misc-anchor = 錨
ui-models-misc-deck_fitting = 甲板艤装品
ui-models-misc-instrument = 計器
ui-models-misc-propeller = スクリュー
ui-models-misc-searchlight = 探照灯
ui-models-misc-boat = 艦載艇
ui-models-misc-flag = 旗
ui-models-misc-insignia = 徽章
ui-models-misc-railing = 手すり
ui-models-misc-hatch = ハッチ
ui-models-misc-ventilation = 通風装置
ui-models-misc-antenna = アンテナ
ui-models-misc-weapon = 兵装
ui-models-misc-stowage = 収納
ui-models-misc-other = その他

## ui.models.variant
ui-models-variant-dead = 残骸
ui-models-variant-skin = 外観

## ui.models.kind
ui-models-kind-ship = 艦艇
ui-models-kind-gun = 砲
ui-models-kind-director = 射撃指揮装置
ui-models-kind-finder = 測距儀
ui-models-kind-radar = レーダー
ui-models-kind-catapult = カタパルト
ui-models-kind-aircraft = 航空機
ui-models-kind-projectile = 弾薬
ui-models-kind-misc = その他の部品
ui-models-kind-cap = 占領ポイント
ui-models-kind-other = その他

## ui.models.sub_kind
ui-models-sub_kind-battleship = 戦艦
ui-models-sub_kind-cruiser = 巡洋艦
ui-models-sub_kind-destroyer = 駆逐艦
ui-models-sub_kind-aircraft_carrier = 航空母艦
ui-models-sub_kind-submarine = 潜水艦
ui-models-sub_kind-auxiliary = 補助艦艇
ui-models-sub_kind-main_battery = 主砲
ui-models-sub_kind-secondary_battery = 副砲
ui-models-sub_kind-anti_aircraft = 対空砲
ui-models-sub_kind-torpedo_tubes = 魚雷発射管
ui-models-sub_kind-depth_charge_launcher = 爆雷投射機
ui-models-sub_kind-missile_launcher = ミサイル発射機
ui-models-sub_kind-search_radar = 捜索レーダー
ui-models-sub_kind-fire_control_radar = 射撃管制レーダー
ui-models-sub_kind-fighter = 戦闘機
ui-models-sub_kind-bomber = 爆撃機
ui-models-sub_kind-dive_bomber = 急降下爆撃機
ui-models-sub_kind-scout = 偵察機
ui-models-sub_kind-torpedo = 魚雷
ui-models-sub_kind-bomb = 爆弾
ui-models-sub_kind-rocket = ロケット弾
ui-models-sub_kind-depth_charge = 爆雷
ui-models-sub_kind-mine = 機雷
ui-models-sub_kind-missile = ミサイル
ui-models-sub_kind-jato = 離陸補助ロケット
ui-models-sub_kind-shell = 砲弾

## ui.models.part
ui-models-part-hull = 船体
ui-models-part-main_battery = 主砲
ui-models-part-secondaries = 副砲
ui-models-part-anti_aircraft = 対空
ui-models-part-torpedoes = 魚雷
ui-models-part-directors = 射撃指揮装置
ui-models-part-finders = 測距儀
ui-models-part-radars = レーダー
ui-models-part-depth_charges = 爆雷
ui-models-part-aircraft = 航空機
ui-models-part-other = その他
ui-models-part-misc = その他の部品

## ui.model_view
ui-model_view-thickness_from_building = { $building } の装甲厚
ui-model_view-thickness_from = 装甲厚: { $ship } ({ $hp })
ui-model_view-thickness_unknown = 装甲厚不明: このモデルを搭載する艦艇はありません
ui-model_view-thickness_other_ships = 他の艦艇
ui-model_view-export_title = モデルをエクスポート
ui-model_view-export_tooltip = モデルを GLB ファイルにエクスポート
ui-model_view-part_model = モデル: { $model }
ui-model_view-part_hardpoint = 取り付け位置: { $hardpoint }

## ui.model_view.part_group
ui-model_view-part_group-hull = 船体
ui-model_view-part_group-main_battery = 主砲
ui-model_view-part_group-secondary_battery = 副砲
ui-model_view-part_group-aa_guns = 対空
ui-model_view-part_group-torpedoes = 魚雷
ui-model_view-part_group-depth_charges = 爆雷
ui-model_view-part_group-fire_control = 射撃管制
ui-model_view-part_group-radar = レーダー
ui-model_view-part_group-missiles = ミサイル発射機
ui-model_view-part_group-decorations = 装飾
ui-model_view-part_group-misc = その他の部品
ui-model_view-part_group-armor = 装甲
ui-model_view-part_group-other = その他

## ui.scene_problem
ui-scene_problem-cannot_show = { $name } を表示できません
ui-scene_problem-geometry_absent = このモデルのジオメトリはゲームファイルにありません。
ui-scene_problem-model_unreadable = このモデルのデータを読み込めませんでした。詳細はログにあります。
ui-scene_problem-ship_unreadable = この艦艇のデータを読み込めませんでした。詳細はログにあります。
ui-scene_problem-no_meshes = このモデルにはメッシュがありません。
ui-scene_problem-no_meshes_at_lod = 選択した詳細レベルではこのモデルにメッシュがありません。
ui-scene_problem-no_visible_surface = このモデルには描画できる可視面がありません。
ui-scene_problem-section_stand_in = この船体セクションには独自のジオメトリがありません。艦艇はこの部分の船体を別のセクションで描画します。

## ui.armor.option_tabs
ui-armor-option_tabs-label = オプションタブ
ui-armor-option_tabs-ship = 艦船
ui-armor-option_tabs-layers = レイヤー
ui-armor-option_tabs-analysis = 分析
ui-armor-option_tabs-configuration = 構成
ui-armor-option_tabs-hull_parts = 船体パーツ
ui-armor-option_tabs-tab_matches = { $tab }、{ $count } 件一致
ui-armor-option_tabs-empty = このタブには表示中のモデルに当てはまる項目がありません。
ui-armor-option_tabs-no_choices = このモデルには選択できる船体、モジュール、迷彩、詳細度がありません。

## ui.armor.hidden_ship
ui-armor-hidden_ship-clan_rental = クラン貸与
ui-armor-hidden_ship-other_class = 艦種ではありません

## ui.armor.textures
ui-armor-textures-title = テクスチャ
ui-armor-textures-hint = ゲームは、パッケージ内の同じパスのファイルの代わりに、ビルドの res_mods フォルダー内のファイルを読み込みます。テクスチャを右クリックするとそのパスをコピーできます。
ui-armor-textures-under_pointer = ポインター位置: { $material }
ui-armor-textures-reading = テクスチャを読み込み中...
ui-armor-textures-summary = マテリアル { $materials } 個、テクスチャ { $textures } 個
ui-armor-textures-no_material = マテリアルなし
ui-armor-textures-unresolved = パスが assets.bin にありません
ui-armor-textures-absent = インストール済みパッケージにありません
ui-armor-textures-unreadable = DDS ヘッダーを読み込めませんでした
ui-armor-textures-unknown_format = 不明な形式
ui-armor-textures-details = { $format }、{ $size }、{ $count } ファイル、{ $bytes }
ui-armor-textures-role_albedo = アルベド
ui-armor-textures-role_normal = 法線
ui-armor-textures-role_metal_gloss = メタル/グロス
ui-armor-textures-role_ambient_occlusion = アンビエントオクルージョン
ui-armor-textures-role_detail = ディテール
ui-armor-textures-role_name_matched = アルベド (ファイル名で検出)
ui-armor-textures-copy_path = パスをコピー
ui-armor-textures-copy_overrides = { $count } 件の res_mods パスをコピー
ui-armor-textures-paths_copied = { $count } 件のパスをコピーしました
ui-armor-textures-no_build = ゲームデータが読み込まれていないため、ビルドの mod フォルダーが不明です。
ui-armor-textures-show_in_unpacker = リソース展開で表示
ui-armor-textures-show_textures = テクスチャを表示
ui-armor-textures-extract = ファイルを展開...
ui-armor-textures-extract_title = テクスチャファイルの展開先
ui-armor-textures-extracting = テクスチャファイルを展開中...
ui-armor-textures-extracted = { $count } 個のファイルを { $folder } に展開しました
ui-armor-textures-extract_failed = テクスチャファイルを展開できませんでした: { $reason }
ui-armor-textures-replace_title = 既存のファイルを置き換えますか?
ui-armor-textures-replace_description = これらのファイルのうち { $count } 個は既に { $folder } にあります。展開すると置き換えられます。
ui-armor-textures-replace = 置き換える

## ui.settings.mcp
ui-settings-mcp-enabled = MCPサーバーを有効にする
ui-settings-mcp-enabled_description = MCP経由でリプレイとゲームデータをAIアシスタントに提供します。ステータスバーに操作ボタンを表示します。
ui-settings-mcp-start_on_launch = プログラムと同時に起動
ui-settings-mcp-start_on_launch_description = WoWs Toolkitを開いたときにサーバーを起動します。
ui-settings-mcp-bind_address = 待ち受けアドレス
ui-settings-mcp-bind_address_description = サーバーが待ち受けるループバック IP アドレス (127.0.0.1 や ::1 など)。このコンピューターからのみ接続できます。
ui-settings-mcp-port = ポート
ui-settings-mcp-port_description = 待ち受けるTCPポート。最初に有効にしたときにランダムに選ばれ、保持されます。
ui-settings-mcp-port_unset = 最初に有効にしたときに選択
ui-settings-mcp-address_not_loopback = { $input } はループバックアドレスではありません。サーバーはこのコンピューター上でのみ待ち受けます (例: 127.0.0.1)。
ui-settings-mcp-address_invalid = { $input } はIPアドレスではありません
ui-settings-mcp-port_invalid = { $input } は1から65535のポートではありません
ui-settings-mcp-port_in_use = このポートは別のプログラムが使用しています。別のポートを選んでください。

## ui.mcp
ui-mcp-started = MCPサーバーが { $url } で動作中
ui-mcp-copy_url = URLをコピー
ui-mcp-start_failed = MCPサーバーを起動できません: { $reason }
ui-mcp-port_in_use = { $address } のポート { $port } は別のプログラムが使用中です
ui-mcp-no_watchdog = バックグラウンド処理の監視が動作していません
ui-mcp-status_running = MCPサーバーが { $url } で動作中。クリックで停止。
ui-mcp-status_starting = MCPサーバーを { $address } で起動中
ui-mcp-status_failed = MCPサーバーを起動できませんでした。クリックで再試行。
ui-mcp-status_stopped = MCPサーバーは停止中。クリックで起動。

## ui.launcher.opt_in
ui-launcher-opt_in-setting = ゲームランチャーを有効化
ui-launcher-opt_in-description = Linux 上のツールキットから、Wargaming アカウントでサインインして World of Warships をプレイします。
ui-launcher-opt_in-title = ゲームランチャーを有効にしますか?
ui-launcher-opt_in-notice_account = ランチャー機能を使用することにより、この機能がゲーム開発元によってサポートされている利用形態ではないことを了承したものとみなされます。あなたはこのアプリケーションに直接パスワードを入力することになります。このアプリケーションを信頼していない場合、アカウントを盗むために使われる可能性があります (ただし、アプリケーションの開発者はそのようなことを望んでいません)。
ui-launcher-opt_in-notice_reports = さらに、この機能を使用する場合、Linux「のみ」に影響するのではないクロスプラットフォームの問題であると本当に確信している場合を除き、グラフィックや安定性に関するバグをゲーム開発元に報告しないことを了承したものとみなされます。
ui-launcher-opt_in-continue = 続行
ui-launcher-opt_in-cancel = キャンセル

## ui.launcher.keyring
ui-launcher-keyring-unencrypted = Secret Service プロバイダー (GNOME Keyring、KeePassXC、KDE Wallet など) が利用できないため、サインインはアプリのデータフォルダーに暗号化されずに保存されます。
ui-launcher-keyring-how = 暗号化して保存する方法
ui-launcher-keyring-how_detail = GNOME Keyring、KeePassXC、KDE Wallet などの Secret Service プロバイダーをインストールして起動し、ツールキットを再起動してください。保存済みのサインインはそちらに移されます。
ui-launcher-keyring-unlock_hint = ツールキットが保存済みのサインインを読み取るために、システムキーリングがロック解除を求めることがあります。

## ui.launcher.play
ui-launcher-play-play = プレイ
ui-launcher-play-play_through_steam = Steam 経由でプレイ
ui-launcher-play-set_up_game = ゲームをセットアップ
ui-launcher-play-set_up_runner = Proton をセットアップ
ui-launcher-play-sign_in_again = 再度サインイン
ui-launcher-play-update = アップデート { $size }
ui-launcher-play-updating = アップデート中 { $percent }%
ui-launcher-play-updating_unknown = アップデート中
ui-launcher-play-paused = 一時停止中
ui-launcher-play-starting = 起動中...
ui-launcher-play-running = 実行中
ui-launcher-play-in_battle = 戦闘中
ui-launcher-play-menu = アカウントとゲームのオプション

## ui.launcher.popover
ui-launcher-popover-game = World of Warships
ui-launcher-popover-no_install = ゲームのインストールが設定されていません。
ui-launcher-popover-update_available = アップデートあり
ui-launcher-popover-check_failed = アップデートを確認できませんでした
ui-launcher-popover-accounts = アカウント
ui-launcher-popover-no_accounts = アカウントがまだありません。追加するとサインインしてプレイできます。
ui-launcher-popover-close_to_switch = アカウントを切り替えるにはゲームを終了してください。
ui-launcher-popover-default = デフォルト
ui-launcher-popover-ready = 準備完了
ui-launcher-popover-restoring = 確認中...
ui-launcher-popover-not_saved = 未保存
ui-launcher-popover-unencrypted = 暗号化せずに保存
ui-launcher-popover-keyring_locked = キーリングがロックされています: 保存済みのサインインを復元するにはロックを解除してください
ui-launcher-popover-forget = 削除
ui-launcher-popover-sign_out = サインアウト
ui-launcher-popover-expires_in = 残り { $duration } で期限切れ
ui-launcher-popover-expired = セッションの期限切れ
ui-launcher-popover-expiry_unknown = 有効期限不明
ui-launcher-popover-make_default = デフォルトに設定
ui-launcher-popover-account_actions = アカウント操作
ui-launcher-popover-play_anyway = このままプレイ
ui-launcher-popover-play_safe_mode = セーフモードでプレイ
ui-launcher-popover-add_account = アカウントを追加...
ui-launcher-popover-game_settings = ゲーム設定...
ui-launcher-popover-installs = インストールとセットアップ...
ui-launcher-popover-open_replay = リプレイをゲームで開く...

## ui.launcher.palette
ui-launcher-palette-play = プレイ
ui-launcher-palette-play_as = { $nickname } ({ $realm }) としてプレイ
ui-launcher-palette-game_settings = ゲーム設定
ui-launcher-palette-add_account = アカウントを追加
ui-launcher-palette-update = World of Warships をアップデート
ui-launcher-palette-open_replay = リプレイをゲームで開く

## ui.launcher.installs
ui-launcher-installs-toolkit = ツールキットのゲームディレクトリ
ui-launcher-installs-played = デフォルトアカウントがこれでプレイ
ui-launcher-installs-play_from = ここからプレイ
ui-launcher-installs-update = アップデート

## ui.launcher.wgc_login
ui-launcher-wgc_login-label = Wargaming ログインを使用
ui-launcher-wgc_login-about = この Steam インストールを、Steam ではなくツールキットのサインインで起動します。設定とリプレイは Steam での起動とは別に保持されます。このモードでは Steam ウォレット、実績、プレイ時間は機能しません。
ui-launcher-wgc_login-steam_accounts = Steam で作成したアカウントにはメールアドレスとパスワードが必要です。先に Wargaming のアカウント管理で追加してください。
ui-launcher-wgc_login-steam_accounts_link = メールアドレスとパスワードを追加する方法
ui-launcher-wgc_login-ready = Game Center ファイルの準備完了
ui-launcher-wgc_login-fetchable = オンにすると、このビルド用の WG の Game Center ファイル ({ $size }) をダウンロードします。
ui-launcher-wgc_login-downloading = Game Center ファイルをダウンロード中: { $percent }%
ui-launcher-wgc_login-downloading_unknown = Game Center ファイルをダウンロード中
ui-launcher-wgc_login-checking = このビルドの Game Center ファイルを探しています...
ui-launcher-wgc_login-needs_sign_in = 先にツールキットに Wargaming アカウントを追加してください。
ui-launcher-wgc_login-unavailable = このビルドでは利用できません: { $error }

## ui.launcher.replay
ui-launcher-replay-no_install = ゲームビルド { $build } のインストールがありません。このリプレイを視聴するには、そのビルドのコピーを保持するかインストールしてください。

## ui.launcher.setup
ui-launcher-setup-title = World of Warships のセットアップ
ui-launcher-setup-game = ゲーム
ui-launcher-setup-proton = Proton
ui-launcher-setup-accounts = アカウント
ui-launcher-setup-replays = リプレイ
ui-launcher-setup-game_question = ツールキットで使用するゲームのコピーはどれですか?
ui-launcher-setup-none_found = このコンピューターにインストールが見つかりませんでした。
ui-launcher-setup-choose_folder = フォルダーを選択...
ui-launcher-setup-install_new = 新しいコピーをインストール...
ui-launcher-setup-proton_explain = Linux ではゲームは Proton を通じて実行されます。Steam がインストールされていれば Steam の Proton、そうでなければ umu-run を使用します。
ui-launcher-setup-proton_ready = Proton が見つかりました。
ui-launcher-setup-accounts_explain = プレイする Wargaming アカウントにサインインしてください。プレイを押すと、デフォルトアカウントでゲームにサインインします。
ui-launcher-setup-replays_explain = リプレイがこの数に達すると、ゲームは最も古いリプレイを削除します。
ui-launcher-setup-skip = セットアップをスキップ
ui-launcher-setup-back = 戻る
ui-launcher-setup-next = 次へ
ui-launcher-setup-done = 完了

## ui.launcher.game_settings
ui-launcher-game_settings-title = ゲーム設定
ui-launcher-game_settings-build = ビルド { $build }
ui-launcher-game_settings-display = ディスプレイ
ui-launcher-game_settings-graphics = グラフィック
ui-launcher-game_settings-sound = サウンド
ui-launcher-game_settings-replays = リプレイ
ui-launcher-game_settings-locked = ゲームが実行中です。終了時にゲームがこれらのファイルを書き換えるため、それまでは読み取り専用です。
ui-launcher-game_settings-footer = ゲームの設定ファイルに書き込まれます。バックアップが保持されます。
ui-launcher-game_settings-cancel = キャンセル
ui-launcher-game_settings-save = 保存
ui-launcher-game_settings-saved = ゲーム設定を保存しました。
ui-launcher-game_settings-no_install = ゲームのインストールが設定されていません。
ui-launcher-game_settings-window_mode = ウィンドウモード
ui-launcher-game_settings-windowed = ウィンドウ
ui-launcher-game_settings-borderless = ボーダーレス
ui-launcher-game_settings-fullscreen = フルスクリーン
ui-launcher-game_settings-monitor = モニター
ui-launcher-game_settings-monitor_name = モニター名
ui-launcher-game_settings-resolution = フルスクリーン解像度
ui-launcher-game_settings-width = 幅
ui-launcher-game_settings-height = 高さ
ui-launcher-game_settings-refresh = リフレッシュレート
ui-launcher-game_settings-display_rate = 表示レート
ui-launcher-game_settings-vsync = 垂直同期
ui-launcher-game_settings-ui_scale = インターフェースの拡大率
ui-launcher-game_settings-battle_fps = 戦闘中のフレームレート上限
ui-launcher-game_settings-port_fps = 港でのフレームレート上限
ui-launcher-game_settings-unlimited = 無制限
ui-launcher-game_settings-skip_intro = イントロ動画をスキップ
ui-launcher-game_settings-master = マスター音量
ui-launcher-game_settings-music_port = 港の音楽
ui-launcher-game_settings-music_battle = 戦闘中の音楽
ui-launcher-game_settings-preset = プリセット
ui-launcher-game_settings-antialiasing = アンチエイリアス
ui-launcher-game_settings-textures = テクスチャ品質
ui-launcher-game_settings-filtering = テクスチャフィルタリング
ui-launcher-game_settings-shadows = 影の品質
ui-launcher-game_settings-graphics_note = グラフィックはゲーム内で変更してください。ゲームがハードウェアに合わせて確認します。
ui-launcher-game_settings-replays_kept = 保持するリプレイ数
ui-launcher-game_settings-replays_kept_zero = 少なくとも 1 件のリプレイを保持してください。
ui-launcher-game_settings-versioned = ゲームバージョンごとにリプレイを分類
ui-launcher-game_settings-replays_note = このビルドの res_mods に書き込まれます。セーフモードでは使用されません。
ui-launcher-game_settings-absent = ゲームのファイルで未設定
ui-launcher-game_settings-unrecognized = 不明な値: { $value }
ui-launcher-game_settings-not_a_number = { $field }: 数値を入力してください。

## ui.settings.launcher
ui-settings-launcher-installs = ゲームのインストール
ui-settings-launcher-installs_description = ランチャーがプレイおよびアップデートするゲームのインストールです。
ui-settings-launcher-runner = Proton (Linux)
ui-settings-launcher-runner_description = Linux でのゲームの実行方法。Steam の Proton Experimental または最新の Proton を自動的に検出し、なければ umu-run を使用します。
ui-settings-launcher-launch_args = 起動引数
ui-settings-launcher-launch_args_description = ゲームの追加コマンドライン引数 (1 行に 1 つ)。
ui-settings-launcher-environment = 環境
ui-settings-launcher-environment_description = ゲームの環境変数 (KEY=VALUE、1 行に 1 つ)。
ui-settings-launcher-safe_mode_args = セーフモードの引数
ui-settings-launcher-safe_mode_args_description = セーフモードでゲームをプレイするときに追加される引数 (1 行に 1 つ)。
