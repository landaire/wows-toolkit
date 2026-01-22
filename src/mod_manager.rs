use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::sync::mpsc::Sender;

use http_body_util::BodyExt;
use parking_lot::RwLock;
use reqwest::Url;
use tokio::runtime::Runtime;
use tracing::error;

use crate::task::BackgroundTask;
use crate::task::BackgroundTaskCompletion;
use crate::task::DownloadProgress;
use crate::ui::mod_manager::ModInfo;
use crate::ui::mod_manager::ModManagerIndex;
use crate::wows_data::WorldOfWarshipsData;

#[derive(Debug)]
#[allow(dead_code)]
pub enum ModTaskCompletion {
    DatabaseLoaded(ModManagerIndex),
    ModDownloaded(ModInfo),
    ModInstalled(ModInfo),
    ModUninstalled(ModInfo),
}

pub enum ModTaskInfo {
    LoadingModDatabase,
    DownloadingMod { mod_info: ModInfo, rx: mpsc::Receiver<DownloadProgress>, last_progress: Option<DownloadProgress> },
    InstallingMod { mod_info: ModInfo, rx: mpsc::Receiver<DownloadProgress>, last_progress: Option<DownloadProgress> },
    UninstallingMod { mod_info: ModInfo, rx: mpsc::Receiver<DownloadProgress>, last_progress: Option<DownloadProgress> },
}

// Used in mod manager feature
pub fn load_mods_db() -> BackgroundTask {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mods_db = std::fs::read_to_string("../mods.toml").unwrap();
        let result = toml::from_str::<ModManagerIndex>(&mods_db)
            .context("failed to deserialize mods db")
            .map(ModTaskCompletion::DatabaseLoaded)
            .map(BackgroundTaskCompletion::from)
            .map_err(|err| err.into());

        tx.send(result).expect("failed to send mod DB result");
    });
    BackgroundTask { receiver: Some(rx), kind: ModTaskInfo::LoadingModDatabase.into() }
}

async fn download_mod_tarball(mod_info: &ModInfo, tx: Sender<DownloadProgress>) -> Result<Vec<u8>, Report> {
    use http_body::Body;

    let Ok(url_parse) = Url::parse(mod_info.meta.repo_url()) else {
        eprintln!("failed to parse repo URL: {}", mod_info.meta.repo_url());
        return Err(report!("Failed to download mod tarball"));
    };

    let mut repo_parts = url_parse.path().split('/').filter(|s| !s.is_empty());
    let Some(owner) = repo_parts.next() else {
        eprintln!("failed to get owner from repo URL: {}", mod_info.meta.repo_url());
        return Err(anyhow!("Failed to download mod tarball"));
    };
    let Some(repo) = repo_parts.next() else {
        eprintln!("failed to get repo from repo URL: {}", mod_info.meta.repo_url());
        return Err(anyhow!("Failed to download mod tarball"));
    };
    let octocrab = octocrab::instance();
    eprintln!("downloading tarball from {}/{}", owner, repo);
    eprintln!("commit: {}", mod_info.meta.commit());
    let repo = octocrab.repos(owner, repo);
    let mut response = repo.download_tarball(mod_info.meta.commit().to_string()).await?;

    if !response.status().is_success() {
        return Err(anyhow!("Failed to download mod tarball - server error"));
    }

    let body = response.body_mut();
    let total = body.size_hint().exact().unwrap_or_default() as usize;
    let mut result = Vec::with_capacity(total);

    // Iterate through all data chunks in the body
    while let Some(frame) = body.frame().await {
        match frame {
            Ok(frame) => {
                if let Some(data) = frame.data_ref() {
                    result.extend_from_slice(data);
                    let _ = tx.send(DownloadProgress { downloaded: result.len() as u64, total: total as u64 });
                }
            }
            Err(_) => Err(anyhow!("Error while downloading mod tarball"))?,
        }
    }

    Ok(result)
}

fn unpack_mod(
    tarball: &[u8],
    wows_data: Arc<RwLock<WorldOfWarshipsData>>,
    mod_info: &ModInfo,
    tx: Sender<DownloadProgress>,
) -> anyhow::Result<()> {
    let tar = flate2::read::GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(tar);

    let wows_dir = { wows_data.read().build_dir.join("res_mods") };

    let mut globs: Vec<_> = mod_info
        .meta
        .paths()
        .iter()
        .filter_map(|pat| match glob::Pattern::new(pat) {
            Ok(pat) => Some(pat),
            Err(e) => {
                eprintln!("failed to parse glob: {}", e);
                None
            }
        })
        .collect();
    if globs.is_empty() {
        eprintln!("using default glob");
        globs.push(glob::Pattern::new("*").unwrap());
    }

    let entries_count = 100;
    let mut paths = Vec::new();
    for (processed_files, entry) in archive.entries()?.enumerate() {
        scopeguard::defer! {
            let _ = tx.send(DownloadProgress {
                downloaded: processed_files as u64,
                total:  entries_count as u64,
            });
        };

        let mut entry = entry.context("entry")?;
        let mod_file_path = entry.path().context("path")?;

        // Tarballs are structured as:
        // <REPO_OWNER>-<REPO_NAME>-<REFERENCE>
        // <REPO_OWNER>-<REPO_NAME>-<REFERENCE>/<MOD_DATA>
        // pax_global_header

        // We want to strip out the first directory in the chain
        let mod_file_path = mod_file_path.components().skip(1).collect::<PathBuf>();
        if mod_file_path.components().count() == 0 {
            eprintln!("skipping repo metadata file");
            continue;
        }

        println!("processing {}", mod_file_path.display());

        // Ensure that this is a file we should be extracting
        if !globs.iter().any(|pat| pat.matches_path(&mod_file_path) || mod_file_path.starts_with(pat.as_str())) {
            eprintln!("Path did not match any glob: {}", mod_file_path.display());
            continue;
        }

        let target_path = wows_dir.join(mod_file_path);

        // Ensure that a mod didn't try to write to elsewhere on the filesystem
        if !std::path::absolute(&target_path).context("absolute path")?.starts_with(&wows_dir) {
            return Err(anyhow!("Mod tried to write to an invalid path: {}", target_path.display()));
        }

        println!("unpacking to {}", target_path.display());

        paths.push(target_path.clone());
        if entry.header().entry_type().is_dir() {
            std::fs::create_dir_all(&target_path).context(target_path.display().to_string())?;
            continue;
        }

        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent).context(parent.display().to_string())?;
        }

        println!("writing file {}", target_path.display());

        entry.unpack(target_path)?;
    }

    let _ = std::fs::File::create(wows_dir.join("PnFModsLoader.py"));

    *mod_info.mod_paths.lock() = paths;

    Ok(())
}

fn install_mod(
    runtime: Arc<Runtime>,
    wows_data: Arc<RwLock<WorldOfWarshipsData>>,
    mod_info: ModInfo,
    tx: mpsc::Sender<BackgroundTask>,
) {
    eprintln!("downloading mod");
    let (download_task_tx, download_task_rx) = mpsc::channel();
    let (download_progress_tx, download_progress_rx) = mpsc::channel();
    let _ = tx.send(BackgroundTask {
        receiver: download_task_rx.into(),
        kind: ModTaskInfo::DownloadingMod { mod_info: mod_info.clone(), rx: download_progress_rx, last_progress: None }
            .into(),
    });

    // TODO: Download pending mods in parallel?
    let tar_file = runtime.block_on(async { download_mod_tarball(&mod_info, download_progress_tx).await });
    eprintln!("downloaded");
    match tar_file {
        Ok(tar_file) => {
            download_task_tx.send(Ok(ModTaskCompletion::ModDownloaded(mod_info.clone()).into())).unwrap();

            let (install_task_tx, install_task_rx) = mpsc::channel();
            let (install_progress_tx, install_progress_rx) = mpsc::channel();
            let _ = tx.send(BackgroundTask {
                receiver: install_task_rx.into(),
                kind: if mod_info.enabled {
                    ModTaskInfo::InstallingMod {
                        mod_info: mod_info.clone(),
                        rx: install_progress_rx,
                        last_progress: None,
                    }
                } else {
                    ModTaskInfo::UninstallingMod {
                        mod_info: mod_info.clone(),
                        rx: install_progress_rx,
                        last_progress: None,
                    }
                }
                .into(),
            });

            let unpack_result = unpack_mod(&tar_file, wows_data.clone(), &mod_info, install_progress_tx);
            eprintln!("unpack res: {:?}", unpack_result);

            install_task_tx.send(Ok(ModTaskCompletion::ModInstalled(mod_info).into())).unwrap();
        }
        Err(e) => {
            eprintln!("{:?}", e);
        }
    }
}

fn uninstall_mod(
    wows_data: Arc<RwLock<WorldOfWarshipsData>>,
    mod_info: ModInfo,
    tx: mpsc::Sender<BackgroundTask>,
) -> anyhow::Result<()> {
    eprintln!("downloading mod");
    let (uninstall_task_tx, uninstall_task_rx) = mpsc::channel();
    let (uninstall_progress_tx, uninstall_progress_rx) = mpsc::channel();
    let _ = tx.send(BackgroundTask {
        receiver: uninstall_task_rx.into(),
        kind: ModTaskInfo::UninstallingMod {
            mod_info: mod_info.clone(),
            rx: uninstall_progress_rx,
            last_progress: None,
        }
        .into(),
    });

    let wows_dir = { wows_data.read().build_dir.join("res_mods") };
    let paths = { mod_info.mod_paths.lock().clone() };
    for (i, file) in paths.iter().enumerate() {
        if !file.starts_with(&wows_dir) {
            continue;
        }

        eprintln!("deleting {}", file.display());
        if file.is_dir() {
            if let Err(e) = std::fs::remove_dir_all(file) {
                error!("failed to remove directory {:?} for mod: {:?}", file, e);
            }
        } else if let Err(e) = std::fs::remove_file(file) {
            error!("failed to remove file {:?} for mod: {:?}", file, e);
        }

        let _ = uninstall_progress_tx.send(DownloadProgress { downloaded: i as u64, total: paths.len() as u64 });
    }

    uninstall_task_tx.send(Ok(ModTaskCompletion::ModUninstalled(mod_info).into())).unwrap();

    Ok(())
}

pub fn start_mod_manager_thread(
    runtime: Arc<Runtime>,
    wows_data: Arc<RwLock<WorldOfWarshipsData>>,
    receiver: mpsc::Receiver<ModInfo>,
    background_task_sender: mpsc::Sender<BackgroundTask>,
) {
    std::thread::spawn(move || {
        while let Ok(mod_info) = receiver.recv() {
            eprintln!("mod was changed: {:?}", mod_info.meta.name());

            if mod_info.enabled {
                eprintln!("installing mod: {:?}", mod_info.meta.name());
                install_mod(runtime.clone(), wows_data.clone(), mod_info.clone(), background_task_sender.clone());
            } else {
                eprintln!("uninstalling mod: {:?}", mod_info.meta.name());
                uninstall_mod(wows_data.clone(), mod_info.clone(), background_task_sender.clone())
                    .expect("failed to uninstall mod");
            }
        }
    });
}
