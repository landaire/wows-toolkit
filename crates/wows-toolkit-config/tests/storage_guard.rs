//! A test process that has not chosen a storage directory never reaches the user's.
//!
//! Its own binary: the choice is made once per process, and another test in the same process
//! could make it first.

#[test]
fn storage_without_a_choice_is_a_temporary_directory_of_the_process() {
    let storage = wows_toolkit_config::storage_dir().expect("a test build always has a storage directory");

    assert_ne!(Some(&storage), wows_toolkit_config::platform_storage_dir().as_ref(), "never the user's own");
    assert!(storage.starts_with(std::env::temp_dir()), "{} is under the temporary directory", storage.display());
    assert!(storage.is_dir());
    assert!(wows_toolkit_config::db_path().starts_with(&storage), "the database follows it");
}
