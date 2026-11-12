//! How a directory is named to the reader.

use std::path::Path;
use std::path::PathBuf;

/// The tab's name: the directory's own name, or the whole path for a root.
pub fn folder_name(root: &Path) -> String {
    match root.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => shortened_path(root),
    }
}

/// `root` with every directory above the last cut to its first letter.
pub fn shortened_path(root: &Path) -> String {
    let parts: Vec<_> = root.components().collect();
    let mut result = PathBuf::new();
    for (index, component) in parts.iter().enumerate() {
        match component {
            std::path::Component::Normal(name) if index + 1 < parts.len() => {
                if let Some(initial) = name.to_string_lossy().chars().next() {
                    result.push(initial.to_string());
                }
            }
            std::path::Component::Normal(name) => {
                let name = name.to_string_lossy();
                let chars: Vec<_> = name.chars().collect();
                if chars.len() > 24 {
                    let head: String = chars[..12].iter().collect();
                    let tail: String = chars[chars.len() - 12..].iter().collect();
                    result.push(format!("{head}..{tail}"));
                } else {
                    result.push(name.as_ref());
                }
            }
            _ => result.push(component.as_os_str()),
        }
    }
    without_verbatim_prefix(&result.display().to_string())
}

/// The path as the reader knows it, without the verbatim prefix Windows puts
/// on a canonical one.
pub fn shown_path(root: &Path) -> String {
    without_verbatim_prefix(&root.display().to_string())
}

/// `\\?\C:\x` as `C:\x`, and `\\?\UNC\server\share` as `\\server\share`.
fn without_verbatim_prefix(path: &str) -> String {
    if let Some(share) = path.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{share}");
    }

    path.strip_prefix(r"\\?\").unwrap_or(path).to_string()
}
