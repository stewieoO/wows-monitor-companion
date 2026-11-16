use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

#[derive(Clone, Debug)]
pub struct PollTarget {
    pub directory: PathBuf,
    pub path_prefix: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryEntrySnapshot {
    pub path: String,
    pub kind: EntryKind,
    pub size: Option<u64>,
    pub last_modified_ms: Option<u128>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    File,
    Directory,
}

pub type Snapshot = HashMap<String, DirectoryEntrySnapshot>;

#[derive(Default)]
pub struct DirectoryChange {
    pub added: Vec<DirectoryEntrySnapshot>,
    pub modified: Vec<DirectoryEntrySnapshot>,
}

pub fn scan(target: &PollTarget) -> io::Result<Snapshot> {
    let mut snapshot = HashMap::new();
    for entry in fs::read_dir(&target.directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = if target.path_prefix.is_empty() {
            name
        } else {
            format!("{}/{}", target.path_prefix.trim_matches('/'), name)
        };

        let metadata = entry.metadata()?;
        let kind = if file_type.is_dir() {
            EntryKind::Directory
        } else if file_type.is_file() {
            EntryKind::File
        } else {
            continue;
        };
        let last_modified_ms = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis());

        snapshot.insert(
            path.clone(),
            DirectoryEntrySnapshot {
                path,
                kind,
                size: (kind == EntryKind::File).then_some(metadata.len()),
                last_modified_ms,
            },
        );
    }
    Ok(snapshot)
}

pub fn compare(previous: &Snapshot, current: &Snapshot) -> DirectoryChange {
    let mut change = DirectoryChange::default();
    for (path, entry) in current {
        match previous.get(path) {
            None => change.added.push(entry.clone()),
            Some(old)
                if old.kind != entry.kind
                    || old.size != entry.size
                    || old.last_modified_ms != entry.last_modified_ms =>
            {
                change.modified.push(entry.clone())
            }
            Some(_) => {}
        }
    }
    change
}

pub fn configuration_paths(watchers: &[crate::resolver::ConfigurationWatcher]) -> Vec<String> {
    watchers
        .iter()
        .map(|watcher| {
            let prefix = if watcher.directory_path == "." {
                String::new()
            } else {
                watcher.directory_path.trim_matches('/').to_owned()
            };
            if prefix.is_empty() {
                watcher.entry_name.clone()
            } else {
                format!("{prefix}/{}", watcher.entry_name)
            }
        })
        .collect()
}

pub fn poll_targets(
    replay_path: &Path,
    watchers: &[crate::resolver::ConfigurationWatcher],
) -> Vec<PollTarget> {
    let mut targets = vec![PollTarget {
        directory: replay_path.to_path_buf(),
        path_prefix: String::new(),
    }];
    targets.extend(watchers.iter().map(|watcher| PollTarget {
        directory: watcher.directory.clone(),
        path_prefix: if watcher.directory_path == "." {
            String::new()
        } else {
            watcher.directory_path.clone()
        },
    }));
    targets
}

pub fn copy_arena_info(source_folder: &Path, target_folder: &Path) -> io::Result<bool> {
    let source = source_folder.join("tempArenaInfo.json");
    let destination = target_folder.join("tempArenaInfo.json");
    if source_folder.canonicalize().ok() == target_folder.canonicalize().ok() {
        return Ok(false);
    }
    let before = fs::metadata(&source)?;
    let contents = fs::read(&source)?;
    let after = fs::metadata(&source)?;
    let before_modified = before.modified().ok();
    let after_modified = after.modified().ok();
    if before.len() != after.len() || before_modified != after_modified {
        return Ok(false);
    }
    fs::write(destination, contents)?;
    Ok(true)
}
