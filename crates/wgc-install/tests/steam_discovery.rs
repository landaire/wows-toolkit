use std::fs;
use std::path::Path;

use wgc_install::steam::install_dir_from_library;
use wgc_install::steam::libraries_from_root;

/// Copy the fixture tree into a tempdir and rewrite FIXTURE_ROOT to it, since
/// libraryfolders.vdf stores absolute library paths.
fn materialize(root: &Path) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/steam");
    copy_dir(&src, root);
    let vdf = root.join("steamapps/libraryfolders.vdf");
    let text = fs::read_to_string(&vdf).unwrap().replace("FIXTURE_ROOT", root.to_str().unwrap());
    fs::write(&vdf, text).unwrap();
}

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), to).unwrap();
        }
    }
}

#[test]
fn library_folders_are_parsed() {
    let dir = tempfile::tempdir().unwrap();
    materialize(dir.path());
    let libs = libraries_from_root(dir.path());
    assert_eq!(libs, vec![dir.path().to_path_buf()]);
}

#[test]
fn install_dir_resolves_from_appmanifest() {
    let dir = tempfile::tempdir().unwrap();
    materialize(dir.path());
    let install = install_dir_from_library(dir.path(), 552990).expect("resolved");
    assert!(install.ends_with("common/World of Warships"));
    assert!(install.is_dir());
}
