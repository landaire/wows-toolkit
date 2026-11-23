//! Toolkit logic that decides what the UI shows, independent of any UI
//! toolkit.
//!
//! The egui and zgui apps both render from these types, so a rule
//! about what to list, filter, sort or queue has one implementation rather
//! than one per front end that can drift apart.

pub mod armor;
pub mod breakdown;
pub mod bug_report;
pub mod collab;
pub mod controls;
pub mod fire_chance;
pub mod first_run;
pub mod formatting;
pub mod glyphs;
pub mod index_rows;
pub mod listing_row;
pub mod match_stats;
pub mod mcp;
pub mod name_lookup;
pub mod notices;
pub mod palette;
pub mod personal_rating;
pub mod player_tracker;
pub mod preview_dwell;
pub mod proxy;
pub mod query_bar;
pub mod rating_breakdown;
pub mod replay_export;
pub mod replay_folders;
pub mod replay_sort;
pub mod search;
pub mod search_results;
pub mod search_settings;
pub mod settings;
pub mod settings_drawer;
pub mod stats;
pub mod tactics;
pub mod text_filter;
pub mod twitch;
pub mod unpacker;
pub mod update;
pub mod upload;
pub mod video_name;
