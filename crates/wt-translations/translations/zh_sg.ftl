# You are encouraged to suggest edits to this file on GitHub by either filing
# an issue, editing the file directly and creating a pull request, or just
# mentioning it in the Discord.
## meta
meta-language_name = 简体中文

# -- 语言选择对话框 --

## dialog
dialog-continue_in_english = 继续使用英文
dialog-continue_in_language = 继续使用简体中文
dialog-select_language = 选择语言
dialog-machine_translation_warning = 大部分翻译由机器生成，可能不准确。

# -- 标签页标题 --

## ui.tabs
ui-tabs-unpacker = 资源解包器
ui-tabs-settings = 设置
ui-tabs-replay_parser = 回放检查器
ui-tabs-player_tracker = 玩家追踪器
ui-tabs-armor_viewer = 装甲查看器
ui-tabs-model_viewer = 模型查看器
ui-tabs-stats = 统计
ui-tabs-directory_stats = { $dir } 统计
ui-tabs-close_directory_stats = 关闭目录统计
ui-tabs-search = 搜索
ui-tabs-search_these_replays_unavailable = 这些回放尚未建立索引
ui-tabs-zoom_in_panel = 放大
ui-tabs-zoom_out_panel = 缩小
ui-tabs-close_panel = 关闭
ui-tabs-close_tab = 关闭
ui-tabs-close_other_tabs = 关闭其他
ui-tabs-close_tabs_to_the_right = 关闭右侧

# -- 通用按钮 --

## ui.buttons
ui-buttons-yes = 是
ui-buttons-no = 否
ui-buttons-send_replays = 发送回放
ui-buttons-send_build_data = 仅发送配置数据
ui-buttons-send_nothing = 不发送任何内容
ui-buttons-switch_to_replays = 切换为发送回放
ui-buttons-keep_current = 保持当前设置
ui-buttons-stop = 停止
ui-buttons-copy = 复制
ui-buttons-cancel = 取消
ui-buttons-continue_ = 继续
ui-buttons-install_update = 安装更新
ui-buttons-view_release = 查看版本
ui-buttons-clear_settings = 清除设置
ui-buttons-more_options = 更多选项
ui-buttons-create_issue = 创建 Issue
ui-buttons-discord = Discord
ui-buttons-github = GitHub
ui-buttons-view_github = 在 GitHub 上查看项目
ui-buttons-choose = 选择...
ui-buttons-reset = 重置
ui-buttons-select_all = 全选
ui-buttons-download = 下载
ui-buttons-retry = 重试
ui-buttons-clear = 清除
ui-buttons-refresh_persisted_data_confirm = 刷新

# -- 菜单项 --

## ui.menu
ui-menu-file = 文件
ui-menu-check_updates = 检查更新
ui-menu-about = 关于
ui-menu-quit = 退出

# -- 窗口标题 --

## ui.windows
ui-windows-build_consent = 配置收集同意
ui-windows-replay_migration = 切换为共享回放
ui-windows-crash_detected = 检测到应用程序崩溃
ui-windows-update_available = 有可用更新
ui-windows-network_warning = 网络警告
ui-windows-download_game_data = 下载游戏数据
ui-windows-error = 错误
ui-windows-software_encode = 没有 GPU 视频编码器
ui-windows-about = 关于
ui-windows-refresh_persisted_data = 刷新已保存的回放数据
ui-windows-tactics_board = 战术板
ui-windows-minimize = 最小化
ui-windows-maximize = 最大化
ui-windows-restore = 还原
ui-windows-close = 关闭

# -- 对话框消息 --

## ui.dialogs
ui-dialogs-build_consent_message = 您是否愿意将排位赛和随机战斗中的玩家配置信息发送给开发者？此数据收集有助于社区批量分析玩家配置。您可以随时在设置中选择退出。
ui-dialogs-replay_migration_message = Toolkit 现在可以共享完整回放，这比仅发送配置数据能实现更丰富的对战分析。您使用测试舰船的战斗不会被共享。您是否愿意切换为共享原始回放？
ui-dialogs-crash_message = WoWs Toolkit 在上次运行时似乎崩溃了。如果您想报告此问题，请在 GitHub 上提交 Issue 或加入 Discord 服务器并提供以下信息。
ui-dialogs-crash_clear_settings = 如果应用程序持久化的数据可能是问题原因，您可以尝试按下方按钮清除设置。这将清除所有设置。追踪的玩家会保留。您的回放和 WoWs 数据将保持安全。
ui-dialogs-update_message = WoWs Toolkit 版本 { $tag } 可用
ui-dialogs-update_notes = 发行说明
ui-dialogs-update_asset_pending = Windows 版下载尚不可用。请查看版本页面了解构建状态。
ui-dialogs-update_windows_only = 有可用更新，但目前仅支持 Windows。
ui-dialogs-p2p_warning = 协作会话使用 P2P 网络。会话中的其他用户可能能够看到您的 IP 地址。
ui-dialogs-software_encode_message = 未找到受支持的 GPU 视频编码器，因此本次导出将使用软件编码。速度会慢很多，较长的战斗可能需要几分钟。
ui-dialogs-download_game_data_intro = 部分回放需要尚未安装的游戏数据。请选择要下载的构建。
ui-dialogs-download_build_row = 版本 { $version }（构建 { $build }）
ui-dialogs-download_replays_needing = { $count } 个回放需要此版本
ui-dialogs-download_availability_exact = 已发布
ui-dialogs-download_availability_nearest = 无完全匹配，最接近的是 { $version }（构建 { $build }）
ui-dialogs-download_availability_unpublished = 从未发布
ui-dialogs-download_availability_unreachable = 已发布，但无法读取其文件列表
ui-dialogs-download_plan_pending = 正在确定需要获取的内容...
ui-dialogs-download_objects_to_fetch = 已勾选构建需获取的对象数：{ $count }
ui-dialogs-download_plan_failed = 无法确定需要获取的内容
ui-dialogs-download_plan_no_cache_dir = 尚未设置游戏数据缓存目录。请在设置中选择一个目录后重试。
ui-dialogs-refresh_persisted_data_message = 重新解析回放目录中的每个回放并重写其保存的数据。使用与其游戏构建不匹配的回放数据映射记录的条目将保持不变，但所有回放的个人评分和游戏模式都会按当前的期望值重新计算。这可能需要一些时间。
ui-dialogs-file_dialog_failed = 无法打开文件对话框：{ $reason }

# -- 标签 --

## ui.labels
ui-labels-error_occurred = 发生错误：
ui-labels-made_by = 由 landaire 制作。
ui-labels-credits = 感谢 Trackpad、TTaro、lkolbly 的贡献。
ui-labels-pr_credits = 个人评分 (PR) 计算数据和公式由 WoWs Numbers 提供。
ui-labels-suppress_warning = 不再显示
ui-labels-more_info = 更多信息
ui-labels-app_version = 版本 { $version }
ui-labels-project_license = WoWs Toolkit 依据 { $license } 许可证分发。
ui-labels-third_party_notices = 第三方声明
ui-labels-notices_search_hint = 搜索声明
ui-labels-no_matching_notices = 没有与搜索匹配的声明。

# -- 提示消息 --

## ui.messages
ui-messages-bug_report_info_copied = 已复制错误报告信息。
ui-messages-update_downloading = 正在下载更新：{ $read } / { $total }
ui-messages-update_downloading_unknown = 正在下载更新：已下载 { $read }
ui-messages-app_up_to_date = 应用程序已是最新版本
ui-messages-checking_app_updates = 正在检查应用更新...
ui-messages-update_starting = 正在下载更新...
ui-messages-update_check_failed = 检查应用更新失败
ui-messages-game_data_loaded = 成功加载游戏数据
ui-messages-no_replays_detected = 未检测到回放。您的 WoWs 目录是否配置正确？
ui-messages-game_data_builds_downloaded = 已下载 { $count } 个构建的游戏数据
ui-messages-game_data_download_failed = 无法下载游戏数据
ui-messages-game_data_up_to_date = 游戏数据已是最新
ui-messages-game_data_updates_available = { $count } 个已缓存构建有更新的数据可用
ui-messages-game_data_cache_valid = 游戏数据缓存有效，且与远程仓库一致
ui-messages-game_data_cache_invalid = { $count } 个已缓存构建已过期或已损坏
ui-messages-replay_load_failed = 加载回放失败
ui-messages-twitch_token_invalid = Twitch 令牌无效或已过期。请在设置中更新。
ui-messages-wows_dir_invalid = World of Warships 目录无效。请检查设置中的路径。
ui-messages-wows_dir_not_set = 尚未设置 World of Warships 目录
ui-messages-constants_version_mismatch =
    回放数据映射文件版本与游戏版本不匹配。
    战后结果可能不准确。请耐心等待项目维护者更新服务器上的映射。
ui-messages-session_started = 会话已开始
ui-messages-user_joined = { $name } 已加入
ui-messages-user_timeout = { $name } 超时
ui-messages-user_left = { $name } 已离开
ui-messages-session_ended = 会话已结束
ui-messages-session_error = 会话错误：{ $msg }
ui-messages-connected_to_session = 已连接到会话
ui-messages-host_opened_replay = 主机打开了回放：{ $name }
ui-messages-host_closed_replay = 主机关闭了回放
ui-messages-session_rejected = 会话被拒绝：{ $reason }
ui-messages-drop_not_a_replay = 这不是回放文件
ui-messages-drop_not_replays = 这些不是回放文件
ui-messages-drop_skipped = 已跳过 { $count } 个非回放或文件夹的拖入文件
ui-messages-log_copied = 已将 { $file } 中的 { $bytes } KB 复制到剪贴板
ui-messages-log_copy_failed = 无法读取日志文件：{ $error }
ui-messages-log_not_found = 在 { $dir } 中未找到日志文件

# -- 统计名称 --

## stat
stat-damage = 伤害
stat-spotting_damage = 侦察伤害
stat-frags = 击沉
stat-raw_xp = Raw XP
stat-base_xp = Base XP
stat-win_rate = 胜率
stat-personal_rating = 个人评分
stat-match_contribution = 战斗贡献
stat-capture_points = 占领点数
stat-planes_shot_down = 击落飞机
stat-anti_air_damage = 防空伤害
stat-survival_rate = 存活率
stat-torpedoes_spotted = 发现鱼雷
stat-avg_pr = 平均 PR
stat-avg_prefix = 平均 { $name }

## chart
chart-combined = 综合
chart-combined_suffix = （综合）
chart-rolling_average_suffix = （滑动平均）

# -- 确认对话框 --

## confirm
confirm-open_in_game = 这将启动 World of Warships。继续吗？
confirm-clear_all_session_stats = 这将清除所有会话统计。继续吗？
confirm-set_as_session_stats = 这将替换您当前的会话统计。继续吗？

# -- 错误消息 --

## error
error-file_watcher_creation = 创建文件监视器失败：{ $error }
error-replay_dir_watch = 监视回放目录失败：{ $error }

# -- 设置 --

## ui.settings.app
ui-settings-app-check_for_updates = 启动时检查更新
ui-settings-app-enable_logging = 启用日志记录（重启后生效）
ui-settings-app-enable_logging_tooltip = 将调试日志写入应用程序数据目录中的文件。有助于故障排除。
ui-settings-app-data_sharing_mode = 数据共享（排位赛和随机战斗）
ui-settings-app-theme = 主题
ui-settings-app-theme_system = 跟随系统
ui-settings-app-theme_dark = 深色
ui-settings-app-theme_light = 浅色
ui-settings-app-code_integrity = 阻止未签名代码（重启后生效）
ui-settings-app-code_integrity_tooltip = 拒绝加载未经 Microsoft 签名的代码，从而将叠加层、录制工具和音频工具挡在本应用程序之外。这类工具是渲染时崩溃和卡死的常见原因。显卡驱动程序已通过 Windows 签名，可继续正常工作。本次会话启动后便无法再关闭此项，因此更改将在下次启动时生效。选择自动时，若您已安装第三方输入法（否则会被阻止），则会跳过此项。请注意，无论此设置如何，其他保护措施都会保持启用；如需全部关闭，请使用 --no-hardening 运行。
ui-settings-app-code_integrity_automatic = 自动
ui-settings-app-code_integrity_always = 始终
ui-settings-app-code_integrity_never = 从不
ui-settings-app-language = 语言
ui-settings-app-open_data_dir_tooltip = 在文件管理器中打开应用数据文件夹（设置、日志、缓存数据）。
ui-settings-app-proxy_url = HTTP 代理
ui-settings-app-proxy_url_hint_live = host:port，留空则使用系统代理设置

## ui.settings.wows
ui-settings-wows-description = 您的 World of Warships 安装路径
ui-settings-wows-directory_hint = World of Warships 目录
ui-settings-wows-browse = 浏览...

## ui.settings.wows.cache
ui-settings-wows-cache-heading = 游戏数据缓存
ui-settings-wows-cache-description = 已转储的游戏数据，使旧构建的回放仍可打开
ui-settings-wows-cache-auto_dump = 缓存游戏数据以兼容旧回放
ui-settings-wows-cache-browse = 浏览
ui-settings-wows-cache-open_folder = 打开文件夹
ui-settings-wows-cache-delete_old = 删除旧版本
ui-settings-wows-cache-stats = 游戏数据缓存：{ $count }个版本共{ $size }
ui-settings-wows-cache-check_updates = 检查更新
ui-settings-wows-cache-actions = 缓存操作
ui-settings-wows-cache-actions_tooltip = 检查更新或校验已缓存的构建
ui-settings-wows-cache-update_all = 全部更新
ui-settings-wows-cache-updates_available = { $count } 个已缓存构建有更新的数据可用
ui-settings-wows-cache-validate = 校验缓存
ui-settings-wows-cache-repair = 修复
ui-settings-wows-cache-repair_needed = { $count } 个已缓存构建已过期或已损坏

## ui.settings.replay
ui-settings-replay-show_raw_xp = 显示 Raw XP
ui-settings-replay-show_entity_id = 显示 Entity ID
ui-settings-replay-show_observed_damage = 显示观察伤害
ui-settings-replay-enable_previews = 悬停时显示小地图预览
ui-settings-replay-enable_previews_tooltip = 悬停回放时会在后台读取并重放该回放，以生成循环播放的小地图预览。若存储速度较慢，请关闭此项。
ui-settings-replay-auto_export_data = 自动导出数据
ui-settings-replay-export_path_hint = 数据自动导出目录
ui-settings-replay-export_path_missing = 该目录不存在，无法导出数据。
ui-settings-replay-index_all_replays = 为所有回放建立索引
ui-settings-replay-show_heals = 显示维修
ui-settings-replay-current_path = 当前回放路径

## ui.settings.session
ui-settings-session-display_name = 显示名称
ui-settings-session-suppress_ip_warning = 不显示 P2P IP 地址警告
ui-settings-session-ip_warning_tooltip = 协作会话使用 P2P 网络。会话中的其他用户可能能够看到您的 IP 地址。
ui-settings-session-disable_auto_open = 禁用自动打开会话窗口
ui-settings-session-auto_open_tooltip = 启用后，会话窗口（回放、战术板）不会自动打开。您仍可从会话弹出窗口中打开它们。

## ui.settings.twitch
ui-settings-twitch-get_token = 获取登录令牌
ui-settings-twitch-get_token_tooltip = 我们使用 Chatterino 的登录页面，因为它提供具有必要权限的令牌（基本上是具有聊天权限的管理员令牌），且无需 WoWs Toolkit 开发者托管自己的登录页面网站来获得相同结果。
ui-settings-twitch-paste_token_no_token = 粘贴令牌（未设置令牌）
ui-settings-twitch-paste_token_tooltip = 从剪贴板读取凭据
ui-settings-twitch-paste_token_valid = 粘贴令牌（令牌有效）
ui-settings-twitch-paste_token_unvalidated = 正在检查令牌
ui-settings-twitch-paste_token_invalid = 粘贴令牌（令牌无效 / 已过期）
ui-settings-twitch-monitored_channel = 监控频道（默认为自己）
ui-settings-twitch-signed_in_as = 已登录为 { $who }

# -- 回放检查器 --

## ui.replay
ui-replay-copy_bug_report_info = 复制错误报告信息
ui-replay-match_age = { $age }前
ui-replay-listing_caption = 回放
ui-replay-loading_game_data_version = 正在加载 { $version } 的游戏数据...
ui-replay-spectator = 观战
ui-replay-loading = 加载中...
ui-replay-preview_loading = 正在生成预览...
ui-replay-preview_no_game_data = 没有客户端版本 { $value } 的游戏数据
ui-replay-preview_unreadable_because = 无法读取此回放: { $value }
ui-replay-preview_unwalkable = 无法遍历此回放的战斗：{ $value }
ui-replay-preview_unknown_version = 此回放报告的客户端版本无法识别：{ $value }
ui-replay-preview_no_map = 没有 { $value } 的地图数据
ui-replay-preview_render_failed = 无法绘制预览：{ $value }
ui-replay-incomplete_results = 不完整的比赛结果
ui-replay-incomplete_results_tooltip = 回放尚无比赛结束结果。当比赛结束并将结束结果添加到回放后，数据将自动重新加载。
ui-replay-no_chat = 此回放中没有发送聊天消息
ui-replay-chat_copy_all = 复制全部
ui-replay-chat_save_to_file = 保存到文件
ui-replay-chat_save_title = 保存游戏聊天
ui-replay-export_results_title = 导出结果
ui-replay-results_exported = 结果已导出
ui-replay-chat_saved = 聊天已保存到 { $path }
ui-replay-chat_save_failed = 聊天未能保存：{ $error }
ui-replay-team_damage = 团队伤害：
ui-replay-team_share = 占团队总计的 { $pct }（{ $team }）
ui-replay-hide_my_stats = 隐藏我的测试舰船统计
ui-replay-open_manually = 手动打开回放文件...
ui-replay-open_directory = 打开回放目录...
ui-replay-search_these_replays = 搜索这些回放
ui-replay-import_constants = 导入 constants.json 文件...
ui-replay-refresh_persisted_data = 刷新已保存的回放数据...
ui-replay-contributing = 正在提交第 { $done } / { $total } 个回放
ui-replay-contributed = 已提交 { $sent } / { $total } 个回放
ui-replay-contribute_nothing_shared = 请先在设置中选择要共享的内容
ui-replay-contribute_none_listed = 没有可提交的回放
ui-replay-contribute_not_ready = 游戏数据和设置加载完成后即可提交回放
ui-replay-contribute_already_running = 提交任务已在运行
ui-replay-auto_export_failed = 无法导出一场战斗：{ $error }
ui-replay-auto_exported = 已将战斗导出到 { $file }
ui-replay-opened_new_battle = 已打开新战斗：{ $battle }
ui-replay-new_battle_listed = 新战斗：{ $battle }
ui-replay-results_arrived = { $battle } 的战斗结果已送达
ui-replay-constants_written = 已下载 { $count } 个构建的结果映射
ui-replay-constants_import_title = 导入 constants.json
ui-replay-constants_imported = 已导入构建 { $build } 的结果映射
ui-replay-constants_import_failed = 未能导入该映射：{ $reason }
ui-replay-constants_no_build = 请先加载一个游戏构建
ui-replay-constants_latest_written = 已下载最新的结果映射
ui-replay-constants_check_failed = 无法检查结果映射：{ $reason }
ui-replay-constants_build_label = 版本 { $build }
ui-replay-constants_generating = 正在更新 { $version } 的游戏常量...
ui-replay-constants_generated = { $version } 的游戏常量已生成，用时 { $seconds } 秒
ui-replay-constants_generation_failed = 无法更新 { $version } 的游戏常量：{ $reason }。{ $fallback }
ui-replay-constants_fallback_kept = 保留已为其保存的常量。
ui-replay-constants_fallback_published = 使用为版本 { $build } 发布的常量。
ui-replay-constants_fallback_embedded = 使用内置常量。
ui-replay-copy_latest_log = 将最新日志复制到剪贴板
ui-replay-load_alt_perspective = 加载对方队伍视角...
ui-replay-load_alt_perspective_failed = 合并回放失败：{ $error }
ui-replay-load_alt_perspective_limit = 这场战斗已在通过 { $limit } 个其他录像读取
ui-replay-consumable_charges_remaining = 剩余 { $remaining } 次，共 { $total } 次
ui-replay-consumable_charges_unlimited = 已使用 { $used } 次（次数无限）
ui-replay-consumable_header_remaining = 剩余
ui-replay-consumable_header_total = 总计
ui-replay-consumable_charges_infinite = ∞
ui-replay-autoload_latest = 自动加载最新回放
ui-replay-autoload_latest_tooltip = 游戏写入完成后立即打开比赛
ui-replay-anonymize_names = 匿名化玩家名称
ui-replay-anonymize_names_tooltip = 以生成的名称代替玩家名称和军团标签显示。导出保留真实名称。
ui-replay-group_win_rate_all_modes = 分组胜率统计所有游戏模式
ui-replay-group_win_rate_all_modes_tooltip = 默认情况下，分组标题仅统计随机战和排位战。开启后统计所有游戏模式，且胜率不再着色，因为颜色是按随机战校准的。
ui-replay-select_replay = 选择回放
ui-replay-drop_hint = 将回放拖放到此处即可打开，拖放文件夹则列出其中的回放
ui-replay-scanning = 正在扫描回放...
ui-replay-scanning_count = 正在读取回放 { $done } / { $total }
ui-replay-no_replays_in = { $folder } 中没有回放
ui-replay-drop_folder_hint = 将文件夹拖放到窗口即可在其专属标签中列出
ui-replay-new_tag = 新
ui-replay-loading_game_data = 正在加载游戏数据...
ui-replay-game_data_failed = 游戏数据加载失败：{ $error }
ui-replay-group_label = 回放分组
ui-replay-column_filters = 列筛选
ui-replay-row_elements = 行显示内容
ui-replay-row_element_personal_rating = 个人评分
ui-replay-row_element_match_contribution = 比赛贡献
ui-replay-row_element_division = 分队
ui-replay-row_element_twitch = Twitch 聊天
ui-replay-row_element_damage = 伤害
ui-replay-row_element_kills = 击沉
ui-replay-row_element_time = 战斗时间
ui-replay-listing_options = 列表选项
ui-replay-open = 打开
ui-replay-open_file = 打开回放文件...
ui-replay-check_game_data = 检查游戏数据
ui-replay-check_game_data_tooltip = 检查此目录中的回放所需的游戏数据
ui-replay-close_side_panel = 关闭面板
ui-replay-nda = NDA
ui-replay-chat = 聊天
ui-replay-game_chat = 游戏聊天
ui-replay-save_to_file = 保存到文件
ui-replay-chat_copied = 聊天已复制
ui-replay-copy_message = 复制消息
ui-replay-actions = 操作
ui-replay-row_actions_hint = 玩家操作（或右键单击该行）
ui-replay-section_match = 比赛
ui-replay-section_export = 导出
ui-replay-section_debug = 调试
ui-replay-timeline = 比赛时间线
ui-replay-row_not_indexed = 未建立索引
ui-replay-hover_survived = 存活
ui-replay-hover_sunk = 已沉没
ui-replay-hover_not_indexed = 未建立索引
ui-replay-row_kills = { $count } 次击沉
ui-replay-row_survived = 存活
ui-replay-row_sunk = 已沉没
ui-replay-row_division = 分队：{ $members }
ui-replay-export_results_json = 结果：JSON
ui-replay-export_results_cbor = 结果：CBOR
ui-replay-export_results_csv = 结果：CSV
ui-replay-timeline_filter_all = 全部
ui-replay-timeline_filter_none = 无
ui-replay-timeline_search_hint = 搜索舰船或玩家
ui-replay-timeline_no_events = 未检测到重要事件。
ui-replay-timeline_no_matches = 没有事件符合当前筛选条件。
ui-replay-timeline_parsing = 正在解析事件...
ui-replay-timeline_failed = 提取比赛时间线失败：{ $error }
ui-replay-timeline_retry = 重试
ui-replay-timeline_export_tooltip = 仅适用于筛选后当前可见的事件。
ui-replay-timeline_copied = 时间线已复制
ui-replay-timeline_event_copied = 事件已复制
ui-replay-timeline_saved = 时间线已保存到 { $path }
ui-replay-timeline_save_failed = 未能保存时间线：{ $error }
ui-replay-timeline_kind_health_lost = 重创
ui-replay-timeline_kind_death = 被击沉
ui-replay-timeline_kind_cap_contested = 占领点争夺中
ui-replay-timeline_kind_cap_flipped = 占领点被占领
ui-replay-timeline_kind_cap_being_captured = 占领点正被占领
ui-replay-timeline_kind_radar = 使用雷达
ui-replay-timeline_kind_advantage = 优势变化
ui-replay-timeline_kind_disconnected = 掉线

## ui.replay.context
ui-replay-context-open_in_new_tab = 在新标签中打开
ui-replay-context-copy_cell = 复制单元格
ui-replay-context-copy_row = 复制行
ui-replay-context-copy_table = 复制表格
ui-replay-context-pin_column = 固定列
ui-replay-context-unpin_column = 取消固定列
ui-replay-context-copied = 已复制
ui-replay-context-copy_path = 复制路径
ui-replay-context-copy_replays = { $count ->
    [one] 复制回放
   *[other] 复制 { $count } 个回放
}
ui-replay-context-copy_paths = { $count ->
    [one] 复制路径
   *[other] 复制 { $count } 个路径
}
ui-replay-context-show_in_explorer = 在文件资源管理器中显示
ui-replay-context-show_replay_controls = 显示回放控制
ui-replay-context-open_in_game = 在游戏中打开
ui-replay-context-render_replay = 渲染回放
ui-replay-render = { ui-replay-context-render_replay }
ui-replay-context-set_session_stats = 设为会话统计（{ $count } 个回放）
ui-replay-context-add_session_stats = 添加到会话统计（{ $count } 个回放）
ui-replay-context-render_to_video = { $count ->
    [one] 导出为视频
   *[other] 将 { $count } 个回放导出为视频
}
ui-replay-context-render_to_clipboard = { $count ->
    [one] 导出到剪贴板
   *[other] 将 { $count } 个回放导出到剪贴板
}

## ui.replay.column
ui-replay-column-player_name = 玩家名称
ui-replay-column-base_xp = Base XP
ui-replay-column-ship_name = 舰船名称
ui-replay-column-hits = 命中
ui-replay-column-heals = 维修
ui-replay-column-heals_tooltip = 观察到该玩家启动维修小组的次数。对于未在屏幕上渲染的舰船（即从未被侦察到的敌舰）可能不准确。
ui-replay-column-heals_no_repair_tooltip = 此舰船没有维修小组消耗品。
ui-replay-column-kills = 击沉
ui-replay-column-observed_damage = 观察伤害
ui-replay-column-actual_damage = 实际伤害
ui-replay-column-potential_damage = 潜在伤害
ui-replay-column-time_lived = 存活时间
ui-replay-column-fires = 火灾
ui-replay-column-floods = 进水
ui-replay-column-citadels = 核心区
ui-replay-column-crits = 暴击
ui-replay-column-received_damage = 受到伤害
ui-replay-column-distance_traveled = 行驶距离
ui-replay-column-skills = 技能
ui-replay-column-personal_rating = PR
ui-replay-column-match_contribution = MC

## ui.rating
ui-rating-personal_rating = 个人评分
ui-rating-match_contribution = 战斗贡献
ui-rating-versus_average = 对比平均
ui-rating-damage = 伤害
ui-rating-kills = 击沉
ui-rating-wins = 胜利
ui-rating-team_damage_share = 队伍伤害占比
ui-rating-tanking = 吸引火力
ui-rating-spotting = 侦察
ui-rating-objectives = 目标
ui-rating-anti_air = 防空
ui-rating-torpedo_warnings = 鱼雷预警
ui-rating-critical_bursts = 致命爆发
ui-rating-unrated_early_exit = 无 MC：录制在战斗结束前终止
ui-rating-mc_unrated_battle_type = 无 MC：战斗贡献仅适用于随机战和排位战

## ui.rating.detail
ui-rating-detail-versus_expected = { $actual }，预期 { $expected }
ui-rating-detail-wins = 胜 { $actual }，预期 { $expected }
ui-rating-detail-weighted_damage = { $actual }，预期 { $expected }，驱逐舰和潜艇伤害 x1.2
ui-rating-detail-team_damage_share = 占队伍对敌伤害的 { $share }%，平均份额为 { $even }%
ui-rating-detail-tanking = { $potential } 潜在伤害，为同类平均的 { $class }x，为队伍平均份额的 { $team }x
ui-rating-detail-spotting = { $damage } 侦察伤害（同类平均 { $baseline }），侦察到舰船 HP { $ships }（平均 { $ships_baseline }）
ui-rating-detail-objectives = 得分：占领 { $capture }，阻止 { $block }，重置 { $reset }，结束时占据 { $battle_end }，占据 { $hold }（按四分之一计算）
ui-rating-detail-anti_air_planes = { $strike } 架攻击机，{ $support } 架战斗机和侦察机
ui-rating-detail-anti_air_damage = { $damage } 防空伤害
ui-rating-detail-torpedo_warnings = 发现 { $threatening } 枚威胁鱼雷
ui-rating-detail-critical_bursts = { $count } 次致命爆发：{ $bursts }。仅对录制玩家附近的舰船可见
ui-rating-detail-critical_burst = { $seconds } 秒内造成 { $ship } 的 { $share }%
ui-rating-detail-critical_burst_at_once = 一次造成 { $ship } 的 { $share }%

## ui.personal_rating
ui-personal_rating-unrated_hover = 个人评分仅适用于随机战和排位战。

## ui.rating.basis
ui-rating-basis-static = 静态
ui-rating-basis-dynamic = 动态
ui-rating-basis-static_explained = 静态：与固定常量或舰种基准比较
ui-rating-basis-dynamic_explained = 动态：与 wows-numbers 预期值或本场战斗其他玩家比较

## ui.replay.group
ui-replay-group-date = 日期
ui-replay-group-ship = 舰船
ui-replay-group-none = 无

## ui.replay.player
ui-replay-player-hidden_profile = 该玩家隐藏了个人资料
ui-replay-player-never_connected = 玩家从未连接到比赛

## ui.replay.build
ui-replay-build-open_shiptool = 在 ShipTool 中打开配置
ui-replay-build-open_in_ship_builder = 在 WoWs SB 中打开配置
ui-replay-build-copy_link = 复制配置链接
ui-replay-build-copy_short_link = 复制短配置链接
ui-replay-build-link_copied = 配置链接已复制
ui-replay-build-open_wows_numbers = 打开 WoWs Numbers 页面
ui-replay-build-not_spotted = 此舰船从未被侦察到。配置信息不可用。

## ui.replay.sections
ui-replay-sections-achievements = 成就
ui-replay-sections-ribbons = 勋带
ui-replay-sections-modules = 模块：
ui-replay-sections-modules_none = 无模块
ui-replay-sections-signals = 信号旗：
ui-replay-sections-loadout = 组件：
ui-replay-sections-loadout_none = 无组件
ui-replay-sections-captain_skills = 舰长技能：
ui-replay-sections-captain_skills_none = 无舰长技能
ui-replay-sections-consumables = 消耗品：
ui-replay-sections-fire_chance = 有效纵火概率
ui-replay-sections-fire_chance_expected = 预期
ui-replay-sections-fire_chance_fires = { $fires } 次起火
ui-replay-sections-fire_chance_eligible_hits = { $hits } 次可能引发起火的命中
ui-replay-sections-fire_chance_ships = 涉及 { $ships } 艘目标舰船
ui-replay-sections-fire_chance_expected_fires = 预期 { $fires } 次起火
ui-replay-sections-fire_chance_per_ship = 按目标舰船
ui-replay-sections-fire_chance_no_eligible_hits = 没有可能引发起火的命中
ui-replay-sections-fire_chance_formula = 攻击方纵火概率公式
ui-replay-sections-fire_chance_formula_base = 基础 burnProb
ui-replay-sections-fire_chance_formula_clamped = 已钳制
ui-replay-sections-fire_chance_breakdown = 明细
ui-replay-sections-fire_chance_shells_fired = 已发射的 HE 炮弹
ui-replay-sections-fire_chance_he_hits = 命中舰船的 HE 炮弹
ui-replay-sections-fire_chance_no_target_ship = 无法归属到目标舰船的 HE 命中
ui-replay-sections-fire_chance_eligible = 可能引发起火
ui-replay-sections-fire_chance_ribbons = SetFire 勋带
ui-replay-sections-fire_chance_ribbons_credited = 已归属到某发炮弹
ui-replay-sections-fire_chance_ribbons_uncredited = 未归属
ui-replay-sections-fire_chance_unattributed_no_burn_state = 没有可与之匹配的燃烧状态变化
ui-replay-sections-fire_chance_unattributed_already_credited = 较早的一次起火已占用附近的命中
ui-replay-sections-fire_chance_unattributed_secondary_contest = 我方副炮同样有可能引发此次起火
ui-replay-sections-fire_chance_unattributed_hit_contest = 我方附近的每次命中都与另一次命中存在歧义
ui-replay-sections-fire_chance_unattributed_all_excluded = 我方附近的每次命中均已被排除
ui-replay-sections-fire_chance_unattributed_none_could_burn = 我方附近没有可能引发起火的命中
ui-replay-sections-fire_chance_unattributed_no_hit = 我方在该时间窗口内没有命中
ui-replay-sections-fire_chance_not_applicable = 目标已被击沉，不适用
ui-replay-sections-fire_chance_copied = 纵火概率明细已复制
ui-replay-sections-fire_chance_click_to_copy = 单击以复制此明细
ui-replay-sections-fire_chance_exclusion_already_burning = 该部位已在燃烧
ui-replay-sections-fire_chance_exclusion_victim_build_unknown = 目标配置未知，起火区域可能被合并
ui-replay-sections-fire_chance_exclusion_damage_control_active = 损管小组处于激活状态
ui-replay-sections-fire_chance_exclusion_damage_control_unknown = 损管状态未知
ui-replay-sections-fire_chance_exclusion_observation_gap = 观测存在空档
ui-replay-sections-fire_chance_exclusion_consumable_unreliable = 消耗品启动模型不可靠
ui-replay-sections-fire_chance_exclusion_victim_fate_unknown = 目标结局未知
ui-replay-sections-fire_chance_exclusion_hit_type_does_not_roll = 此命中类型不进行起火判定
ui-replay-sections-fire_chance_exclusion_no_geometry = 没有起火区域几何数据
ui-replay-sections-fire_chance_exclusion_impact_unplaceable = 无法在匹配到的舰船上定位该弹着点
ui-replay-sections-fire_chance_exclusion_victim_pose_unknown = 命中时目标位置未知
ui-replay-sections-fire_chance_exclusion_ambiguous_hit = 与我方另一次命中存在歧义
ui-replay-sections-damage_events = 伤害事件

## ui.replay.debug
ui-replay-debug-view_raw_metadata = 查看原始玩家元数据
ui-replay-debug-raw_metadata = 原始元数据
ui-replay-debug-raw_results = 原始结果
ui-replay-debug-mapped_results = 映射结果
ui-replay-debug-player_metadata = 玩家元数据
ui-replay-debug-search = 搜索
ui-replay-debug-previous_match = 上一个匹配项（Shift+Enter）
ui-replay-debug-next_match = 下一个匹配项（Enter）

## ui.replay.controls
ui-replay-controls-window_title = 回放控制
ui-replay-controls-unavailable = 控制不可用（在游戏文件中未找到 commands.scheme.xml）。

# -- 协作 / 会话 --

## ui.collab
ui-collab-session = 会话
ui-collab-starting = 正在启动会话...
ui-collab-session_active = 会话活跃
ui-collab-connected_to_session = 已连接到会话
ui-collab-session_token = 会话令牌：
ui-collab-toggle_visibility = 切换令牌可见性
ui-collab-copy_token = 复制令牌
ui-collab-token_copied = 令牌已复制到剪贴板
ui-collab-copy_web_link = 复制网页链接
ui-collab-web_link_copied = 网页链接已复制到剪贴板
ui-collab-copy_localhost_link = 复制 Localhost 链接
ui-collab-localhost_copied = Localhost 链接已复制到剪贴板
ui-collab-connected_count = { $count } 人已连接
ui-collab-promote_cohost = 提升为联合主持
ui-collab-permissions = 权限
ui-collab-lock_annotations = 锁定标注
ui-collab-lock_settings = 锁定设置
ui-collab-reset_overrides = 重置客户端覆盖
ui-collab-reset_overrides_tooltip = 重置所有客户端显示设置更改
ui-collab-leave = 离开
ui-collab-role_host = 主持
ui-collab-role_cohost = 联合主持
ui-collab-you = （你）
ui-collab-display_name = 显示名称：
ui-collab-display_name_hint = 你的名字...
ui-collab-enter_display_name = 请先输入显示名称
ui-collab-host_session = 主持会话
ui-collab-start_session = 开始会话
ui-collab-join_session = 加入会话
ui-collab-paste_and_join = 粘贴令牌并加入
ui-collab-clipboard_empty = 剪贴板为空
ui-collab-invalid_token = 无效令牌：{ $error }
ui-collab-no_runtime = 无法启动会话：没有运行时
ui-collab-shared_windows = 共享窗口
ui-collab-watching = 观看中
ui-collab-waiting_for_frames = 正在等待战斗数据
ui-collab-open = 打开
ui-collab-open_for_everyone = 为所有人打开
ui-collab-tactics_board = 战术板

# -- 战斗结果文字 --

## battle
battle-victory = 胜利
battle-defeat = 失败
battle-draw = 平局
battle-battle_starts_in = 战斗开始倒计时

# -- 结束类型 --

## finish_type
finish_type-unknown = 战斗结束
finish_type-extermination = 消灭所有敌舰
finish_type-base_captured = 基地被占领
finish_type-timeout = 时间到
finish_type-failure = 任务失败
finish_type-technical = 技术性结束
finish_type-score = 达到分数上限
finish_type-score_on_timeout = 时间到时分数领先
finish_type-pve_success = 任务完成
finish_type-pve_failed = 任务失败
finish_type-score_zero = 分数耗尽
finish_type-score_excess = 超过分数上限

# -- 统计标签页 --

## ui.stats
ui-stats-overview = 概览
ui-stats-sample_notice = 示例数据，非已记录的对局
ui-stats-div_all = 全部
ui-stats-div_solo = 单人
ui-stats-div_div = 分队
ui-stats-filters = 筛选
ui-stats-own_filters = 单独筛选此图表
ui-stats-own_filters_tooltip = 仅缩小此图表的范围，不影响其他面板的显示内容
ui-stats-mode_label = 模式：
ui-stats-clear = 清除
ui-stats-pr = PR：
ui-stats-mc = MC：
ui-stats-best_frags = 最佳：{ $ship }（{ $count } 次击沉）
ui-stats-max_damage = 最高伤害：{ $ship }（{ $damage }）
ui-stats-copy_markdown = 复制为 Markdown
ui-stats-copy_image = 复制为图片
ui-stats-copy_image_done = 图表已复制到剪贴板
ui-stats-copy_image_failed = 无法复制图表
ui-stats-copy_csv = 复制为 CSV
ui-stats-remove_games_hint = 移除 { $ship } 的所有战斗（Ctrl+Click 跳过确认）
ui-stats-settings = 设置
ui-stats-options = 选项
ui-stats-ships = 舰船
ui-stats-filter_ships = 筛选图表舰船
ui-stats-all_ships = 全部
ui-stats-no_ships = 无
ui-stats-search_ships = 搜索舰船...
ui-stats-no_ship_matches = 没有符合搜索条件的舰船
ui-stats-search_statistics = 搜索统计项
ui-stats-no_statistic_matches = 没有符合搜索条件的统计项
ui-stats-ship_selection_count = 已选择 { $selected } / { $total }
ui-stats-all_ships_tooltip = 选择所有匹配的舰船
ui-stats-no_ships_tooltip = 清除匹配舰船的选择
ui-stats-limit_to_recent = 仅限最近的比赛
ui-stats-limit_to_recent_tooltip = 在“舰船”和“图表”中按每艘舰船分别限制，在“概览”中对所有比赛统一限制。
ui-stats-division_label = 分队：
ui-stats-add_chart = 添加图表
ui-stats-clear_tooltip = 删除所有已记录的比赛
ui-stats-clear_failed = 未清除：{ $reason }
ui-stats-clear_ship_confirm = 再次按下以删除此舰船的比赛
ui-stats-copy_table = 复制此表格
ui-stats-copied = 已复制到剪贴板
ui-stats-no_games = 当前筛选条件下没有已记录的比赛
ui-stats-nothing_to_plot = 当前筛选条件下没有可绘制的数据
ui-stats-column_ship = 舰船
ui-stats-column_battles = 战斗场数
ui-stats-column_win_rate = 胜率
ui-stats-column_avg_damage = 平均伤害
ui-stats-column_avg_frags = 平均击沉
ui-stats-frags = { $count } 次击沉
ui-stats-statistic = 统计项
ui-stats-open_directory_stats = 目录统计
ui-stats-open_directory_stats_tooltip = 打开此目录及其子目录中所有回放的统计
ui-stats-directory_stats_unavailable = 游戏数据仍在加载
ui-stats-directory_reading_task = 正在读取回放以生成目录统计...
ui-stats-directory_walking = 正在查找回放...
ui-stats-directory_scanning = 正在检查第 { $done } / { $total } 个回放...
ui-stats-directory_reading = 正在读取第 { $done } / { $total } 个回放...
ui-stats-directory_reload = 重新加载
ui-stats-directory_reload_tooltip = 重新读取此目录中的回放
ui-stats-directory_skipped = 已跳过 { $count } 个回放
ui-stats-directory_skipped_detail =
    无法读取：{ $unreadable }
    没有对应构建的游戏数据：{ $missing_build }
    没有战斗结果：{ $no_results }
ui-stats-chart = 图表
ui-stats-combine_ships = 合并舰船
ui-stats-running_average = 累计平均
ui-stats-value_labels = 数值标签
ui-stats-reset_view = 重置视图
ui-stats-keep_current_layout = 保持当前布局
ui-stats-session_title = 会话统计
ui-stats-session_manual = 手动
ui-stats-session_since_opened = 自应用启动以来
ui-stats-session_manual_tooltip = 会话包含您进行的战斗，以及您从回放列表中设置或添加的回放。它会在多次运行之间保留。
ui-stats-session_since_opened_tooltip = 会话包含在 { $time }（应用启动时）或之后开始的所有战斗。您的手动会话会被保留，切换到手动时即可恢复。
ui-stats-session_battles = { $count } 场战斗
ui-stats-session_chip_manual = 会话：{ $battles }
ui-stats-session_chip_since = 自 { $time } 起的会话：{ $battles }
ui-stats-session_empty_title = 此会话中尚无战斗
ui-stats-session_empty_manual = 您进行的战斗在结束后加入会话。您也可以从回放列表的菜单中设置或添加回放。
ui-stats-session_empty_since = 在 { $time } 之后开始的战斗会在结束后加入会话。
ui-stats-session_set_add_unavailable = 会话包含自 { $time } 起的战斗。切换到手动以设置或添加回放。
ui-stats-clear_unavailable_since = 自应用启动以来的会话不会被保存。切换到手动以清除已保存的会话。
ui-stats-charts_save_failed = 图表设置未保存：{ $reason }
ui-stats-layout_save_failed = 图表布局未保存：{ $reason }
ui-stats-axis_game = 场次
ui-stats-chart_hover_game = 第{ $number }场，{ $time }
ui-stats-series_average = { $stat }（平均）
ui-stats-series_running = { $stat }（累计）
ui-stats-series_more = 还有 { $count } 项
ui-stats-mode_random = 随机战
ui-stats-mode_ranked = 排位战
ui-stats-mode_other = 其他
ui-stats-win_rate_rated_only = 胜率仅统计随机战和排位战
ui-stats-win_rate_breakdown_line = { $mode }：{ $rate }（{ $battles } 场）
ui-stats-matches = 对局
ui-stats-column_time = 时间
ui-stats-column_mode = 模式
ui-stats-column_result = 结果
ui-stats-keep_game = 将此对局计入统计
ui-stats-leave_out_game = 将此对局排除在统计之外
ui-stats-leave_out_ship = 排除所有 { $ship } 对局
ui-stats-leave_out_ship_hint = 将 { $ship } 排除在统计之外
ui-stats-keep_all_games = 计入所有对局
ui-stats-keep_all_games_tooltip = 重新计入所有手动排除的对局
ui-stats-left_out_only = 仅显示已排除
ui-stats-match_count = { $total } 场中的 { $shown } 场，已排除 { $left_out } 场
ui-stats-no_matches = 没有符合当前筛选条件的对局
ui-stats-search_matches = 按舰船搜索...
ui-stats-ship_filter_tooltip = 选择计入统计的舰船
ui-stats-keep_ships_tooltip = 计入所有匹配的舰船
ui-stats-leave_out_ships_tooltip = 将所有匹配的舰船排除在统计之外
ui-stats-dates = 日期
ui-stats-all_dates = 所有日期
ui-stats-clear_dates = 显示所有日期
ui-stats-games_left_out = 已排除 { $count } 场对局
ui-stats-reset_filters = 重置筛选
ui-stats-reset_filters_tooltip = 显示所有对局：无数量限制、日期范围、分队、模式或已排除的舰船和对局

## ui.stats.table
ui-stats-table-min = 最小
ui-stats-table-max = 最大
ui-stats-table-total = 总计
ui-stats-table-average = 平均

# -- 玩家追踪器 --

## ui.player_tracker
ui-player_tracker-clear_stats = 清除统计
ui-player_tracker-populate_from_replays = 从回放填充数据
ui-player_tracker-find_matches = 查找比赛
ui-player_tracker-open_wows_numbers = 在 wows-numbers 上打开
ui-player_tracker-open_shipbuilds = 在 shipbuilds 上打开配置
ui-player_tracker-subtab_players = 玩家
ui-player_tracker-filter_hint = 按玩家或战队筛选...
ui-player_tracker-time_period = 时间范围
ui-player_tracker-allies = 友方
ui-player_tracker-enemies = 敌方
ui-player_tracker-live_no_directory = 请设置 World of Warships 目录以监视战斗
ui-player_tracker-live_checking = 正在检查是否有进行中的战斗...
ui-player_tracker-live_none = 没有进行中的战斗
ui-player_tracker-live_loading_build = 正在加载此构建的舰船数据，加载完成后将补全名称和舰种。
ui-player_tracker-waiting_for_index = 正在等待回放索引
ui-player_tracker-loading_clans = 正在加载战队...
ui-player_tracker-loading_players = 正在加载玩家...
ui-player_tracker-index_failed = 无法读取索引：{ $reason }
ui-player_tracker-no_players = 此时间范围内没有已索引的玩家
ui-player_tracker-note_not_saved = 未保存：{ $reason }
ui-player_tracker-subtab_current_match = 当前比赛
ui-player_tracker-encounters_hover = 共遇到 { $total } 次，所选时间范围内 { $range } 次。上次遇到：{ $last }。
ui-player_tracker-never_encountered = 此前未遇到过
ui-player_tracker-copy_wg_id = 单击以复制 WG ID
ui-player_tracker-copied_wg_id = 已复制 WG ID { $id }
ui-player_tracker-aliases_hover = 曾用名：{ $names }
ui-player_tracker-notes_hint = 关于此玩家的备注
ui-player_tracker-arena_count = 已记录 { $count } 场比赛
ui-player_tracker-last_encountered_exact = 上次遭遇于 { $timestamp }
ui-player_tracker-subtab_clans = 战队
ui-player_tracker-clan_no_data = 尚未记录任何战队遭遇。
ui-player_tracker-find_clan_matches = 查找提及此战队标签的比赛。索引中没有仅限战队的字段，因此包含该标签的玩家名称也会被匹配。
ui-player_tracker-clan_sightings_hover = 其中 { $range } 次落在所选时间范围内。
ui-player_tracker-clan_member_matches = 在 { $count } 场比赛中出现
ui-player_tracker-show_division_mates = 显示分队队友
ui-player_tracker-show_division_mates_hover = 统计您组成分队进行的比赛。默认关闭，因为那些玩家是您邀请才遇到的。与同一批玩家的其他比赛始终会被统计。
ui-player_tracker-seen_in_twitch_chat = 在 Twitch 聊天中出现
ui-player_tracker-seen_in_twitch_chat_hover = 仅显示在与您同场的比赛中，名称与 Twitch 聊天登录名匹配的玩家。
ui-player_tracker-twitch_seen_hover = 在与您同场的 { $count } 场比赛中以 { $logins } 出现在 Twitch 聊天中
ui-player_tracker-win_rate_overall = 总体
ui-player_tracker-win_rate_ship = 舰船
ui-player_tracker-view_compact = 紧凑
ui-player_tracker-view_detailed = 详细
ui-player_tracker-ship_stats = 舰船统计
ui-player_tracker-stats_resolving = 正在读取比赛名单...
ui-player_tracker-stats_fetching = 正在获取玩家统计...
ui-player_tracker-stats_unavailable = 统计不可用：{ $reason }
ui-player_tracker-stats_hidden = 该玩家的个人资料为私密
ui-player_tracker-stats_no_data = 此玩家没有统计数据
ui-player_tracker-pr_overall_hover = 所有舰船的账号 PR
ui-player_tracker-pr_ship_hover = 此舰船的 PR
ui-player_tracker-win_rate_hover_line = { $label }：{ $rate }，共 { $battles } 场战斗
ui-player_tracker-win_rate_hover_line_damage = { $label }：{ $rate }，共 { $battles } 场战斗，平均伤害 { $damage }
ui-player_tracker-team_average_win_rate = 平均胜率：{ $rate }
ui-player_tracker-team_players = { $count } 名玩家
ui-player_tracker-debug_replay_picker = 调试：将某个回放当作当前比赛
ui-player_tracker-debug_replay_picker_hover = 将已结束回放的名单加载到此标签页，以便在战斗之外测试统计流程。
ui-player_tracker-debug_replay_none = 选择一个回放

## ui.player_tracker.column
ui-player_tracker-column-win_rate = 胜率
ui-player_tracker-column-personal_rating = PR
ui-player_tracker-column-battles = 战斗
ui-player_tracker-column-avg_damage = 伤害
ui-player_tracker-column-encounters = 遇到
ui-player_tracker-column-clan = 战队
ui-player_tracker-column-player_name = 玩家名称
ui-player_tracker-column-total_encounters = 总遭遇次数
ui-player_tracker-column-encounters_in_range = 时间范围内遭遇次数
ui-player_tracker-column-last_encountered = 上次遭遇
ui-player_tracker-column-members = 成员
ui-player_tracker-column-matches = 比赛
ui-player_tracker-column-sightings = 出现次数

## ui.player_tracker.period
ui-player_tracker-period-past_hour = 过去 1 小时
ui-player_tracker-period-past_six_hours = 过去 6 小时
ui-player_tracker-period-past_day = 过去 24 小时
ui-player_tracker-period-past_week = 过去一周
ui-player_tracker-period-past_month = 过去一个月
ui-player_tracker-period-all_time = 所有时间

# -- 模组管理器 --

## ui.twitch
ui-twitch-possible_name = 可能的直播名称：{ $name }
ui-twitch-seen_minutes = 发现于：比赛开始后 { $minutes } 分钟
ui-twitch-click_to_copy = 单击以复制
ui-twitch-copied = 已复制 { $name }

# -- 回放渲染器 --

## ui.renderer
ui-renderer-preparing_export = 正在准备导出...
ui-renderer-encoding = 编码中
ui-renderer-muxing = 混流中

## ui.renderer.advantage
ui-renderer-advantage-breakdown = 优势分析
ui-renderer-advantage-team_eliminated = 一支队伍已被消灭
ui-renderer-advantage-score_projection = 分数预测
ui-renderer-advantage-fleet_power = 舰队战力
ui-renderer-advantage-strategic_threat = 战略威胁
ui-renderer-advantage-total = 总计
ui-renderer-advantage-hp_incomplete = HP/舰船数据不完整。仅显示分数因素

## ui.renderer.context
ui-renderer-context-show_trail = 显示轨迹
ui-renderer-context-disable_other_trails = 禁用所有其他轨迹
ui-renderer-context-ranges = 范围
ui-renderer-context-detection = 侦察
ui-renderer-context-main_battery = 主炮
ui-renderer-context-secondary = 副炮
ui-renderer-context-torpedo = 鱼雷
ui-renderer-context-radar = 雷达
ui-renderer-context-hydro = 水听
ui-renderer-context-enable_all = 全部启用
ui-renderer-context-disable_all = 全部禁用
ui-renderer-context-disable_other_ranges = 禁用所有其他范围
ui-renderer-context-enable_all_ranges = 启用所有舰船范围
ui-renderer-context-realtime_armor = 显示实时装甲

## ui.renderer.controls
ui-renderer-controls-jump_to_start = 跳至开始
ui-renderer-controls-previous_event = 上一事件 (Shift+Left)
ui-renderer-controls-back_10s = 后退 10 秒 (Left)
ui-renderer-controls-forward_10s = 前进 10 秒 (Right)
ui-renderer-controls-next_event = 下一事件 (Shift+Right)
ui-renderer-controls-jump_to_end = 跳至结束
ui-renderer-controls-speed = 播放速度

## ui.renderer.settings
ui-renderer-settings-title = 显示设置
ui-renderer-settings-hp_bars = 血条
ui-renderer-settings-armament = 武器装备
ui-renderer-settings-dead_ships = 已沉没舰船
ui-renderer-settings-dead_ship_names = 已沉没舰船名称
ui-renderer-settings-player_names = 玩家名称
ui-renderer-settings-ship_ranges = 舰船范围
ui-renderer-settings-ship_names = 舰船名称
ui-renderer-settings-camera_direction = 炮塔方向
ui-renderer-settings-heat_trail = 热力轨迹
ui-renderer-settings-speed_trails = 速度轨迹
ui-renderer-settings-dead_ship_trails = 已沉没舰船轨迹
ui-renderer-settings-buildings = 建筑
ui-renderer-settings-capture_points = 占领点
ui-renderer-settings-consumables = 消耗品
ui-renderer-settings-planes = 飞机
ui-renderer-settings-smoke = 烟雾
ui-renderer-settings-torpedoes = 鱼雷
ui-renderer-settings-tracers = 曳光弹
ui-renderer-settings-battle_result = 战斗结果
ui-renderer-settings-buff_counters = 增益计数器
ui-renderer-settings-chat_label = 聊天
ui-renderer-settings-kill_feed = 击沉通知
ui-renderer-settings-score_label = 分数
ui-renderer-settings-score_timers = 分数计时器
ui-renderer-settings-team_advantage = 团队优势
ui-renderer-settings-team_rosters = 队伍名单
ui-renderer-settings-stats_panel = 统计面板
ui-renderer-settings-timer = 计时器
ui-renderer-settings-export_settings = 导出设置
ui-renderer-settings-prefer_cpu = 优先使用 CPU 编码器
ui-renderer-settings-prefer_cpu_tooltip = 视频导出时使用软件 (CPU) 编码器代替 GPU
ui-renderer-settings-include_pre_battle = 包含战前阶段
ui-renderer-settings-include_pre_battle_tooltip = 在导出的视频中包含战斗开始前的出生和倒计时阶段
ui-renderer-settings-export_timing = 视频计时
ui-renderer-settings-export_timing_length = 适应时长
ui-renderer-settings-export_timing_speed = 速度
ui-renderer-settings-export_timing_result_speed = 约以 { $speed }x 播放
ui-renderer-settings-export_timing_result_length = 视频时长 { $length }
ui-renderer-settings-codec = 编解码器
ui-renderer-settings-codec_auto = 自动
ui-renderer-settings-save_defaults = 保存默认值
ui-renderer-settings-save_defaults_done = 已保存为渲染器打开时的默认设置
ui-renderer-settings-save_defaults_failed = 无法保存默认设置
ui-renderer-settings-event_timeline = 事件时间线

# -- 文件解包器 --

## ui.unpacker
ui-unpacker-assets_bin = Assets.bin
ui-unpacker-loading_assets = 正在加载 assets.bin...
ui-unpacker-load_failed = 加载失败：{ $error }
ui-unpacker-no_game_data = 未加载游戏数据。请在设置中设置您的 World of Warships 目录。
ui-unpacker-filter_files = 筛选文件...
ui-unpacker-search_in_files = 在文件中搜索
ui-unpacker-search_hint = 在文件中搜索...
ui-unpacker-path_filter_hint = 路径筛选（例如 *.xml）
ui-unpacker-queue_all_files = 所有文件加入队列
ui-unpacker-view_as_json = 以 JSON 查看
ui-unpacker-extract_as_json = 提取为 JSON
ui-unpacker-view_contents = 查看内容
ui-unpacker-version = 版本：
ui-unpacker-decode_prototypes = 将原型解码为 JSON
ui-unpacker-decode_prototypes_tooltip = 提取时将受支持的 Assets.bin 原型（.visual、.model、.mfm）解码为 JSON
ui-unpacker-extraction_queue = 提取队列
ui-unpacker-collapse_folders = 折叠文件夹树
ui-unpacker-expand_folders = 展开文件夹树
ui-unpacker-clear_all = 清除全部
ui-unpacker-browse = 浏览
ui-unpacker-output_dir_hint = 输出目录...
ui-unpacker-remove_from_queue = 从队列中移除
ui-unpacker-queue_folder = 将整个文件夹加入队列
ui-unpacker-queue_file = 将此文件加入队列
ui-unpacker-extraction_output = 提取到
ui-unpacker-queue_empty = 在列表中勾选文件或文件夹以加入队列
ui-unpacker-packages = 资源包
ui-unpacker-item_count = { $count } 项
ui-unpacker-search_title = 搜索：{ $query }
ui-unpacker-search_in = 位于 { $source }
ui-unpacker-image_decode_failed = 无法解码此图像：{ $reason }
ui-unpacker-find = 查找
ui-unpacker-find_previous = 上一个匹配项
ui-unpacker-find_next = 下一个匹配项
ui-unpacker-find_close = 关闭查找
ui-unpacker-find_count = { $current } / { $total }
ui-unpacker-find_none = 无匹配项
ui-unpacker-find_match_case = 区分大小写
ui-unpacker-queue = 加入队列
ui-unpacker-reveal_in_tree = 在资源包树中显示
ui-unpacker-hit_offset = 位于 { $offset }
ui-unpacker-queued_count = 已加入队列 { $count } 项
ui-unpacker-counting_files = 正在统计文件...
ui-unpacker-extracting = 正在提取 { $done } / { $total }
ui-unpacker-extract_failed = 提取失败：{ $reason }
ui-unpacker-extracted = 已提取 { $count } 个文件
ui-unpacker-cancelled = 已在提取 { $count } 个文件后取消
ui-unpacker-extract = { $count ->
    [0] 提取
   *[other] 提取 { $count } 项
}
ui-unpacker-choose_directory = 请先选择提取目标目录
ui-unpacker-dump_parameters = 导出 GameParams
ui-unpacker-dump_parameters_tooltip = 导出此构建的 GameParams.data
ui-unpacker-load_a_build = 请先加载一个构建
ui-unpacker-parameters_written = 参数已写入
ui-unpacker-save_parameters_title = 保存游戏参数
ui-unpacker-writing = 正在写入...
ui-unpacker-prototype_written = 原型已写入
ui-unpacker-whole_tree = 完整树
ui-unpacker-base_only = 仅基础参数

## ui.unpacker.column
ui-unpacker-column-name = 名称
ui-unpacker-column-size = 大小
ui-unpacker-column-type_col = 类型

# -- 装甲查看器 --

## ui.armor
ui-armor-load_settings = 请在设置中加载游戏数据以使用装甲查看器。
ui-armor-load_failed = 加载舰船数据失败：{ $error }
ui-armor-empty = 空
ui-armor-mirror_cameras = 镜像摄像机
ui-armor-sync_options = 同步选项
ui-armor-show_armor_by_default = 默认显示装甲
ui-armor-sync_tooltip = 在所有面板中保持装甲/船体可见性和显示叠加层（船体中心、水线、摄像机环）同步
ui-armor-common_settings = 通用设置
ui-armor-tier_filter = { $lowest } 至 { $highest } 级
ui-armor-fortifications = 要塞
ui-armor-show_hidden_ships = 显示隐藏的舰船
ui-armor-show_hidden_ships_tooltip = 同时列出列表略去的舰船，例如军团租借舰船
ui-armor-compare_split = 在新分栏中比较
ui-armor-open_in_new_tab = 在新标签页中打开
ui-armor-export_model = 导出舰船模型
ui-armor-export_disclaimer = 这些 3D 模型和纹理是 Wargaming 的知识产权，使用这些模型应符合您当地的法律。
ui-armor-export_button = 导出
ui-armor-hull_toggle = 船体
ui-armor-splash_tooltip = 切换溅射框可见性
ui-armor-show_hidden = 显示隐藏
ui-armor-show_hidden_tooltip = 切换（可能）隐藏面板的可见性
ui-armor-gaps = 间隙
ui-armor-gaps_tooltip = 突出显示装甲板未连接的边界边缘（潜在间隙）
ui-armor-display = 显示
ui-armor-marker_opacity = 标记不透明度
ui-armor-export_tooltip = 将舰船模型导出为 OBJ 文件
ui-armor-pen_check = 穿深检查
ui-armor-trajectory = 弹道
ui-armor-trajectory_range = 距离
ui-armor-trajectory_clear = 全部清除
ui-armor-arc_heading = 弹道 { $number }
ui-armor-arc_summary = { $hits } 次命中，{ $range } km 处 { $armor }mm
ui-armor-arc_delete = 删除此弹道
ui-armor-arc_isolate_plates = 单独显示装甲板
ui-armor-arc_isolate_plates_tooltip = 仅显示此弹道穿过的装甲板
ui-armor-arc_isolate_zones = 单独显示装甲区域
ui-armor-arc_isolate_zones_tooltip = 显示此弹道进入的装甲区域的所有部分
ui-armor-arc_simulation_caveat = 此模拟基于逆向工程得到的数据，可能与游戏计算弹道的方式不一致。
ui-armor-arc_no_shell = 未投射炮弹，仅显示穿过的装甲板
ui-armor-arc_detonated_in = 在 { $zone } 内爆炸
ui-armor-arc_detonated_past_armor = 穿过全部装甲后爆炸
ui-armor-arc_ricochet = 在 { $plate } 处跳弹
ui-armor-arc_shatter = 在 { $plate } 处未击穿
ui-armor-arc_stopped = 在 { $plate } 处停止
ui-armor-arc_overpen = 过度击穿
ui-armor-arc_overpen_unarmed = 过度击穿（引信未触发）
ui-armor-arc_plate_unknown = 未记录的装甲板
ui-armor-angle_good = 小于 30 度
ui-armor-angle_mid = 30 至 45 度
ui-armor-angle_bad = 大于 45 度
ui-armor-trajectory_ship = 舰船：
ui-armor-trajectory_select_ship = 选择舰船
ui-armor-trajectory_ship_tooltip = 选择提供弹道炮弹的舰船
ui-armor-trajectory_add_ship = 添加射击舰船
ui-armor-trajectory_add_ship_tooltip = 打开穿深检查以搜索并添加舰船
ui-armor-trajectory_shell = 炮弹：
ui-armor-trajectory_select_shell = 选择炮弹
ui-armor-trajectory_shell_tooltip = 选择用于弹道投射的炮弹
ui-armor-continue_ricochet = 跳弹后继续
ui-armor-splash_mode = 溅射
ui-armor-splash_no_data = 此舰船没有溅射数据
ui-armor-splash_no_shells = 请将拥有 HE/SAP 炮弹的舰船添加到比较列表
ui-armor-splash_tooltip_mode = 单击装甲以可视化 HE 溅射伤害范围
ui-armor-select_ship = 从列表中选择一艘舰船
ui-armor-all_btn = 全部
ui-armor-none_btn = 无
ui-armor-opaque = 不透明
ui-armor-upgrade = 升级：
ui-armor-camo_none = 原厂（无）
ui-armor-camo_group_universal = 通用迷彩
ui-armor-camo_group_expendable = 消耗型迷彩
ui-armor-camo_group_other = 其他迷彩
ui-armor-show_armor_thickness = 显示装甲厚度图例
ui-armor-plate_edges = 装甲板边缘
ui-armor-waterline = 水线
ui-armor-camera_rings = 摄像机环
ui-armor-realtime_readout = { $health } HP  -  { $hits } 次命中
ui-armor-show_camera_rings = 显示摄像机环
ui-armor-camera_fov = 摄像机视场角
ui-armor-camera_height = 摄像机高度
ui-armor-show_camera_zoom_path = 显示缩放路径
ui-armor-zoom_path_regular_fov = 常规视场角
ui-armor-zoom_path_max_fov = 最大视场角
ui-armor-camera_perspective = 摄像机视角
ui-armor-perspective_fov = 视图视场角
ui-armor-perspective_projection = 投影
ui-armor-perspective_proj_game = 游戏内
ui-armor-perspective_proj_center = 穿过中心
ui-armor-ship_center = 船体中心
ui-armor-grid = 网格
ui-armor-length_marks = 长度刻度
ui-armor-length_readout = 长 { $length }，宽 { $beam }
ui-armor-length_metres = { $value } 米
ui-armor-length_centimetres = { $value } 厘米
ui-armor-filter_options = 筛选选项...
ui-armor-filter_zones = 筛选区域...
ui-armor-filter_parts = 筛选部件...
ui-armor-tree_matches = 匹配：{ $count }
ui-armor-tree_no_matches = 无匹配项
ui-armor-no_matching_options = 没有符合筛选的选项
ui-armor-opacity = 不透明度
ui-armor-opaque_hull = 不透明船体
ui-armor-lighting = 船体光照
ui-armor-lighting_enabled = 启用光照
ui-armor-lighting_preset_ingame = 游戏内
ui-armor-lighting_preset_flat = 平铺
ui-armor-lighting_preset_studio = 演播室
ui-armor-lighting_flat = 平铺（环境光）
ui-armor-lighting_intensity = 光照强度
ui-armor-lighting_azimuth = 光照方位角
ui-armor-lighting_elevation = 光照仰角
ui-armor-lighting_rim = 轮廓光
ui-armor-lighting_specular = 镜面反射
ui-armor-lighting_shininess = 光泽度
ui-armor-lighting_key_color = 主光颜色
ui-armor-lighting_ambient_color = 环境光颜色
ui-armor-zero_mm_plates = 0 mm 装甲板
ui-armor-armor_opacity = 装甲不透明度
ui-armor-ballistic_disclaimer = 弹道结果基于对模拟工作方式的逆向工程/估算
ui-armor-roll = 横摇
ui-armor-reset_plates = 重置装甲板
ui-armor-armor_options = 装甲
ui-armor-no_ship_loaded = 未加载舰船
ui-armor-lod = LOD：
ui-armor-lod_highest = 0（最高）
ui-armor-camouflage = 迷彩
ui-armor-ships = 舰船
ui-armor-compare = 比较
ui-armor-compare_tooltip = 打开新的比较面板，或右键单击舰船以比较该舰船
ui-armor-mirror_cameras_tooltip = 同步移动所有面板的摄像机
ui-armor-show_armor_by_default_tooltip = 打开模型时显示装甲而非船体
ui-armor-xray_highlight = 在视图中悬停时的透视高亮
ui-armor-xray_always = 始终
ui-armor-xray_while_held = 按住 { $key } 时
ui-armor-xray_off = 关闭
ui-armor-stack_panes = 堆叠面板
ui-armor-stack_panes_tooltip = 将比较面板上下排列，而不是左右并排
ui-armor-viewports = 视口
ui-armor-options_panel = 选项面板
ui-armor-options_pane = 面板 { $number } / { $count }
ui-armor-splash_boxes = 溅射框
ui-armor-cast_here_hint = 按住 { $key } 并点击以在此处投射炮弹
ui-armor-trajectory_next_cast = 下一次投射
ui-armor-arc_pick_hint = 点击标记以编辑其弹道。
ui-armor-arc_marker_tooltip = 弹道 { $number }。点击编辑，右键点击移除。
ui-armor-zone_toggle_tooltip = 显示或隐藏此区域的所有装甲板。按住 { $key } 可仅显示此区域。
ui-armor-mode_solo_zone = 仅此区域
ui-armor-mode_cast_trajectory = 投射弹道
ui-armor-loading_catalog = 正在加载舰船目录...
ui-armor-legend_collapse = 折叠
ui-armor-legend_hide = 隐藏图例
ui-armor-armor_thickness_title = 装甲厚度
ui-armor-initializing = 正在初始化 3D 视图...
ui-armor-init_failed = 装甲视图初始化失败：{ $reason }
ui-armor-loading_named_ship = 正在加载 { $ship }...
ui-armor-options = 选项

## ui.armor.compass
ui-armor-compass-north = 北
ui-armor-compass-east = 东
ui-armor-compass-south = 南
ui-armor-compass-west = 西

## ui.armor.realtime
ui-armor-realtime-incoming_fire = 来袭火力
ui-armor-realtime-attacker_filter = 攻击者筛选
ui-armor-realtime-all_enemies = 所有敌人
ui-armor-realtime-show_secondaries = 显示副炮
ui-armor-realtime-seek_to_salvo = 跳至齐射
ui-armor-realtime-no_data = 炮弹数据不再可用
ui-armor-realtime-sim_agrees = 模拟一致
ui-armor-realtime-sim_unchecked = { $count } 发炮弹无法模拟
ui-armor-realtime-sim_rng_zone = 随机区间（{ $from }-{ $to } 中的 { $angle } 度）
ui-armor-realtime-sim_disagrees = 模拟：{ $sim } / 服务器：{ $server }
ui-armor-realtime-no_armor_hit = 未命中装甲板
ui-armor-realtime-tracked = 已追踪 { $salvos } 次齐射、{ $shells } 发炮弹
ui-armor-realtime-nothing_landed = 此舰船尚未被命中
ui-armor-realtime-shells = { $count }x
ui-armor-realtime-salvo_shell = { $caliber } 毫米 { $shell }
ui-armor-realtime-salvo_range = { $km } km
ui-armor-realtime-unknown_attacker = 舰船 { $id }
ui-armor-realtime-server_outcome = 服务器: { $outcome }
ui-armor-realtime-not_cast = 无法在船体内追踪此炮弹: 未记录其飞行或舰船位置
ui-armor-realtime-pick_hint = 点击齐射或炮弹以在船体内追踪其弹道

## ui.armor.plates
ui-armor-plates-heading = 命中的装甲板 ({ $count })
ui-armor-plates-arrival = 着弹 { $velocity } m/s，飞行 { $time } 秒，落角 { $fall } 度
ui-armor-plates-overmatch = 过穿: 穿深 { $penetration } mm，{ $velocity } m/s
ui-armor-plates-penetrated = 击穿: { $penetration } mm 对 等效 { $effective } mm，{ $velocity } m/s
ui-armor-plates-ricochet = { $angle } 度跳弹
ui-armor-plates-shatter = 碎裂: { $penetration } mm 对 等效 { $effective } mm
ui-armor-plates-fuse_armed = 引信已触发
ui-armor-plates-detonation = 在 { $zone } 内爆炸，装甲板 #{ $number } 之后 { $distance } m
ui-armor-plates-detonation_past_armor = 在所有装甲之外爆炸，装甲板 #{ $number } 之后 { $distance } m
ui-armor-plates-flat_penetrates = { $ammo } 击穿: { $penetration } mm 对 { $thickness } mm
ui-armor-plates-flat_shatters = { $ammo } 碎裂: { $penetration } mm 对 { $thickness } mm
ui-armor-plates-server_angle = 服务器 { $angle } 度

# -- 优势等级标签 --

## advantage
advantage-absolute = 绝对
advantage-strong = 强势
advantage-moderate = 中等
advantage-weak = 弱势

## ui.search
ui-search-edit_value = 编辑值
ui-search-add_filter = 添加筛选
ui-search-pill_select = 选择
ui-search-pill_deselect = 取消选择
ui-search-pill_group_all = 以“且”分组
ui-search-pill_group_any = 以“或”分组
ui-search-pill_ungroup = 解散分组
ui-search-pill_negate = 取反
ui-search-pill_flip_connector = 切换且/或
ui-search-pill_delete = 删除
ui-search-match_count_truncated = { $count }+ 场比赛
ui-search-outcome_win = 胜利
ui-search-outcome_loss = 失败
ui-search-outcome_draw = 平局
ui-search-outcome_unknown = 未知
ui-search-bool_true = 是
ui-search-bool_false = 否
ui-search-match_count = { $count } 场比赛
ui-search-row_actions_hint = 回放操作（或右键单击该行）
ui-search-looking_up_values = 正在查找取值...
ui-search-type_a_query = 输入查询后按 Enter
ui-search-parse_failed = 无法解析该查询：{ $reason }
ui-search-searching = 正在搜索...
ui-search-failed = 搜索失败：{ $reason }
ui-search-no_matches = 没有匹配结果
ui-search-game_mode_gap_hint = 有 { $count } 场已建立索引的比赛未记录游戏模式。请在设置中重建所有回放索引以补全。
ui-search-open = 打开
ui-search-open_missing = 回放文件已不存在
ui-search-free_text = 包含 "{ $text }"
ui-search-quant_any = 任意
ui-search-quant_none = 没有
ui-search-quant_ge = { $n } 个或更多
ui-search-quant_eq = 恰好 { $n } 个
ui-search-quant_le = 至多 { $n } 个
ui-search-quant_gt = 多于 { $n } 个
ui-search-quant_lt = 少于 { $n } 个
ui-search-quant_ne = 不等于 { $n } 个
ui-search-scope_self = 自己
ui-search-scope_ally = 友方
ui-search-scope_enemy = 敌方
ui-search-scope_div = 分队

## ui.search.suggest
ui-search-suggest-preset_divmate_test_ship = 我的分队中有人使用了测试舰船
ui-search-suggest-preset_no_enemy_cv = 没有敌方航母
ui-search-suggest-preset_all_enemies_died = 所有敌人都被击沉
ui-search-suggest-preset_high_damage_enemies = 3 名或更多敌人伤害超过 10 万
ui-search-suggest-preset_stream_sniper = 包含直播狙击者
ui-search-suggest-preset_i_survived = 我存活了
ui-search-suggest-preset_i_disconnected = 我掉线了
ui-search-suggest-my_damage = 我的伤害
ui-search-suggest-enemy_ship = 敌方舰船
ui-search-suggest-allied_ship = 友方舰船
ui-search-suggest-player_in_match = 玩家参与了此比赛
ui-search-suggest-someone_in_division = 我的分队中的某人
ui-search-suggest-scope_self = 我的
ui-search-suggest-scope_ally = 友方
ui-search-suggest-scope_enemy = 敌方
ui-search-suggest-scope_division = 分队
ui-search-suggest-scope_anyone = 任意玩家

## ui.search.field
ui-search-field-outcome = 结果
ui-search-field-map = 地图
ui-search-field-class = 舰种
ui-search-field-tier = 等级
ui-search-field-date = 日期
ui-search-field-player_present = 玩家
ui-search-field-group = 回放分组
ui-search-field-stat_damage = 伤害
ui-search-field-stat_kills = 击沉
ui-search-field-stat_spotting = 侦察
ui-search-field-stat_potential = 潜在伤害
ui-search-field-stat_received = 受到伤害
ui-search-field-stat_pr = PR
ui-search-field-rating = 评级
ui-search-field-stat_survived = 存活
ui-search-field-stat_disconnected = 掉线
ui-search-field-game_type = 战斗类型
ui-search-field-game_mode = 游戏模式
ui-search-field-match_group = 比赛分组
ui-search-field-build = 构建
ui-search-field-results_available = 有战斗结果
ui-search-field-relation = 关系
ui-search-field-division = 分队
ui-search-field-name = 名称
ui-search-field-clan = 战队
ui-search-field-realm = 服务器
ui-search-field-ship = 舰船
ui-search-field-ship_index = 舰船索引
ui-search-field-nation = 国家
ui-search-field-test_ship = 测试舰船
ui-search-field-stream_sniper = 直播狙击者
ui-search-field-sniper_login = 狙击者 Twitch 登录名

## ui.search.op
ui-search-op-contains = 包含
ui-search-op-equals = 等于
ui-search-op-not_equals = 不等于
ui-search-op-eq = =
ui-search-op-ne = !=
ui-search-op-gt = >
ui-search-op-ge = >=
ui-search-op-lt = <
ui-search-op-le = <=
ui-search-op-is = 是
ui-search-op-is_not = 不是
ui-search-op-is_set = 已设置
ui-search-op-is_not_set = 未设置

## ui.search.value
ui-search-value-relation_self = 自己
ui-search-value-relation_ally = 友方
ui-search-value-relation_enemy = 敌方
ui-search-value-division_mine = 我的
ui-search-value-division_any = 任意
ui-search-value-division_none = 无
ui-search-value-class_air_carrier = 航空母舰
ui-search-value-class_battleship = 战列舰
ui-search-value-class_cruiser = 巡洋舰
ui-search-value-class_destroyer = 驱逐舰
ui-search-value-class_submarine = 潜艇
ui-search-value-class_auxiliary = 辅助舰艇
ui-search-value-rating_bad = 糟糕
ui-search-value-rating_below_average = 低于平均
ui-search-value-rating_average = 平均
ui-search-value-rating_good = 良好
ui-search-value-rating_very_good = 很好
ui-search-value-rating_great = 优秀
ui-search-value-rating_unicum = 独角兽
ui-search-value-rating_super_unicum = 超级独角兽
ui-search-value-game_mode_test = 测试
ui-search-value-game_mode_standard = 标准
ui-search-value-game_mode_singlebase = 单基地
ui-search-value-game_mode_domination = 领域争夺
ui-search-value-game_mode_tutorial = 教程
ui-search-value-game_mode_megabase = 超级基地
ui-search-value-game_mode_forts = 要塞
ui-search-value-game_mode_standard_domination = 标准领域争夺
ui-search-value-game_mode_epicenter = 中心争夺
ui-search-value-game_mode_assault_defense = 强攻防御
ui-search-value-game_mode_pve = PVE
ui-search-value-game_mode_arms_race = 军备竞赛
ui-search-value-game_mode_epicenter_ring = 中心争夺环
ui-search-value-game_mode_anti_standard = 反向标准
ui-search-value-game_mode_attack_defense = 攻防战
ui-search-value-game_mode_torpedo_beat = 鱼雷对决
ui-search-value-game_mode_team_battle_royale = 团队大逃杀
ui-search-value-game_mode_escape_to_portal = 逃向传送门
ui-search-value-game_mode_domination_asymm = 非对称领域争夺
ui-search-value-game_mode_key_battle = 关键之战
ui-search-value-game_mode_portal_2021 = 传送门 2021
ui-search-value-game_mode_team_battle_royale_2021 = 团队大逃杀 2021
ui-search-value-game_mode_convoy_event = 护航活动
ui-search-value-game_mode_convoy_airship = 飞艇护航
ui-search-value-game_mode_two_teams_battle_royale = 双队大逃杀
ui-search-value-game_mode_pinata_event = 皮纳塔活动
ui-search-value-game_mode_respawns = 重生
ui-search-value-game_mode_respawns_sectors = 重生区域

## ui.search.column
ui-search-column-date = 日期
ui-search-column-map = 地图
ui-search-column-mode = 模式
ui-search-column-ship = 我的舰船
ui-search-column-result = 结果
ui-search-column-damage = 伤害
ui-search-column-kills = 击沉
ui-search-column-pr = PR
ui-search-column-mc = MC

## ui.settings.index
ui-settings-index-heading = 回放索引
ui-settings-index-description = 目录中每个回放的可搜索记录
ui-settings-index-build = 建立索引
ui-settings-index-rebuild = 重建全部索引
ui-settings-index-rebuild_hint = 重新读取每个回放，用于修正旧版解析出错的条目。“建立索引”只读取缺失的部分。
ui-settings-index-progress = 已读取 { $done } / { $total }，{ $failed } 个无法读取
ui-settings-index-progress_skipped = 已读取 { $done } / { $total }，{ $skipped } 个已在索引中，{ $failed } 个无法读取
ui-settings-index-built = 已为 { $indexed } 个回放建立索引，{ $failed } 个无法读取
ui-settings-index-built_with_skipped = 已为 { $indexed } 个回放建立索引，{ $skipped } 个已在索引中，{ $failed } 个无法读取

## ui.settings
ui-settings-loading = 正在加载设置...
ui-settings-load_failed = 加载设置失败
ui-settings-not_set = （未设置）

## ui.settings.panel
ui-settings-panel-mcp = MCP 服务器
ui-settings-panel-open = 打开设置
ui-settings-panel-search_placeholder = 搜索设置
ui-settings-panel-count = { $count } 项设置
ui-settings-panel-no_matches = 没有与搜索匹配的设置
ui-settings-panel-close = 关闭设置
ui-settings-panel-in_error = { $name }：有设置需要处理
ui-settings-panel-general = 常规
ui-settings-panel-appearance = 外观
ui-settings-panel-game = World of Warships
ui-settings-panel-replays = 回放
ui-settings-panel-collaboration = 协作
ui-settings-panel-twitch = Twitch
ui-settings-panel-language_description = 界面语言
ui-settings-panel-data_sharing_description = 排位战和随机战结束后发送到 ShipBuilds.com 的内容
ui-settings-panel-code_integrity_description = 是否允许未经 Microsoft 签名的代码加载到本应用程序中
ui-settings-panel-check_for_updates_description = 每次启动应用程序时检查是否有新版本
ui-settings-panel-data_folder = 应用数据文件夹
ui-settings-panel-theme_description = 界面颜色。将指针指向某个选项即可预览。
ui-settings-panel-zoom_description = 所有文本和控件的大小
ui-settings-panel-zoom = 缩放
ui-settings-panel-zoom_shortcuts = { $zoom_in } 放大，{ $zoom_out } 缩小，{ $reset } 恢复为 1x
ui-settings-panel-zoom_changed = 缩放：{ $factor }x
ui-settings-panel-cache_directory = 缓存目录
ui-settings-panel-cache_directory_description = 缓存配置的存储位置
ui-settings-panel-cached_builds = 已缓存的配置
ui-settings-panel-current_path_description = 最近打开的回放文件
ui-settings-panel-show_raw_xp_description = 在结果表中添加原始 XP 列
ui-settings-panel-show_observed_damage_description = 在结果表中添加观测伤害列
ui-settings-panel-show_entity_id_description = 在结果表中添加实体 ID 列
ui-settings-panel-show_heals_description = 在结果表中添加治疗列
ui-settings-panel-auto_export_description = 将每场战斗的数据以所选格式写入所选目录中的文件
ui-settings-panel-display_name_description = 会话中其他人看到的名称
ui-settings-panel-get_token_description = 打开用于发放聊天令牌的登录页面
ui-settings-panel-token = 登录令牌
ui-settings-panel-monitored_channel_description = 要监视的频道。留空则监视您自己的频道。
ui-settings-panel-armor_defaults = 已保存的默认设置
ui-settings-panel-armor_saved_description = 从模型查看器选项中保存

## ui.settings.licenses
ui-settings-licenses-title = 关于与许可证
ui-settings-licenses-back = 返回设置
ui-settings-licenses-third_party_intro = 本应用程序包含以下列出的第三方组件。选择一个条目可显示使用它的软件包及其完整许可证文本。

## ui.settings.armor
ui-settings-armor-no_defaults = （没有已保存的默认设置）

## ui.app
ui-app-status_cache_checking = 正在检查游戏数据缓存
ui-app-status_cache_validating = 正在校验游戏数据缓存
ui-app-status_cache_downloading = 正在下载游戏数据
ui-app-status_cache_planning = 正在查询已发布的游戏数据
ui-app-drop_to_load = 松开以加载 { $file }
ui-app-drop_to_list = 松开以列出 { $folder } 中的录像
ui-app-drop_to_list_replays = 拖放以列出 { $count } 个回放
ui-app-drop_to_list_folders = 拖放以列出 { $count } 个文件夹中的回放
ui-app-drop_to_open_and_list = 拖放以打开 { $file } 并列出 { $folders }中的回放
ui-app-drop_to_list_replays_and_folders = 拖放以列出 { $count } 个回放及 { $folders }中的回放
ui-app-drop_folder_count = { $count } 个文件夹
ui-app-reading_replay = 正在读取 { $replay }
ui-app-reading_replays = 正在读取 { $count } 个回放
ui-app-status_constants_generating = 正在更新 { $version } 的游戏常量
ui-app-worker_constants = 游戏常量工作进程
ui-app-worker_live_match = 战斗监视
ui-app-worker_replay_loader = 回放加载器
ui-app-worker_restarting = { $worker }已停止，正在重启
ui-app-worker_stalled = { $worker }无响应
ui-app-worker_stopped = { $worker }已停止
ui-app-status_indexing = 正在为回放建立索引
ui-app-status_of = { $done } / { $total }
ui-app-status_tasks = { $count } 个任务正在运行
ui-app-status_show_panel = 显示面板：{ $panel }
ui-app-status_hide_panel = 隐藏面板：{ $panel }
ui-app-panel_left = 左侧
ui-app-panel_right = 右侧
ui-app-panel_bottom = 底部
ui-app-debug_build = 调试版本

## ui.palette
ui-palette-title = 命令
ui-palette-placeholder = 输入命令或搜索...
ui-palette-go_to = 转到：{ $tab }
ui-palette-advanced_search = 高级搜索...
ui-palette-set_theme = 主题：{ $theme }
ui-palette-games_i_died_in = 我被击沉的比赛
ui-palette-games_i_won = 我获胜的比赛
ui-palette-send_all_replays = 将所有回放发送到 ShipBuilds
ui-palette-send_all_replays_again = 将所有回放发送到 ShipBuilds（忽略发送记录）
ui-palette-search_players = 搜索玩家...
ui-palette-my_matches_in_ship = 我使用某舰船的比赛...
ui-palette-open_ship_in_armor_viewer = 在模型查看器中打开舰船...
ui-palette-open_ship_in_shiptool = 在 ShipTool 中打开舰船...
ui-palette-players_title = 搜索玩家
ui-palette-my_ships_title = 我使用某舰船的比赛
ui-palette-armor_ships_title = 在模型查看器中打开舰船
ui-palette-shiptool_ships_title = 在 ShipTool 中打开舰船
ui-palette-players_placeholder = 按名称搜索玩家...
ui-palette-my_ships_placeholder = 搜索您使用过的舰船...
ui-palette-armor_ships_placeholder = 按名称搜索所有舰船...
ui-palette-armor_ship_row = { $name } -- { $tier } 级{ $class }，{ $nation }
ui-palette-back_hint = Esc：返回
ui-palette-ships_loading = 正在加载舰船列表...
ui-palette-ships_unavailable = 没有舰船列表：{ $reason }
ui-palette-matches_found = { $count } 个匹配结果
ui-palette-sample_stats = 调试：在统计中显示示例数据
ui-palette-restore_stats = 调试：在统计中显示记录数据

## ui.replay.hover
ui-replay-hover-ship = 舰船
ui-replay-hover-map = 地图
ui-replay-hover-mode = 模式
ui-replay-hover-blank_mode = 空白
ui-replay-hover-played = 游戏时间
ui-replay-hover-damage = 伤害
ui-replay-hover-kills = 击沉
ui-replay-hover-result = 结果
ui-replay-hover-division = 分队
ui-replay-hover-twitch = Twitch 聊天

## ui.replay.renderer
ui-replay-renderer-export_video = 渲染为视频
ui-replay-renderer-export_clipboard = 渲染到剪贴板
ui-replay-renderer-export_saved = 视频已保存到 { $path }
ui-replay-renderer-export_copied = 视频已复制到剪贴板
ui-replay-renderer-export_not_written = 无法渲染视频：{ $reason }
ui-replay-renderer-pop_out = 在独立窗口中打开
ui-replay-renderer-export_path_unusable = 无法写入该位置
ui-replay-renderer-batch_started = 正在将 { $count } 个回放渲染到 { $dir }
ui-replay-renderer-batch_finished = 已渲染 { $written } 个回放，{ $failed } 个无法渲染
ui-replay-renderer-batch_all_written = 已渲染 { $written } 个回放
ui-replay-renderer-batch_copied = 已渲染 { $written } 个回放并放入剪贴板
ui-replay-renderer-batch_not_copied = 已渲染到 { $dir }，但剪贴板拒绝接收：{ $reason }
ui-replay-renderer-batch_progress = 正在渲染 { $done } / { $total }：{ $replay }
ui-replay-renderer-baking = 正在读取回放并重放战斗...
ui-replay-renderer-failed = 无法播放此回放
ui-replay-renderer-play = 播放
ui-replay-renderer-pause = 暂停

## ui.renderer.annotations
ui-renderer-annotations-title = 绘图工具
ui-renderer-annotations-undo = 撤销
ui-renderer-annotations-redo = 重做
ui-renderer-annotations-ship_hint = 查找舰船
ui-renderer-annotations-no_ship = 未选择舰船
ui-renderer-annotations-nib = 线条粗细
ui-renderer-annotations-thinner = 线条变细
ui-renderer-annotations-thicker = 线条变粗
ui-renderer-annotations-shortcuts_title = 键盘快捷键
ui-renderer-annotations-shortcuts_tools = 工具
ui-renderer-annotations-shortcuts_actions = 操作
ui-renderer-annotations-tool_select = 选择
ui-renderer-annotations-tool_arrow = 箭头
ui-renderer-annotations-tool_freehand = 自由绘制
ui-renderer-annotations-tool_line = 直线
ui-renderer-annotations-tool_circle = 圆形
ui-renderer-annotations-tool_rectangle = 矩形
ui-renderer-annotations-tool_triangle = 三角形
ui-renderer-annotations-tool_measurement = 测量
ui-renderer-annotations-tool_eraser = 橡皮擦
ui-renderer-annotations-action_undo = 撤销
ui-renderer-annotations-action_multi_select = 多选
ui-renderer-annotations-action_stroke_width = 线条粗细
ui-renderer-annotations-action_delete = 删除
ui-renderer-annotations-action_cancel = 取消 / 取消选择
ui-renderer-annotations-toggle_fill = 再次点击以切换填充
ui-renderer-annotations-clear_all = 全部清除
ui-renderer-annotations-edit = 编辑绘图
ui-renderer-annotations-size = 大小
ui-renderer-annotations-color = 颜色
ui-renderer-annotations-custom_ink = 自定义颜色
ui-renderer-annotations-filled = 填充
ui-renderer-annotations-locked = 主持人已锁定标注

## ui.armor.cast
ui-armor-cast-prompt = { $key }+点击舰体以添加命中点
ui-armor-cast-empty = 尚无命中点
ui-armor-cast-miss_nothing_under_pointer = 指针下没有装甲板
ui-armor-cast-miss_thickness_unknown = 此模型的装甲板厚度未知，无法投射
ui-armor-cast-miss_no_plate_on_path = 从此角度投射的炮弹弹道未穿过任何装甲板

## ui.armor.pen
ui-armor-pen-search_hint = 搜索要比较的舰船...
ui-armor-pen-add_a_ship = 在上方搜索并添加舰船以比较穿深
ui-armor-pen-hover_a_plate = 将鼠标悬停在舰船的装甲板上进行检查
ui-armor-pen-awaiting_plate = 请悬停装甲板
ui-armor-pen-clear_all = 全部清除
ui-armor-pen-remove = 从比较中移除
ui-armor-pen-firing_ship_tooltip = 使用此舰船的炮弹进行弹道投射
ui-armor-pen-use_for_trajectory = 用于弹道
ui-armor-pen-no_shells = { $ship } 没有主炮炮弹
ui-armor-pen-ifhe = IFHE
ui-armor-pen-ifhe_tooltip = 高爆弹的惯性引信使高爆弹穿深提高四分之一
ui-armor-pen-penetrates = 击穿
ui-armor-pen-shatters = 未击穿
ui-armor-pen-angle_dependent = 取决于角度
ui-armor-pen-no_data = 没有穿深数据

## ui.armor.splash
ui-armor-splash-hint = 在溅射模式下单击装甲，查看爆炸波及的区域
ui-armor-splash-half_extent = 爆炸半宽：{ $value } 模型单位
ui-armor-splash-triangles = 爆炸范围内的装甲三角形：{ $total }，击穿：{ $penetrated }
ui-armor-splash-no_zones = 此处没有重叠的溅射框
ui-armor-splash-zones = 爆炸范围内的区域
ui-armor-splash-direct_hit = 直接命中
ui-armor-splash-hp = { $value } HP
ui-armor-splash-section = 溅射区域

## ui.armor.export
ui-armor-export-hull = 舰体
ui-armor-export-contents = 内容
ui-armor-export-contents_mesh = 网格
ui-armor-export-contents_armor = 装甲
ui-armor-export-contents_both = 网格和装甲
ui-armor-export-lod = 网格细节
ui-armor-export-lod_tooltip = 0 为最高细节。装甲没有细节级别，不受影响。
ui-armor-export-res = 纹理分辨率
ui-armor-export-res_full = 原始
ui-armor-export-res_2048 = 2048 px
ui-armor-export-res_1024 = 1024 px
ui-armor-export-res_512 = 512 px
ui-armor-export-camos = 迷彩
ui-armor-export-camo_select_none = 全不选
ui-armor-export-camo_reset = 恢复默认
ui-armor-export-loading_details = 加载中...
ui-armor-export-size_estimate = 预计文件大小：{ $size }
ui-armor-export-size_breakdown = 几何体 { $geometry }（精确），纹理 { $textures }（估算）
ui-armor-export-size_incomplete = 无法估算 { $count } 个迷彩的大小，总大小可能不完整
ui-armor-export-exporting = 正在导出 { $ship }...
ui-armor-export-already_running = 已有导出任务在运行
ui-armor-export-exported = 已导出 { $ship }
ui-armor-export-exported_with_size = 已导出 { $ship }（{ $size }）
ui-armor-export-vehicle_not_found = 未找到舰船
ui-armor-export-export_failed = 导出失败：{ $error }

## ui.tactics
ui-tactics-a_peers_board = 会话中其他人共享的战术板
ui-tactics-map = 地图
ui-tactics-map_hint = 查找地图
ui-tactics-mode = 模式
ui-tactics-pick_a_map = 选择要绘制的地图
ui-tactics-drawing = 正在绘制地图...
ui-tactics-reset_view = 重置视图
ui-tactics-add_cap = 添加占领点
ui-tactics-add_cap_tooltip = 在下次单击地图时放置占领点
ui-tactics-selected_cap = 占领点 { $letter }
ui-tactics-cap_radius = { $km } km
ui-tactics-cap_team = 队伍
ui-tactics-cap_team_tooltip = 循环切换所选占领点的队伍
ui-tactics-team_neutral = 中立
ui-tactics-team_one = 队伍 1（绿色）
ui-tactics-team_two = 队伍 2（红色）
ui-tactics-delete_cap = 删除
ui-tactics-delete_cap_tooltip = 移除所选占领点
ui-tactics-cap_replay_delete_tooltip = 无法删除从回放加载的占领点
ui-tactics-cap_narrow_tooltip = 缩小所选占领点的半径
ui-tactics-cap_widen_tooltip = 增大所选占领点的半径
ui-tactics-undo_tooltip = 撤销上一次战术板更改（{ $shortcut }）
ui-tactics-redo_tooltip = 重做上一次撤销的战术板更改（{ $shortcut } 或 { $alternate }）
ui-tactics-clear_caps = 清除添加的占领点
ui-tactics-clear_added_caps_tooltip = 移除用户添加的占领点
ui-tactics-preset_name = 战术板名称
ui-tactics-preset_save = 保存
ui-tactics-preset_delete = 删除
ui-tactics-reset_board = 重置战术板
ui-tactics-reset_board_tooltip = 恢复所选模式的占领点并清除绘图
ui-tactics-preset_needs_a_name = 请先为战术板命名
ui-tactics-preset_needs_a_map = 请先为战术板选择地图
ui-tactics-preset_saved = 已保存 { $name }
ui-tactics-preset_save_failed = 无法保存战术板：{ $error }
ui-tactics-preset_load_failed = 无法打开战术板：{ $error }
ui-tactics-preset_delete_failed = 无法删除战术板：{ $error }
ui-tactics-presets = 已保存的战术板
ui-tactics-preset_open = 打开
ui-tactics-clear_drawing = 清除绘图
ui-tactics-scan_replays = 从回放填充占领点
ui-tactics-scan_replays_tooltip = 读取回放，获取其所在模式的占领点布局
ui-tactics-scan_needs_game_data = 请先加载一个游戏构建
ui-tactics-scan_running = 正在读取回放...
ui-tactics-scan_progress = 正在读取回放... { $read } / { $total }
ui-tactics-scan_done = 已读取 { $total } 个回放，发现 { $added } 个新布局
ui-tactics-session_boards_full = 会话中已打开 { $limit } 个战术板，因此此战术板保持在当前地图
ui-tactics-placing = 正在放置 { $ship }
ui-tactics-side_friendly = 友方
ui-tactics-side_enemy = 敌方
ui-tactics-ink_white = 白色墨迹
ui-tactics-ink_gray = 灰色墨迹
ui-tactics-ink_red = 红色墨迹
ui-tactics-ink_orange = 橙色墨迹
ui-tactics-ink_yellow = 黄色墨迹
ui-tactics-ink_green = 绿色墨迹
ui-tactics-ink_blue = 蓝色墨迹
ui-tactics-ink_purple = 紫色墨迹
ui-tactics-ink_pink = 粉色墨迹
ui-tactics-no_map_art = 此构建不包含该地图的图像
ui-tactics-ship_config = 舰船配置
ui-tactics-hull = 船体
ui-tactics-hull_default = 默认
ui-tactics-modifiers = 修正
ui-tactics-concealment_expert = 隐蔽专家
ui-tactics-gun_range_mod = 主炮射程改装
ui-tactics-secondary_range_mod = 副炮射程改装
ui-tactics-ship_ranges = 射程圈
ui-tactics-shape = 形状
ui-tactics-shape_size = 大小
ui-tactics-shape_size_km = 大小 (公里)
ui-tactics-shape_filled = 填充
ui-tactics-shape_smaller = 缩小形状
ui-tactics-shape_larger = 放大形状

## ui.armor.outcome
ui-armor-outcome-penetration = 击穿
ui-armor-outcome-citadel = 核心区
ui-armor-outcome-ricochet = 跳弹
ui-armor-outcome-shatter = 未击穿
ui-armor-outcome-overpenetration = 过度击穿
ui-armor-outcome-underwater = 水下
ui-armor-outcome-unknown = 无法识别（{ $raw }）
ui-armor-outcome-stopped = 停在装甲中

## ui.armor.sim
ui-armor-sim-overmatch_rules_out_ricochet = 口径碾压，不会跳弹
ui-armor-sim-always_ricochet_band = 跳弹，处于必定跳弹角度区间
ui-armor-sim-ricochet_angle_too_low = 角度过小，不可能跳弹

## ui.launcher
ui-launcher-rescan = 重新扫描
ui-launcher-add_install = 添加安装...
ui-launcher-add_install_title = World of Warships 安装目录
ui-launcher-no_installs = 未找到已安装的游戏。请添加一个，或在下方安装游戏。
ui-launcher-managed_by_steam = 由 Steam 管理
ui-launcher-checking_updates = 正在检查更新
ui-launcher-up_to_date = 已是最新
ui-launcher-update_available = 有可用更新：{ $parts }
ui-launcher-not_installed = 未安装：{ $parts }
ui-launcher-check_failed = 检查更新失败：{ $error }
ui-launcher-recheck = 重新检查
ui-launcher-download = 下载 / 更新...
ui-launcher-download_title = 构建的安装位置
ui-launcher-download_hint = 全新安装需要数十 GB。目前尚不支持增量更新，因此有变化的部分会完整重新下载。
ui-launcher-pause = 暂停
ui-launcher-cancel = 取消
ui-launcher-resume = 继续
ui-launcher-paused = 下载已暂停，已暂存的文件会保留。
ui-launcher-installed = 已安装
ui-launcher-installed_skipped = 已安装（跳过了 { $count } 个增量部分：尚不支持，已下载但未应用）
ui-launcher-download_failed = 下载失败：{ $error }
ui-launcher-update_in_place = 更新会安装到该安装本身的文件夹中。请选择 { $path } 进行更新。
ui-launcher-cancelled = 下载已取消。
ui-launcher-launch_failed = 启动失败：{ $error }
ui-launcher-use_for_toolkit = 供 Toolkit 使用
ui-launcher-install_heading = 安装游戏
ui-launcher-install_hint = 全新安装需要数十 GB。
ui-launcher-install_title = 游戏的安装位置
ui-launcher-install = 安装...
ui-launcher-checking_version = 正在检查可用版本
ui-launcher-version = 版本 { $version }
ui-launcher-version_unreported = 未报告客户端版本
ui-launcher-unavailable = 不可用：{ $error }
ui-launcher-retry = 重试

## ui.launcher.account
ui-launcher-account-sign_in = 登录
ui-launcher-account-add_title = 添加账号
ui-launcher-account-again_title = 重新登录
ui-launcher-account-memory_only = 仅在本次会话中保持登录。登录信息不会保存，因此重启后需要重新登录。
ui-launcher-account-password_note = 您的密码只会发送给 Wargaming，且绝不会被保存。
ui-launcher-account-realm_fixed = 服务器：{ $realm }
ui-launcher-account-email = 电子邮箱
ui-launcher-account-password = 密码
ui-launcher-account-code = 两步验证码
ui-launcher-account-code_required = 需要两步验证码。
ui-launcher-account-use_backup_code = 使用备用验证码
ui-launcher-account-submit_code = 提交验证码
ui-launcher-account-captcha = 此次登录需要验证码（CAPTCHA），但本启动器目前无法显示。请先通过官方 Wargaming Game Center 或在浏览器中登录 wargaming.net 一次，然后重试。

## ui.launcher.sign_in_error
ui-launcher-sign_in_error-wrong_credentials = 电子邮箱或密码错误。
ui-launcher-sign_in_error-two_factor_required = 此账号需要双重验证批准。请在 Wargaming Auth 中或使用邮件中的验证码批准登录，然后重新登录。
ui-launcher-sign_in_error-two_factor_invalid = 双重验证码错误或已过期。请输入新的验证码。
ui-launcher-sign_in_error-captcha_required = Wargaming 要求完成验证码（CAPTCHA）。请先在官方 Game Center 或 wargaming.net 登录一次，然后重试。
ui-launcher-sign_in_error-rate_limited = 登录尝试次数过多。请等待几分钟后重试。
ui-launcher-sign_in_error-account_banned = 此账号已被封禁。
ui-launcher-sign_in_error-network_unreachable = 无法连接 Wargaming。请检查您的网络连接。
ui-launcher-sign_in_error-service_unavailable = Wargaming 登录服务不可用。请稍后重试。
ui-launcher-sign_in_error-session_expired = 已保存的登录已过期。请重新登录。
ui-launcher-sign_in_error-keyring_locked = 系统密钥环已锁定。请将其解锁后重新登录。
ui-launcher-sign_in_error-not_saved = 已登录，但无法保存登录信息。重启后需要重新登录。
ui-launcher-sign_in_error-unexpected = 登录失败。详细信息请查看日志。

## ui.launcher.realm
ui-launcher-realm-eu = 欧洲
ui-launcher-realm-na = 北美
ui-launcher-realm-asia = 亚洲

## ui.launcher.runner
ui-launcher-runner-looking = 正在查找 Proton。
ui-launcher-runner-steam_proton = Steam 的 { $name }
ui-launcher-runner-umu = umu-run（{ $name }）
ui-launcher-runner-umu_default = umu-run（UMU-Proton）
ui-launcher-runner-native = 游戏以原生方式运行。
ui-launcher-runner-first_run_prefix = 首次启动会创建 Wine 前缀，需要几秒钟。
ui-launcher-runner-first_run_download = 首次启动会下载 UMU-Proton 和 Steam 运行时，约 700 MB。
ui-launcher-runner-choose = 选择 Proton 文件夹
ui-launcher-runner-automatic = 自动查找
ui-launcher-runner-detect = 重新查找
ui-launcher-runner-needed = 在 Linux 上启动需要 Proton。请安装带 Proton 的 Steam，或安装 umu-launcher。

## ui.replay.damage_share
ui-replay-damage_share-mode = 伤害占对方舰船总量的比例
ui-replay-damage_share-dealt = { $ship }造成的伤害中有{ $pct }打在了{ $other }身上。
ui-replay-damage_share-dealt_inverse = { $other }受到的伤害中有{ $pct }来自{ $ship }。
ui-replay-damage_share-received = { $ship }受到的伤害中有{ $pct }来自{ $other }。
ui-replay-damage_share-received_inverse = { $other }造成的伤害中有{ $pct }打在了{ $ship }身上。
ui-replay-damage_share-hold = 按住{ $key }以反转。
ui-replay-damage_share-release = 松开{ $key }以还原。

## ui.reindex
ui-reindex-context = { $count ->
    [one] 强制重建索引
   *[other] 强制为 { $count } 个回放重建索引
}
ui-reindex-palette_by_version = 强制重建回放索引：按版本
ui-reindex-palette_all = 强制重建回放索引：全部
ui-reindex-versions_title = 强制重建回放索引：按版本
ui-reindex-versions_placeholder = 筛选游戏版本...
ui-reindex-version_row = { $version } -- { $count } 个回放
ui-reindex-no_versions = 列表中没有回放标明游戏版本
ui-reindex-confirm_title = 强制重建回放索引
ui-reindex-confirm_ok = 重建索引
ui-reindex-confirm_count = 将重新读取 { $count } 个回放并重写其索引条目。
ui-reindex-confirm_range = 游戏时间从 { $oldest } 到 { $newest }。
ui-reindex-confirm_undated = 待读取的 { $count } 个回放没有可读的战斗日期，不在该范围内。
ui-reindex-confirm_downloads = 将先下载以下版本的游戏数据：
ui-reindex-confirm_unobtainable = 无法获取以下版本的游戏数据，因此跳过其 { $count } 个回放：
ui-reindex-confirm_no_cache_dir = 未设置游戏数据缓存目录，因此无法下载以下版本的游戏数据，跳过其 { $count } 个回放：
ui-reindex-confirm_no_build = { $count } 个回放未标明游戏构建，已跳过。
ui-reindex-confirm_no_version = { $count } 个回放未标明可读的游戏版本，已跳过。
ui-reindex-build_row = { $version }（构建 { $build }）：{ $count } 个回放
ui-reindex-nothing_listed = 列表中没有需要重建索引的回放。
ui-reindex-nothing_readable = 这些回放均无法重建索引：没有可用的游戏数据（跳过 { $count } 个）。
ui-reindex-no_game_data = 设置游戏目录之前无法读取回放。
ui-reindex-busy_index = 索引任务已在运行。
ui-reindex-busy_cache = 游戏数据任务已在运行。请在其完成后重试。
ui-reindex-planning = 正在检查可用的游戏数据...
ui-reindex-no_index = 回放索引未打开，因此未重建任何索引。
ui-reindex-done = 已为 { $indexed } 个回放重建索引；跳过 { $skipped } 个，{ $failed } 个无法读取
ui-reindex-stopped = 重建索引已停止：已重建 { $indexed } 个，跳过 { $skipped } 个，{ $failed } 个无法读取
ui-reindex-stopped_before_reading = 在读取任何回放之前已停止重建索引。

## ui.index_folder
ui-index_folder-tooltip = 为此列表中回放索引尚未包含的所有回放建立索引
ui-index_folder-confirm_title = 为回放建立索引
ui-index_folder-confirm_ok = 建立索引
ui-index_folder-confirm_count = 此列表中有 { $count } 个回放尚未加入回放索引，将读取并添加到索引中。
ui-index_folder-note_live = 已建立索引的回放会添加到搜索和玩家追踪器所读取的回放索引中。
ui-index_folder-note_folder = 已建立索引的回放会以此目录自身的来源 ({ $dir }) 添加到回放索引中。搜索和玩家追踪器的玩家列表会包含这些回放。玩家追踪器的军团列表只读取游戏自身的回放。
ui-index_folder-nothing_listed = 此列表中没有可建立索引的回放。
ui-index_folder-nothing_missing = 此列表中的所有回放均已建立索引。
ui-index_folder-unreadable_index = 无法读取回放索引。
ui-index_folder-done = 已为 { $indexed } 个回放建立索引；跳过 { $skipped } 个，{ $failed } 个无法读取
ui-index_folder-stopped = 索引已停止：已索引 { $indexed } 个，跳过 { $skipped } 个，{ $failed } 个无法读取
ui-index_folder-stopped_before_reading = 索引在读取任何回放之前已停止。

## ui.replay_folders
ui-replay_folders-install = 游戏
ui-replay_folders-watched = 已监视：新增和更改的回放会显示在此处
ui-replay_folders-unwatched = 未监视：使用刷新重新读取
ui-replay_folders-unseen = 自上次查看以来有 { $count } 个新增或更改的回放
ui-replay_folders-missing = 此目录已不存在：{ $dir }
ui-replay_folders-missing_hint = 在取消固定之前，每次启动时都会在此处打开。
ui-replay_folders-pin = 固定
ui-replay_folders-unpin = 取消固定
ui-replay_folders-refresh = 刷新
ui-replay_folders-close = 关闭
ui-replay_folders-pins_unreadable = 无法读取已固定的回放目录。
ui-replay_folders-game_data_available = 列出的回放有可用的游戏数据。
ui-replay_folders-browse = 浏览...
ui-replay_folders-recent_title = 打开回放目录
ui-replay_folders-recent_placeholder = 筛选最近的目录...
ui-replay_folders-recent_missing = 此目录已不存在，已从最近列表中移除：{ $dir }
ui-replay_folders-checking = 正在查找 { $dir }...
ui-replay_folders-indexing = 正在为 { $dir } 建立索引
ui-replay_folders-indexing_in_background = 此目录仍在建立索引
ui-replay_folders-search_while_indexing = 此目录仍在建立索引，因此结果尚不完整。完成后请重新搜索。
ui-replay_folders-source_failed = 无法为此目录的回放建立索引：{ $error }
ui-replay_folders-restart_watch = 重新开始监视
ui-replay_folders-watch_stopped = { $folder } 的监视已停止：{ $cause }。
ui-replay_folders-watch_restarting = 将在 { $seconds } 秒后重新开始（第 { $attempt } 次，共 { $attempts } 次）。
ui-replay_folders-watch_gave_up = 已重试 { $attempts } 次均失败，不再自动重试。
ui-replay_folders-watch_restart_hint = 点击红点或在标签页菜单中选择“重新开始监视”即可立即重新开始。
ui-replay_folders-watch_resumed = { $folder } 的监视已恢复。
ui-replay_folders-watch_cause_backend = 文件系统报告了错误：{ $error }
ui-replay_folders-watch_cause_gone = 目录已被移动或删除
ui-replay_folders-watch_cause_panicked = 内部错误：{ $error }
ui-replay_folders-watch_cause_exited = 监视意外结束
ui-replay_folders-dropped = 拖入
ui-replay_folders-dropped_numbered = 拖入 { $number }
ui-replay_folders-dropped_count = { $count } 个拖入的回放
ui-replay_folders-dropped_no_source = 拖入的回放按其所在目录建立索引。打开目录以搜索或建立索引。
ui-replay_folders-unreadable_replay = 无法读取：{ $error }

## ui.models
ui-models-heading = 模型
ui-models-loading = 正在加载模型...
ui-models-unavailable = 模型不可用
ui-models-search = 搜索舰船和模型...
ui-models-open = 打开
ui-models-export = 导出模型
ui-models-copy_path = 复制模型路径
ui-models-path_copied = 已复制 { $path }
ui-models-skin_replaces = 装备 { $exterior } 时替换 { $model }
ui-models-geometry_absent = 不在游戏文件中
ui-models-geometry_none = 此模型没有几何体
ui-models-hardpoints_tooltip = 显示挂载点
ui-models-common = 通用模型

## ui.models.group
ui-models-group-gameplay = 游戏
ui-models-group-building = 建筑
ui-models-group-character = 角色
ui-models-group-location = 场景
ui-models-group-port = 港口
ui-models-group-style = 风格
ui-models-group-other = 其他模型

## ui.models.building
ui-models-building-weapon = 建筑武器
ui-models-building-unassigned = 未分配建筑
ui-models-building-unspecified = 无类型建筑

## ui.models.misc
ui-models-misc-crane = 起重机
ui-models-misc-anchor = 锚
ui-models-misc-deck_fitting = 甲板属具
ui-models-misc-instrument = 仪器
ui-models-misc-propeller = 螺旋桨
ui-models-misc-searchlight = 探照灯
ui-models-misc-boat = 小艇
ui-models-misc-flag = 旗帜
ui-models-misc-insignia = 徽记
ui-models-misc-railing = 栏杆
ui-models-misc-hatch = 舱口
ui-models-misc-ventilation = 通风设备
ui-models-misc-antenna = 天线
ui-models-misc-weapon = 武器
ui-models-misc-stowage = 储物
ui-models-misc-other = 其他

## ui.models.variant
ui-models-variant-dead = 残骸
ui-models-variant-skin = 外观

## ui.models.kind
ui-models-kind-ship = 舰船
ui-models-kind-gun = 火炮
ui-models-kind-director = 射击指挥仪
ui-models-kind-finder = 测距仪
ui-models-kind-radar = 雷达
ui-models-kind-catapult = 弹射器
ui-models-kind-aircraft = 飞机
ui-models-kind-projectile = 弹药
ui-models-kind-misc = 杂项
ui-models-kind-cap = 占领点
ui-models-kind-other = 其他

## ui.models.sub_kind
ui-models-sub_kind-battleship = 战列舰
ui-models-sub_kind-cruiser = 巡洋舰
ui-models-sub_kind-destroyer = 驱逐舰
ui-models-sub_kind-aircraft_carrier = 航空母舰
ui-models-sub_kind-submarine = 潜艇
ui-models-sub_kind-auxiliary = 辅助舰
ui-models-sub_kind-main_battery = 主炮
ui-models-sub_kind-secondary_battery = 副炮
ui-models-sub_kind-anti_aircraft = 防空炮
ui-models-sub_kind-torpedo_tubes = 鱼雷发射管
ui-models-sub_kind-depth_charge_launcher = 深水炸弹投射器
ui-models-sub_kind-missile_launcher = 导弹发射器
ui-models-sub_kind-search_radar = 搜索雷达
ui-models-sub_kind-fire_control_radar = 火控雷达
ui-models-sub_kind-fighter = 战斗机
ui-models-sub_kind-bomber = 轰炸机
ui-models-sub_kind-dive_bomber = 俯冲轰炸机
ui-models-sub_kind-scout = 侦察机
ui-models-sub_kind-torpedo = 鱼雷
ui-models-sub_kind-bomb = 炸弹
ui-models-sub_kind-rocket = 火箭弹
ui-models-sub_kind-depth_charge = 深水炸弹
ui-models-sub_kind-mine = 水雷
ui-models-sub_kind-missile = 导弹
ui-models-sub_kind-jato = 起飞助推火箭
ui-models-sub_kind-shell = 炮弹

## ui.models.part
ui-models-part-hull = 舰体
ui-models-part-main_battery = 主炮
ui-models-part-secondaries = 副炮
ui-models-part-anti_aircraft = 防空
ui-models-part-torpedoes = 鱼雷
ui-models-part-directors = 射击指挥仪
ui-models-part-finders = 测距仪
ui-models-part-radars = 雷达
ui-models-part-depth_charges = 深水炸弹
ui-models-part-aircraft = 飞机
ui-models-part-other = 其他
ui-models-part-misc = 杂项

## ui.model_view
ui-model_view-thickness_from_building = 厚度来自 { $building }
ui-model_view-thickness_from = 厚度来自 { $ship } ({ $hp })
ui-model_view-thickness_unknown = 厚度未知: 没有舰船搭载此模型
ui-model_view-thickness_other_ships = 其他舰船
ui-model_view-export_title = 导出模型
ui-model_view-export_tooltip = 将模型导出为 GLB 文件
ui-model_view-part_model = 模型：{ $model }
ui-model_view-part_hardpoint = 挂载点：{ $hardpoint }

## ui.model_view.part_group
ui-model_view-part_group-hull = 舰体
ui-model_view-part_group-main_battery = 主炮
ui-model_view-part_group-secondary_battery = 副炮
ui-model_view-part_group-aa_guns = 防空
ui-model_view-part_group-torpedoes = 鱼雷
ui-model_view-part_group-depth_charges = 深水炸弹
ui-model_view-part_group-fire_control = 火控
ui-model_view-part_group-radar = 雷达
ui-model_view-part_group-missiles = 导弹发射器
ui-model_view-part_group-decorations = 装饰
ui-model_view-part_group-misc = 杂项
ui-model_view-part_group-armor = 装甲
ui-model_view-part_group-other = 其他

## ui.scene_problem
ui-scene_problem-cannot_show = 无法显示 { $name }
ui-scene_problem-geometry_absent = 此模型的几何数据不在你的游戏文件中。
ui-scene_problem-model_unreadable = 无法读取此模型的数据。详细信息见日志。
ui-scene_problem-ship_unreadable = 无法读取此舰船的数据。详细信息见日志。
ui-scene_problem-no_meshes = 此模型没有网格。
ui-scene_problem-no_meshes_at_lod = 此模型在所选细节级别下没有网格。
ui-scene_problem-no_visible_surface = 此模型没有可绘制的可见表面。
ui-scene_problem-section_stand_in = 此船体分段没有自己的几何数据。舰船在另一个分段中绘制这部分船体。

## ui.armor.option_tabs
ui-armor-option_tabs-label = 选项标签页
ui-armor-option_tabs-ship = 舰船
ui-armor-option_tabs-layers = 图层
ui-armor-option_tabs-analysis = 分析
ui-armor-option_tabs-configuration = 配置
ui-armor-option_tabs-hull_parts = 船体部件
ui-armor-option_tabs-tab_matches = { $tab }，{ $count } 项匹配
ui-armor-option_tabs-empty = 此标签页中没有适用于当前模型的内容。
ui-armor-option_tabs-no_choices = 此模型没有可选择的船体、模块、迷彩或细节级别。

## ui.armor.hidden_ship
ui-armor-hidden_ship-clan_rental = 军团租借
ui-armor-hidden_ship-other_class = 不是舰种

## ui.armor.textures
ui-armor-textures-title = 纹理
ui-armor-textures-hint = 游戏会读取该版本 res_mods 文件夹下的文件，以替代相同路径的打包文件。右键单击纹理可复制该路径。
ui-armor-textures-under_pointer = 指针下：{ $material }
ui-armor-textures-reading = 正在读取纹理...
ui-armor-textures-summary = { $materials } 个材质，{ $textures } 个纹理
ui-armor-textures-no_material = 无材质
ui-armor-textures-unresolved = 该路径不在 assets.bin 中
ui-armor-textures-absent = 不在已安装的资源包中
ui-armor-textures-unreadable = 无法读取 DDS 文件头
ui-armor-textures-unknown_format = 无法识别的格式
ui-armor-textures-details = { $format }，{ $size }，{ $count } 个文件，{ $bytes }
ui-armor-textures-role_albedo = 反照率
ui-armor-textures-role_normal = 法线
ui-armor-textures-role_metal_gloss = 金属光泽
ui-armor-textures-role_ambient_occlusion = 环境光遮蔽
ui-armor-textures-role_detail = 细节
ui-armor-textures-role_name_matched = 反照率（按文件名匹配）
ui-armor-textures-copy_path = 复制路径
ui-armor-textures-copy_overrides = { $count ->
    [one] 复制 res_mods 路径
   *[other] 复制 { $count } 个 res_mods 路径
}
ui-armor-textures-paths_copied = 已复制 { $count } 个路径
ui-armor-textures-no_build = 游戏数据尚未加载，因此无法确定该版本的 mod 文件夹。
ui-armor-textures-show_in_unpacker = 在资源解包器中显示
ui-armor-textures-show_textures = 显示纹理
ui-armor-textures-extract = 提取文件...
ui-armor-textures-extract_title = 将纹理文件提取到
ui-armor-textures-extracting = 正在提取纹理文件...
ui-armor-textures-extracted = 已将 { $count } 个文件提取到 { $folder }
ui-armor-textures-extract_failed = 无法提取纹理文件：{ $reason }
ui-armor-textures-replace_title = 替换现有文件？
ui-armor-textures-replace_description = 其中 { $count } 个文件已存在于 { $folder }。提取将替换这些文件。
ui-armor-textures-replace = 替换

## ui.settings.mcp
ui-settings-mcp-enabled = 启用 MCP 服务器
ui-settings-mcp-enabled_description = 通过 MCP 向 AI 助手提供录像和游戏数据。在状态栏显示控制按钮。
ui-settings-mcp-start_on_launch = 随程序启动
ui-settings-mcp-start_on_launch_description = 打开 WoWs Toolkit 时启动服务器。
ui-settings-mcp-bind_address = 绑定地址
ui-settings-mcp-bind_address_description = 服务器监听的环回 IP 地址，例如 127.0.0.1 或 ::1。只有这台计算机能访问服务器。
ui-settings-mcp-port = 端口
ui-settings-mcp-port_description = 监听的 TCP 端口。首次启用时随机选择并保留。
ui-settings-mcp-port_unset = 首次启用时选择
ui-settings-mcp-address_not_loopback = { $input } 不是环回地址。服务器只在这台计算机上监听，例如 127.0.0.1。
ui-settings-mcp-address_invalid = { $input } 不是 IP 地址
ui-settings-mcp-port_invalid = { $input } 不是 1 到 65535 之间的端口
ui-settings-mcp-port_in_use = 其他程序正在使用此端口。请选择其他端口。

## ui.mcp
ui-mcp-started = MCP 服务器运行于 { $url }
ui-mcp-copy_url = 复制 URL
ui-mcp-start_failed = 无法启动 MCP 服务器：{ $reason }
ui-mcp-port_in_use = { $address } 上的端口 { $port } 被其他程序占用
ui-mcp-no_watchdog = 后台任务监督程序未运行
ui-mcp-status_running = MCP 服务器运行于 { $url }。点击停止。
ui-mcp-status_starting = MCP 服务器正在 { $address } 上启动
ui-mcp-status_failed = MCP 服务器启动失败。点击重试。
ui-mcp-status_stopped = MCP 服务器已停止。点击启动。

## ui.launcher.opt_in
ui-launcher-opt_in-setting = 启用游戏启动器
ui-launcher-opt_in-description = 在 Linux 上通过 Toolkit 使用您的 Wargaming 账号登录并游玩 World of Warships。
ui-launcher-opt_in-title = 启用游戏启动器？
ui-launcher-opt_in-notice_account = 使用启动器功能即表示您知悉：此功能并非游戏开发商所支持的使用场景。您将直接在本应用程序中输入密码；如果您不信任本应用程序，它可能被用来盗取您的账号（尽管本应用程序的开发者无意这样做）。
ui-launcher-opt_in-notice_reports = 此外，您确认：如果您使用此功能，您将不会向游戏开发商报告图形或稳定性方面的错误，除非您确实认为这是一个跨平台问题，并且并非“仅”影响 Linux。
ui-launcher-opt_in-continue = 继续
ui-launcher-opt_in-cancel = 取消

## ui.launcher.keyring
ui-launcher-keyring-unencrypted = 由于没有可用的 Secret Service 提供程序（例如 GNOME Keyring、KeePassXC 或 KDE Wallet），您的登录信息以未加密方式保存在应用的数据文件夹中。
ui-launcher-keyring-how = 如何保持加密
ui-launcher-keyring-how_detail = 请安装并启动 Secret Service 提供程序，例如 GNOME Keyring、KeePassXC 或 KDE Wallet，然后重启 Toolkit。已保存的登录会迁移到其中。
ui-launcher-keyring-unlock_hint = 系统密钥环可能会要求解锁，以便 Toolkit 读取您已保存的登录。

## ui.launcher.play
ui-launcher-play-play = 开始游戏
ui-launcher-play-play_through_steam = 通过 Steam 开始游戏
ui-launcher-play-set_up_game = 设置游戏
ui-launcher-play-set_up_runner = 设置 Proton
ui-launcher-play-sign_in_again = 重新登录
ui-launcher-play-update = 更新 { $size }
ui-launcher-play-updating = 正在更新 { $percent }%
ui-launcher-play-updating_unknown = 正在更新
ui-launcher-play-paused = 已暂停
ui-launcher-play-starting = 正在启动...
ui-launcher-play-running = 运行中
ui-launcher-play-in_battle = 战斗中
ui-launcher-play-menu = 账号和游戏选项

## ui.launcher.popover
ui-launcher-popover-game = World of Warships
ui-launcher-popover-no_install = 尚未设置游戏安装。
ui-launcher-popover-update_available = 有可用更新
ui-launcher-popover-check_failed = 无法检查更新
ui-launcher-popover-accounts = 账号
ui-launcher-popover-no_accounts = 尚无账号。添加一个账号即可登录游玩。
ui-launcher-popover-close_to_switch = 请关闭游戏以切换账号。
ui-launcher-popover-default = 默认
ui-launcher-popover-ready = 就绪
ui-launcher-popover-restoring = 正在检查...
ui-launcher-popover-not_saved = 未保存
ui-launcher-popover-unencrypted = 未加密保存
ui-launcher-popover-keyring_locked = 密钥环已锁定：请将其解锁以恢复已保存的登录
ui-launcher-popover-forget = 移除
ui-launcher-popover-sign_out = 退出登录
ui-launcher-popover-expires_in = { $duration }后过期
ui-launcher-popover-expired = 会话已过期
ui-launcher-popover-expiry_unknown = 过期时间未知
ui-launcher-popover-make_default = 设为默认
ui-launcher-popover-account_actions = 账号操作
ui-launcher-popover-play_anyway = 仍然开始游戏
ui-launcher-popover-play_safe_mode = 以安全模式开始游戏
ui-launcher-popover-add_account = 添加账号...
ui-launcher-popover-game_settings = 游戏设置...
ui-launcher-popover-installs = 安装与设置...
ui-launcher-popover-open_replay = 在游戏中打开回放...

## ui.launcher.palette
ui-launcher-palette-play = 开始游戏
ui-launcher-palette-play_as = 以 { $nickname }（{ $realm }）身份开始游戏
ui-launcher-palette-game_settings = 游戏设置
ui-launcher-palette-add_account = 添加账号
ui-launcher-palette-update = 更新 World of Warships
ui-launcher-palette-open_replay = 在游戏中打开回放

## ui.launcher.installs
ui-launcher-installs-toolkit = Toolkit 的游戏目录
ui-launcher-installs-played = 默认账号使用此安装
ui-launcher-installs-play_from = 从此处开始游戏
ui-launcher-installs-update = 更新

## ui.launcher.wgc_login
ui-launcher-wgc_login-label = 使用 Wargaming 登录
ui-launcher-wgc_login-about = 通过 Toolkit 的登录（而非 Steam）运行此 Steam 安装。设置和回放与通过 Steam 运行时分开保存。此模式下 Steam 钱包、成就和游戏时间不可用。
ui-launcher-wgc_login-steam_accounts = 通过 Steam 创建的账号需要电子邮箱和密码。请先在 Wargaming 账号管理中添加。
ui-launcher-wgc_login-steam_accounts_link = 如何添加电子邮箱和密码
ui-launcher-wgc_login-ready = Game Center 文件已就绪
ui-launcher-wgc_login-fetchable = 开启后将下载此构建的 WG Game Center 文件（{ $size }）。
ui-launcher-wgc_login-downloading = 正在下载 Game Center 文件：{ $percent }%
ui-launcher-wgc_login-downloading_unknown = 正在下载 Game Center 文件
ui-launcher-wgc_login-checking = 正在查找此构建的 Game Center 文件...
ui-launcher-wgc_login-needs_sign_in = 请先向 Toolkit 添加 Wargaming 账号。
ui-launcher-wgc_login-unavailable = 此构建不可用：{ $error }

## ui.launcher.replay
ui-launcher-replay-no_install = 没有安装游戏构建 { $build }。请保留或安装该构建的副本以观看此回放。

## ui.launcher.setup
ui-launcher-setup-title = 设置 World of Warships
ui-launcher-setup-game = 游戏
ui-launcher-setup-proton = Proton
ui-launcher-setup-accounts = 账号
ui-launcher-setup-replays = 回放
ui-launcher-setup-game_question = Toolkit 应使用哪个游戏副本？
ui-launcher-setup-none_found = 在这台电脑上未找到已安装的游戏。
ui-launcher-setup-choose_folder = 选择文件夹...
ui-launcher-setup-install_new = 安装新副本...
ui-launcher-setup-proton_explain = 在 Linux 上，游戏通过 Proton 运行：已安装 Steam 时使用 Steam 的 Proton，否则使用 umu-run。
ui-launcher-setup-proton_ready = 已找到 Proton。
ui-launcher-setup-accounts_explain = 登录您游玩的 Wargaming 账号。“开始游戏”会以默认账号登录游戏。
ui-launcher-setup-replays_explain = 当回放数量达到此值时，游戏会删除最旧的回放。
ui-launcher-setup-skip = 跳过设置
ui-launcher-setup-back = 上一步
ui-launcher-setup-next = 下一步
ui-launcher-setup-done = 完成

## ui.launcher.game_settings
ui-launcher-game_settings-title = 游戏设置
ui-launcher-game_settings-build = 构建 { $build }
ui-launcher-game_settings-display = 显示
ui-launcher-game_settings-graphics = 图形
ui-launcher-game_settings-sound = 声音
ui-launcher-game_settings-replays = 回放
ui-launcher-game_settings-locked = 游戏正在运行。游戏退出时会重写这些文件，因此在此之前它们为只读。
ui-launcher-game_settings-footer = 写入游戏的偏好设置文件。会保留一份备份。
ui-launcher-game_settings-cancel = 取消
ui-launcher-game_settings-save = 保存
ui-launcher-game_settings-saved = 游戏设置已保存。
ui-launcher-game_settings-no_install = 尚未设置游戏安装。
ui-launcher-game_settings-window_mode = 窗口模式
ui-launcher-game_settings-windowed = 窗口化
ui-launcher-game_settings-borderless = 无边框
ui-launcher-game_settings-fullscreen = 全屏
ui-launcher-game_settings-monitor = 显示器
ui-launcher-game_settings-monitor_name = 显示器名称
ui-launcher-game_settings-resolution = 全屏分辨率
ui-launcher-game_settings-width = 宽度
ui-launcher-game_settings-height = 高度
ui-launcher-game_settings-refresh = 刷新率
ui-launcher-game_settings-display_rate = 显示频率
ui-launcher-game_settings-vsync = 垂直同步
ui-launcher-game_settings-ui_scale = 界面缩放
ui-launcher-game_settings-battle_fps = 战斗中帧率上限
ui-launcher-game_settings-port_fps = 港口中帧率上限
ui-launcher-game_settings-unlimited = 无限制
ui-launcher-game_settings-skip_intro = 跳过开场视频
ui-launcher-game_settings-master = 主音量
ui-launcher-game_settings-music_port = 港口音乐
ui-launcher-game_settings-music_battle = 战斗音乐
ui-launcher-game_settings-preset = 预设
ui-launcher-game_settings-antialiasing = 抗锯齿
ui-launcher-game_settings-textures = 纹理质量
ui-launcher-game_settings-filtering = 纹理过滤
ui-launcher-game_settings-shadows = 阴影质量
ui-launcher-game_settings-graphics_note = 请在游戏中更改图形设置。游戏会根据您的硬件对其进行检查。
ui-launcher-game_settings-replays_kept = 保留的回放数量
ui-launcher-game_settings-replays_kept_zero = 请至少保留一个回放。
ui-launcher-game_settings-versioned = 按游戏版本分类回放
ui-launcher-game_settings-replays_note = 写入此构建的 res_mods。安全模式不会使用它。
ui-launcher-game_settings-absent = 游戏文件中未设置
ui-launcher-game_settings-unrecognized = 未知值：{ $value }
ui-launcher-game_settings-not_a_number = { $field }：请输入数字。

## ui.settings.launcher
ui-settings-launcher-installs = 游戏安装
ui-settings-launcher-installs_description = 启动器为游戏启动和更新的安装。
ui-settings-launcher-runner = Proton (Linux)
ui-settings-launcher-runner_description = 游戏在 Linux 上的运行方式。会自动查找 Steam 的 Proton Experimental 或最新的 Proton，否则使用 umu-run。
ui-settings-launcher-launch_args = 启动参数
ui-settings-launcher-launch_args_description = 传给游戏的额外命令行参数，每行一个。
ui-settings-launcher-environment = 环境变量
ui-settings-launcher-environment_description = 游戏的环境变量，KEY=VALUE，每行一个。
ui-settings-launcher-safe_mode_args = 安全模式参数
ui-settings-launcher-safe_mode_args_description = 以安全模式启动游戏时添加的参数，每行一个。
