//! Windows path spellings for programs and readers that do not take the verbatim form.

use std::path::Path;
use std::path::PathBuf;

/// `path` without the `\\?\` prefix `std::fs::canonicalize` gives it on Windows, for showing,
/// copying or handing to a program: `\\?\C:\x` as `C:\x` and `\\?\UNC\server\share` as
/// `\\server\share`. A path the plain spelling would name differently, and any other path, is
/// returned as it is.
///
/// Only for those uses: the replay index keys folders by the verbatim spelling, which the other
/// front ends write too.
pub fn without_verbatim_prefix(path: &Path) -> PathBuf {
    path.to_str().and_then(plain_spelling).map_or_else(|| path.to_path_buf(), PathBuf::from)
}

/// A verbatim drive or share path spelled without its prefix, when that spelling means the same:
/// short enough for the classic limit, and with no name that the classic parser would alter or
/// read as a device.
fn plain_spelling(verbatim: &str) -> Option<String> {
    // The classic limit, counting the terminating null.
    const MAX_PATH: usize = 260;

    // A root is answered with its separator, which would otherwise read as an empty last name.
    let verbatim = verbatim.strip_suffix('\\').unwrap_or(verbatim);
    let (mut plain, names) = if let Some(share) = verbatim.strip_prefix(r"\\?\UNC\") {
        let mut parts = share.split('\\');
        let server = parts.next().filter(|server| !server.is_empty())?;
        let volume = parts.next().filter(|volume| !volume.is_empty())?;
        (format!(r"\\{server}\{volume}"), parts)
    } else {
        let rest = verbatim.strip_prefix(r"\\?\")?;
        let mut parts = rest.split('\\');
        let drive = parts.next()?;
        let letter = drive.strip_suffix(':')?;
        if letter.len() != 1 || !letter.chars().all(|letter| letter.is_ascii_alphabetic()) {
            return None;
        }
        (drive.to_owned(), parts)
    };

    for name in names {
        if !classic_name(name) {
            return None;
        }
        plain.push('\\');
        plain.push_str(name);
    }
    if plain.ends_with(':') {
        plain.push('\\');
    }

    (plain.encode_utf16().count() < MAX_PATH).then_some(plain)
}

/// Whether the classic parser leaves `name` as it is: not empty, not a dot entry, no trailing dot
/// or space, and not a device name.
fn classic_name(name: &str) -> bool {
    const DEVICES: [&str; 28] = [
        "CON",
        "PRN",
        "AUX",
        "NUL",
        "COM1",
        "COM2",
        "COM3",
        "COM4",
        "COM5",
        "COM6",
        "COM7",
        "COM8",
        "COM9",
        "COM\u{b9}",
        "COM\u{b2}",
        "COM\u{b3}",
        "LPT1",
        "LPT2",
        "LPT3",
        "LPT4",
        "LPT5",
        "LPT6",
        "LPT7",
        "LPT8",
        "LPT9",
        "LPT\u{b9}",
        "LPT\u{b2}",
        "LPT\u{b3}",
    ];

    if name.is_empty() || name == "." || name == ".." || name.ends_with(['.', ' ']) {
        return false;
    }

    // A device name is one with or without an extension, and with trailing spaces before it.
    let stem = name.split_once('.').map_or(name, |(stem, _)| stem).trim_end_matches(' ');

    !DEVICES.iter().any(|device| stem.eq_ignore_ascii_case(device))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_prefixes_are_removed() {
        assert_eq!(
            without_verbatim_prefix(Path::new(r"\\?\C:\Games\World of Warships\replays\a,b.wowsreplay")),
            Path::new(r"C:\Games\World of Warships\replays\a,b.wowsreplay")
        );
        assert_eq!(
            without_verbatim_prefix(Path::new(r"\\?\UNC\host\share\replays\x.wowsreplay")),
            Path::new(r"\\host\share\replays\x.wowsreplay")
        );
        assert_eq!(
            without_verbatim_prefix(Path::new(r"D:\replays\x.wowsreplay")),
            Path::new(r"D:\replays\x.wowsreplay")
        );
        assert_eq!(
            without_verbatim_prefix(Path::new(r"\\?\Volume{0b1c}\replays")),
            Path::new(r"\\?\Volume{0b1c}\replays")
        );
        assert_eq!(
            without_verbatim_prefix(Path::new(r"\\?\C:\replays\nul.wowsreplay")),
            Path::new(r"\\?\C:\replays\nul.wowsreplay")
        );
    }

    #[test]
    fn verbatim_paths_lose_their_prefix_when_the_plain_spelling_is_the_same() {
        assert_eq!(
            plain_spelling(r"\\?\C:\Games\World_of_Warships\replays").as_deref(),
            Some(r"C:\Games\World_of_Warships\replays")
        );
        assert_eq!(plain_spelling(r"\\?\C:\").as_deref(), Some(r"C:\"));
        assert_eq!(plain_spelling(r"\\?\UNC\server\share\replays").as_deref(), Some(r"\\server\share\replays"));
    }

    #[test]
    fn verbatim_paths_keep_their_prefix_when_the_plain_spelling_differs() {
        assert_eq!(plain_spelling(r"\\?\C:\replays\nul.txt"), None, "a device name");
        assert_eq!(plain_spelling("\\\\?\\C:\\replays\\COM\u{b9}"), None, "a superscript device name");
        assert_eq!(plain_spelling(r"\\?\C:\replays\trailing."), None, "a trailing dot");
        assert_eq!(plain_spelling(r"\\?\C:\replays\trailing "), None, "a trailing space");
        assert_eq!(plain_spelling(r"\\?\Volume{0b1c}\replays"), None, "a volume GUID");
        assert_eq!(plain_spelling(&format!(r"\\?\C:\{}", "a".repeat(300))), None, "past the classic limit");
        assert_eq!(plain_spelling(r"C:\replays"), None, "not verbatim");
    }
}
