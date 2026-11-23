use std::path::Path;

use tokio::sync::watch;

use super::*;
use crate::services::mcp::tools::files::ReadArgs;
use crate::services::mcp::tools::index::SearchArgs;
use crate::services::mcp::tools::replays::HeaderArgs;
use crate::services::mcp::tools::replays::ReplayRef;

/// Tools over an install at `wows_dir`, with no index and no game data.
fn tools_over(wows_dir: &Path) -> ReplayTools {
    let context = ToolContext { wows_dir: Some(wows_dir.to_path_buf()), ..ToolContext::default() };
    let (_sender, receiver) = watch::channel(context);

    ReplayTools::new(receiver)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime starts")
}

fn header_of(tools: &ReplayTools, path: &str) -> Result<replays::HeaderAnswer, ToolError> {
    let args = HeaderArgs { replay: ReplayRef { arena_id: None, path: Some(path.to_owned()) } };

    runtime().block_on(replays::header(tools, args))
}

/// An install with a replays folder, and a replay-named file beside the install.
fn install() -> (tempfile::TempDir, tempfile::TempDir) {
    let install = tempfile::tempdir().expect("a temporary directory");
    std::fs::create_dir(install.path().join("replays")).expect("the replays folder is made");
    let outside = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(outside.path().join("secret.wowsreplay"), b"not yours").expect("a file is written");

    (install, outside)
}

#[test]
fn a_replay_outside_the_replay_folders_is_refused() {
    let (install, outside) = install();
    let tools = tools_over(install.path());

    let path = outside.path().join("secret.wowsreplay");
    let refused = header_of(&tools, &path.to_string_lossy());

    assert!(matches!(refused, Err(ToolError::ReplayPath { .. })), "{refused:?}");
}

#[test]
fn a_parent_component_cannot_climb_out_of_the_replay_folder() {
    let (install, outside) = install();
    let tools = tools_over(install.path());

    let climbing = install.path().join("replays").join("..").join("..").join(outside.path().file_name().unwrap());
    let climbing = climbing.join("secret.wowsreplay");
    let refused = header_of(&tools, &climbing.to_string_lossy());

    assert!(matches!(refused, Err(ToolError::ReplayPath { .. })), "{refused:?}");
}

#[cfg(unix)]
#[test]
fn a_link_inside_the_replay_folder_cannot_point_out_of_it() {
    let (install, outside) = install();
    let tools = tools_over(install.path());
    let link = install.path().join("replays").join("linked.wowsreplay");
    std::os::unix::fs::symlink(outside.path().join("secret.wowsreplay"), &link).expect("a link is made");

    let refused = header_of(&tools, &link.to_string_lossy());

    assert!(matches!(refused, Err(ToolError::ReplayPath { .. })), "{refused:?}");
}

#[test]
fn only_replay_files_are_read() {
    let (install, _outside) = install();
    let tools = tools_over(install.path());
    let notes = install.path().join("replays").join("notes.txt");
    std::fs::write(&notes, b"text").expect("a file is written");

    let refused = header_of(&tools, &notes.to_string_lossy());

    assert!(matches!(refused, Err(ToolError::NotAReplay { .. })), "{refused:?}");
}

#[test]
fn a_file_larger_than_any_replay_is_not_read() {
    let (install, _outside) = install();
    let tools = tools_over(install.path());
    let huge = install.path().join("replays").join("huge.wowsreplay");
    // Sparse, so the test writes nothing.
    std::fs::File::create(&huge).and_then(|file| file.set_len(300 * 1024 * 1024)).expect("a sparse file is made");

    let refused = header_of(&tools, &huge.to_string_lossy());

    assert!(matches!(refused, Err(ToolError::ReplayTooLarge { .. })), "{refused:?}");
}

#[test]
fn a_replay_is_named_one_way_only() {
    let (install, _outside) = install();
    let tools = tools_over(install.path());

    let both = HeaderArgs { replay: ReplayRef { arena_id: Some(1), path: Some("x.wowsreplay".to_owned()) } };
    let neither = HeaderArgs { replay: ReplayRef { arena_id: None, path: None } };

    assert!(matches!(runtime().block_on(replays::header(&tools, both)), Err(ToolError::InvalidArgument { .. })));
    assert!(matches!(runtime().block_on(replays::header(&tools, neither)), Err(ToolError::InvalidArgument { .. })));
}

#[test]
fn game_file_paths_stay_inside_the_build() {
    let (install, _outside) = install();
    let tools = tools_over(install.path());

    for path in ["../secret", "gui/../../secret", "gui\\..\\..\\secret", "C:/Windows/win.ini", "gui/a\0b"] {
        let args = ReadArgs { build: 1, path: path.to_owned(), offset: None, length: None, encoding: None };
        let refused = runtime().block_on(files::read(&tools, args));

        assert!(
            matches!(refused, Err(ToolError::ParentComponent { .. } | ToolError::ForbiddenCharacter { .. })),
            "{path:?}: {refused:?}"
        );
    }
}

#[test]
fn a_game_file_read_is_bounded() {
    let (install, _outside) = install();
    let tools = tools_over(install.path());

    for length in [0, 1024 * 1024 + 1] {
        let args = ReadArgs { build: 1, path: "gui".to_owned(), offset: None, length: Some(length), encoding: None };
        let refused = runtime().block_on(files::read(&tools, args));

        assert!(matches!(refused, Err(ToolError::InvalidArgument { .. })), "{length}: {refused:?}");
    }
}

#[test]
fn pages_are_bounded() {
    assert!(Page::new(None, Some(0), 25, 100).is_err());
    assert!(Page::new(None, Some(101), 25, 100).is_err());

    let page = Page::new(Some(2), Some(2), 25, 100).expect("a page in bounds");
    let (items, next) = page.slice(vec![0, 1, 2, 3, 4]);
    assert_eq!(items, vec![2, 3]);
    assert_eq!(next, Some(4));

    let (items, next) = page.slice(vec![0, 1, 2]);
    assert_eq!(items, vec![2]);
    assert_eq!(next, None);
}

#[test]
fn a_search_query_is_bounded_before_it_is_parsed() {
    let (install, _outside) = install();
    let tools = tools_over(install.path());
    let nested = format!("{}x{}", "(".repeat(65), ")".repeat(65));
    let long = "a".repeat(4097);

    for query in [nested, long] {
        let args = SearchArgs { query: Some(query), sort: None, ascending: false, offset: None, limit: None };
        let refused = runtime().block_on(index::search(&tools, args));

        assert!(matches!(refused, Err(ToolError::InvalidArgument { .. })), "{refused:?}");
    }
}
