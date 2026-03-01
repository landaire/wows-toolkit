use std::io::Write;
use std::path::Path;
use std::process::Command;

use rootcause::prelude::*;

use crate::manifest::GameVersionEntry;

const APP_ID: u32 = 552990;

/// Download game data for a specific build via DepotDownloader.
/// If `entry` is Some, uses depot_id and manifest_id for a pinned download.
/// If `entry` is None, downloads the latest public branch.
pub fn download_build(
    build: u32,
    entry: Option<&GameVersionEntry>,
    data_dir: &Path,
    repo_root: &Path,
    username_override: Option<&str>,
) -> Result<(), Report> {
    // Check DepotDownloader is available
    let dd_cmd = find_depot_downloader()?;

    // Resolve Steam username
    let username = resolve_username(username_override, repo_root)?;

    // Create output directory
    let output_dir = data_dir.join("builds").join(build.to_string());
    std::fs::create_dir_all(&output_dir)
        .attach_with(|| format!("Failed to create {}", output_dir.display()))?;

    // Write filelist for selective download
    let filelist = write_filelist(data_dir)?;

    // Build command
    let mut cmd = Command::new(&dd_cmd);
    cmd.arg("-app").arg(APP_ID.to_string());

    if let Some(entry) = entry {
        cmd.arg("-depot").arg(entry.depot_id.to_string());
        cmd.arg("-manifest").arg(&entry.manifest_id);
    }

    cmd.arg("-dir").arg(&output_dir);
    cmd.arg("-filelist").arg(&filelist);
    cmd.arg("-username").arg(&username);
    cmd.arg("-remember-password");

    println!("Downloading build {build} to {}", output_dir.display());
    if let Some(entry) = entry {
        println!("  depot: {}, manifest: {}", entry.depot_id, entry.manifest_id);
    } else {
        println!("  (latest public branch)");
    }
    println!();

    let status = cmd
        .status()
        .attach_with(|| "Failed to run DepotDownloader")?;

    // Clean up filelist
    let _ = std::fs::remove_file(&filelist);

    if !status.success() {
        bail!("DepotDownloader exited with status {status}");
    }

    println!("Download complete.");
    Ok(())
}

fn find_depot_downloader() -> Result<String, Report> {
    // Try common names
    for name in &["DepotDownloader", "depotdownloader"] {
        if Command::new(name).arg("--help").output().is_ok() {
            return Ok(name.to_string());
        }
    }

    // Try dotnet tool
    if let Ok(output) = Command::new("dotnet").args(["tool", "list", "-g"]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.to_lowercase().contains("depotdownloader") {
            // dotnet tools are available on PATH when installed globally
            bail!(
                "DepotDownloader is installed as a dotnet tool but not on PATH.\n\
                 Try running: dotnet tool install -g DepotDownloader\n\
                 Then ensure ~/.dotnet/tools is in your PATH."
            );
        }
    }

    bail!(
        "DepotDownloader not found.\n\
         Install it with: dotnet tool install -g DepotDownloader"
    );
}

fn resolve_username(override_username: Option<&str>, repo_root: &Path) -> Result<String, Report> {
    if let Some(u) = override_username {
        return Ok(u.to_string());
    }

    let steam_user_file = repo_root.join(".steam-user");
    if steam_user_file.exists() {
        let user = std::fs::read_to_string(&steam_user_file)
            .attach_with(|| "Failed to read .steam-user")?
            .trim()
            .to_string();
        if !user.is_empty() {
            println!("Using saved Steam username: {user}");
            println!("(delete .steam-user to change)");
            return Ok(user);
        }
    }

    println!("World of Warships requires a Steam account to download.");
    println!("Your username will be saved to .steam-user for future runs.");
    println!();
    print!("Steam username: ");
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let username = input.trim().to_string();

    if username.is_empty() {
        bail!("No username provided");
    }

    std::fs::write(&steam_user_file, &username)
        .attach_with(|| "Failed to save .steam-user")?;

    Ok(username)
}

fn write_filelist(data_dir: &Path) -> Result<std::path::PathBuf, Report> {
    let filelist_path = data_dir.join(".filelist.tmp");
    let content = "regex:bin/\\d+/idx/.*\\.idx$\nregex:res_packages/.*\\.pkg$\n";
    std::fs::create_dir_all(data_dir)?;
    std::fs::write(&filelist_path, content)
        .attach_with(|| format!("Failed to write filelist to {}", filelist_path.display()))?;
    Ok(filelist_path)
}
