//! Guards the theme migration. UI code must name semantic roles
//! (`ui.sem().win`), not dark-tuned egui constants, so both themes stay
//! readable.
//!
//! Only named `Color32` constants are detectable here. Raw
//! `Color32::from_rgb(..)` is legitimate when parsing server-supplied colour,
//! so it is left to review.
//!
//! A line that genuinely needs a literal can opt out with a trailing
//! `// theme-exempt: <reason>`.

use std::fs;
use std::path::Path;
use std::path::PathBuf;

/// Constants tuned for a dark ground that break in light mode.
const BANNED: &[&str] = &[
    "Color32::WHITE",
    "Color32::BLACK",
    "Color32::GRAY",
    "Color32::DARK_GRAY",
    "Color32::LIGHT_GRAY",
    "Color32::LIGHT_RED",
    "Color32::LIGHT_GREEN",
    "Color32::LIGHT_YELLOW",
    "Color32::LIGHT_BLUE",
    "Color32::GOLD",
    "Color32::YELLOW",
    "Color32::ORANGE",
    "Color32::RED",
    "Color32::GREEN",
    "Color32::PURPLE",
    "Color32::BROWN",
    "Color32::CYAN",
    "Color32::MAGENTA",
    "Color32::KHAKI",
    "Color32::DARK_RED",
    "Color32::DARK_GREEN",
    "Color32::DARK_BLUE",
];

/// Paths that draw over imagery or a 3D scene, where the theme does not apply,
/// plus the theme module itself which necessarily names colours.
const EXEMPT_DIRS: &[&str] = &["src/ui/theme", "src/replay/renderer", "src/replay/minimap_view", "src/viewport_3d"];

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn ui_code_uses_semantic_colours() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs_files(&src, &mut files);
    assert!(!files.is_empty(), "found no source files under {}", src.display());

    let mut violations = Vec::new();

    for file in files {
        let rel = file.strip_prefix(&src).unwrap_or(&file).to_string_lossy().replace('\\', "/");
        let rel = format!("src/{rel}");
        if EXEMPT_DIRS.iter().any(|d| rel.starts_with(d)) {
            continue;
        }

        let Ok(contents) = fs::read_to_string(&file) else { continue };
        for (n, line) in contents.lines().enumerate() {
            let trimmed = line.trim_start();
            // A comment naming a constant documents it; it does not use it.
            if trimmed.starts_with("//") || line.contains("theme-exempt:") {
                continue;
            }
            for banned in BANNED {
                let Some(idx) = line.find(banned) else { continue };
                // Reject a prefix match against a longer constant name.
                let tail = &line[idx + banned.len()..];
                if tail.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                violations.push(format!("{rel}:{}: {}", n + 1, banned));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "hardcoded colours found; use ui.sem() instead, or add `// theme-exempt: <reason>`:\n{}",
        violations.join("\n")
    );
}
