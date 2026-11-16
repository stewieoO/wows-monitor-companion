use crate::{resolver, watcher};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread,
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct WorkerEnvelope {
    pub generation: u64,
    pub event: MonitorEvent,
}

#[derive(Clone, Debug)]
pub enum MonitorEvent {
    Status(String),
    Resolved { path: PathBuf, region: String },
    Copied { destination: PathBuf },
    Error(String),
}

pub fn start(
    generation: u64,
    game_folder: PathBuf,
    target_folder: PathBuf,
    sender: Sender<WorkerEnvelope>,
    stop: Arc<AtomicBool>,
) {
    thread::Builder::new()
        .name("wows-replay-poller".to_owned())
        .spawn(move || {
            run(generation, game_folder, target_folder, sender, stop);
        })
        .expect("could not start the WoWs replay polling thread");
}

fn run(
    generation: u64,
    game_folder: PathBuf,
    target_folder: PathBuf,
    sender: Sender<WorkerEnvelope>,
    stop: Arc<AtomicBool>,
) {
    emit(
        &sender,
        generation,
        MonitorEvent::Status("Loading game versions from WoWs Monitor…".to_owned()),
    );
    let game_versions = match resolver::load_game_versions() {
        Ok(versions) => {
            emit(
                &sender,
                generation,
                MonitorEvent::Status(format!("Loaded {} game version entries.", versions.len())),
            );
            versions
        }
        Err(error) => {
            emit(
                &sender,
                generation,
                MonitorEvent::Status(format!(
                    "Game version API unavailable; using installed game folders. ({error})"
                )),
            );
            Vec::new()
        }
    };

    let mut resolved = match resolver::resolve_game_folder(&game_folder, &game_versions) {
        Ok(resolved) => resolved,
        Err(error) => {
            emit(&sender, generation, MonitorEvent::Error(error));
            return;
        }
    };
    report_resolved(&sender, generation, &resolved);

    let mut targets = watcher::poll_targets(&resolved.path, &resolved.configuration_watchers);
    let mut snapshots = baseline(&targets, &sender, generation);
    let mut config_paths = watcher::configuration_paths(&resolved.configuration_watchers);
    copy_existing(&resolved.path, &target_folder, &sender, generation);
    let mut target_scan_errors = vec![false; targets.len()];

    while !stop.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(500));
        if stop.load(Ordering::Relaxed) {
            break;
        }

        let mut arena_info_changed = false;
        let mut configuration_changed = false;

        for (index, target) in targets.iter().enumerate() {
            match watcher::scan(target) {
                Ok(current) => {
                    if let Some(previous) = snapshots.get(index) {
                        let changes = watcher::compare(previous, &current);
                        if index == 0
                            && changes
                                .added
                                .iter()
                                .chain(changes.modified.iter())
                                .any(|entry| entry.path == "tempArenaInfo.json")
                        {
                            arena_info_changed = true;
                        }
                        if changes
                            .added
                            .iter()
                            .chain(changes.modified.iter())
                            .any(|entry| config_paths.iter().any(|path| path == &entry.path))
                        {
                            configuration_changed = true;
                        }
                    }
                    snapshots[index] = current;
                    if target_scan_errors[index] {
                        target_scan_errors[index] = false;
                        emit(
                            &sender,
                            generation,
                            MonitorEvent::Status(format!(
                                "Scanning recovered for {}.",
                                target.directory.display()
                            )),
                        );
                    }
                }
                Err(error) => {
                    if !target_scan_errors[index] {
                        target_scan_errors[index] = true;
                        emit(
                            &sender,
                            generation,
                            MonitorEvent::Error(format!(
                                "Could not scan {} (will retry): {error}",
                                target.directory.display()
                            )),
                        );
                    }
                }
            }
        }

        if arena_info_changed {
            match watcher::copy_arena_info(&resolved.path, &target_folder) {
                Ok(true) => emit(
                    &sender,
                    generation,
                    MonitorEvent::Copied {
                        destination: target_folder.join("tempArenaInfo.json"),
                    },
                ),
                Ok(false) => {}
                Err(error) => emit(
                    &sender,
                    generation,
                    MonitorEvent::Error(format!("Could not copy tempArenaInfo.json: {error}")),
                ),
            }
        }

        if configuration_changed {
            emit(
                &sender,
                generation,
                MonitorEvent::Status(
                    "Replay path settings changed; resolving the game folder again…".to_owned(),
                ),
            );
            sleep_interruptible(&stop, Duration::from_millis(300));
            if stop.load(Ordering::Relaxed) {
                break;
            }

            match resolver::resolve_game_folder(&game_folder, &game_versions) {
                Ok(next) => {
                    resolved = next;
                    report_resolved(&sender, generation, &resolved);
                    targets =
                        watcher::poll_targets(&resolved.path, &resolved.configuration_watchers);
                    snapshots = baseline(&targets, &sender, generation);
                    config_paths = watcher::configuration_paths(&resolved.configuration_watchers);
                    target_scan_errors = vec![false; targets.len()];
                    copy_existing(&resolved.path, &target_folder, &sender, generation);
                }
                Err(error) => emit(
                    &sender,
                    generation,
                    MonitorEvent::Error(format!(
                        "Could not update the replay path after a game configuration change: {error}"
                    )),
                ),
            }
        }
    }
}

fn baseline(
    targets: &[watcher::PollTarget],
    sender: &Sender<WorkerEnvelope>,
    generation: u64,
) -> Vec<watcher::Snapshot> {
    targets
        .iter()
        .map(|target| match watcher::scan(target) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                emit(
                    sender,
                    generation,
                    MonitorEvent::Error(format!(
                        "Could not scan {} (will retry): {error}",
                        target.directory.display()
                    )),
                );
                watcher::Snapshot::new()
            }
        })
        .collect()
}

fn report_resolved(
    sender: &Sender<WorkerEnvelope>,
    generation: u64,
    resolved: &resolver::ResolvedReplayFolder,
) {
    emit(
        sender,
        generation,
        MonitorEvent::Resolved {
            path: resolved.path.clone(),
            region: resolved.region.label().to_owned(),
        },
    );
}

fn copy_existing(
    replay_path: &Path,
    target_folder: &Path,
    sender: &Sender<WorkerEnvelope>,
    generation: u64,
) {
    let arena_info = replay_path.join("tempArenaInfo.json");
    if !arena_info.is_file() {
        return;
    }
    match watcher::copy_arena_info(replay_path, target_folder) {
        Ok(true) => emit(
            sender,
            generation,
            MonitorEvent::Copied {
                destination: target_folder.join("tempArenaInfo.json"),
            },
        ),
        Ok(false) => {}
        Err(error) => emit(
            sender,
            generation,
            MonitorEvent::Error(format!("Could not copy tempArenaInfo.json: {error}")),
        ),
    }
}

fn sleep_interruptible(stop: &AtomicBool, duration: Duration) {
    let slice = Duration::from_millis(50);
    let mut waited = Duration::ZERO;
    while waited < duration && !stop.load(Ordering::Relaxed) {
        let remaining = duration.saturating_sub(waited);
        let sleep = remaining.min(slice);
        thread::sleep(sleep);
        waited += sleep;
    }
}

fn emit(sender: &Sender<WorkerEnvelope>, generation: u64, event: MonitorEvent) {
    let _ = sender.send(WorkerEnvelope { generation, event });
}
