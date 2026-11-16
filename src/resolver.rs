use roxmltree::{Document, Node};
use std::{
    cmp::Ordering,
    fs,
    path::{Path, PathBuf},
};

const GAME_VERSIONS_API: &str = "https://wows-monitor.com/api/game-version/";

#[derive(Clone, Debug)]
pub struct GameVersion {
    pub region: Region,
    pub client_version: String,
    pub folder_version: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Region {
    Eu,
    Na,
    Ru,
    Apac,
    Cn,
    Int,
    None,
}

impl Region {
    pub fn label(self) -> &'static str {
        match self {
            Self::Eu => "EU",
            Self::Na => "NA",
            Self::Ru => "RU",
            Self::Apac => "APAC",
            Self::Cn => "CN",
            Self::Int => "INT",
            Self::None => "Unknown",
        }
    }

    fn from_api(value: &serde_json::Value) -> Option<Self> {
        match value {
            serde_json::Value::Number(number) => match number.as_i64()? {
                0 => Some(Self::Eu),
                1 => Some(Self::Na),
                2 => Some(Self::Ru),
                3 => Some(Self::Apac),
                4 => Some(Self::Cn),
                5 => Some(Self::Int),
                -1 => Some(Self::None),
                _ => None,
            },
            serde_json::Value::String(name) => match name.to_ascii_uppercase().as_str() {
                "EU" => Some(Self::Eu),
                "NA" => Some(Self::Na),
                "RU" => Some(Self::Ru),
                "APAC" => Some(Self::Apac),
                "CN" => Some(Self::Cn),
                "INT" => Some(Self::Int),
                "NONE" => Some(Self::None),
                _ => None,
            },
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConfigurationWatcher {
    pub directory: PathBuf,
    pub directory_path: String,
    pub entry_name: String,
}

#[derive(Clone, Debug)]
pub struct ResolvedReplayFolder {
    pub path: PathBuf,
    pub region: Region,
    pub configuration_watchers: Vec<ConfigurationWatcher>,
}

#[derive(Clone, Debug)]
struct ReplaySettings {
    path_base: String,
    directory: String,
    versioned: bool,
}

#[derive(Clone, Debug)]
struct GameRootConfiguration {
    region: Region,
    client_version: Option<String>,
    folder_version: String,
    version_directory: PathBuf,
    res_directory: PathBuf,
    modded_res_directory: Option<PathBuf>,
    engine_config: ReplaySettings,
    engine_config_source: &'static str,
}

pub fn load_game_versions() -> Result<Vec<GameVersion>, String> {
    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|error| error.to_string())?
        .get(GAME_VERSIONS_API)
        .send()
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?;

    let body: serde_json::Value = response.json().map_err(|error| error.to_string())?;
    let entries = body
        .as_array()
        .ok_or_else(|| "The game version API returned an unexpected response.".to_owned())?;

    Ok(entries
        .iter()
        .filter_map(|entry| {
            Some(GameVersion {
                region: Region::from_api(entry.get("region")?)?,
                client_version: entry.get("clientVersion")?.as_str()?.to_owned(),
                folder_version: entry.get("folderVersion")?.as_str()?.to_owned(),
            })
        })
        .collect())
}

pub fn resolve_game_folder(
    game_root: &Path,
    game_versions: &[GameVersion],
) -> Result<ResolvedReplayFolder, String> {
    if !game_root.is_dir() {
        return Err(format!(
            "The selected game folder does not exist: {}",
            game_root.display()
        ));
    }

    let bin = game_root.join("bin");
    if !bin.is_dir() {
        return Err(format!(
            "No direct bin folder was found in the selected game folder: {}",
            game_root.display()
        ));
    }

    let configuration = read_game_root_configuration(game_root, &bin, game_versions)?;
    let replay_path = find_configured_replay_folder(game_root, &configuration)?;
    let watchers = create_configuration_watchers(game_root, &configuration);

    Ok(ResolvedReplayFolder {
        path: replay_path,
        region: configuration.region,
        configuration_watchers: watchers,
    })
}

fn read_game_root_configuration(
    game_root: &Path,
    bin: &Path,
    game_versions: &[GameVersion],
) -> Result<GameRootConfiguration, String> {
    let preferences_path = game_root.join("preferences.xml");
    let preferences_xml = fs::read_to_string(&preferences_path).map_err(|error| {
        format!("Detected a game folder, but could not read preferences.xml: {error}")
    })?;

    let region = read_region(&preferences_xml);
    let client_version = get_client_version(&preferences_xml, game_versions, region);
    let (folder_version, version_directory) = get_folder_version(bin, game_versions, region)?;

    let res_directory = version_directory.join("res");
    if !res_directory.is_dir() {
        return Err(format!(
            "Found bin/{folder_version}, but its direct res folder is missing."
        ));
    }
    let base_engine_config_path = res_directory.join("engine_config.xml");
    let base_engine_config = fs::read_to_string(&base_engine_config_path).map_err(|error| {
        format!("Could not read bin/{folder_version}/res/engine_config.xml: {error}")
    })?;

    let modded_res_directory = version_directory.join("res_mods");
    let modded_res_directory = modded_res_directory
        .is_dir()
        .then_some(modded_res_directory);
    let modded_config = modded_res_directory
        .as_ref()
        .and_then(|directory| fs::read_to_string(directory.join("engine_config.xml")).ok());
    let (engine_xml, engine_config_source) = match modded_config.as_deref() {
        Some(xml) => (xml, "res_mods"),
        None => (base_engine_config.as_str(), "res"),
    };
    let engine_config = read_replay_settings(engine_xml).ok_or_else(|| {
        format!(
            "Could not parse replay settings from bin/{folder_version}/{engine_config_source}/engine_config.xml."
        )
    })?;

    if engine_config.path_base != "CWD" && engine_config.path_base != "EXE_PATH" {
        return Err(format!(
            "Unsupported replays.pathBase \"{}\". Expected CWD or EXE_PATH.",
            engine_config.path_base
        ));
    }
    if engine_config.versioned && client_version.is_none() {
        eprintln!(
            "[ReplayWatcher] Replay path is versioned, but the client version is unavailable."
        );
    }

    Ok(GameRootConfiguration {
        region,
        client_version,
        folder_version,
        version_directory,
        res_directory,
        modded_res_directory,
        engine_config,
        engine_config_source,
    })
}

fn get_folder_version(
    bin: &Path,
    game_versions: &[GameVersion],
    region: Region,
) -> Result<(String, PathBuf), String> {
    let folders = fs::read_dir(bin)
        .map_err(|error| format!("Could not read the game's bin folder: {error}"))?;
    let mut candidates = Vec::new();
    for folder in folders.flatten() {
        if folder
            .file_type()
            .map(|kind| kind.is_dir())
            .unwrap_or(false)
        {
            let name = folder.file_name().to_string_lossy().into_owned();
            if !name.eq_ignore_ascii_case("clientrunner") {
                candidates.push((name, folder.path()));
            }
        }
    }

    if let Some(requested) = game_versions
        .iter()
        .find(|version| version.region == region)
        .map(|version| version.folder_version.as_str())
    {
        if let Some(found) = candidates.iter().find(|(name, _)| name == requested) {
            return Ok(found.clone());
        }
        eprintln!(
            "[ReplayWatcher] API bin folder {requested} is not installed; using an installed version."
        );
    }

    candidates.sort_by(|(left, _), (right, _)| compare_version_folders(left, right));
    candidates
        .into_iter()
        .next()
        .ok_or_else(|| "Found no version folders under the game's bin directory.".to_owned())
}

fn compare_version_folders(left: &str, right: &str) -> Ordering {
    let left_parts = left
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>();
    let right_parts = right
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>();

    match (left_parts, right_parts) {
        (Ok(left_parts), Ok(right_parts)) => {
            for index in 0..left_parts.len().max(right_parts.len()) {
                let left_part = left_parts.get(index).copied().unwrap_or_default();
                let right_part = right_parts.get(index).copied().unwrap_or_default();
                if left_part != right_part {
                    return right_part.cmp(&left_part);
                }
            }
            Ordering::Equal
        }
        _ => right.cmp(left),
    }
}

fn get_client_version(
    preferences_xml: &str,
    game_versions: &[GameVersion],
    region: Region,
) -> Option<String> {
    game_versions
        .iter()
        .find(|version| version.region == region)
        .map(|version| version.client_version.clone())
        .or_else(|| {
            preference_value(preferences_xml, "clientVersion").map(|value| value.replace(',', "."))
        })
}

fn read_region(preferences_xml: &str) -> Region {
    let server = preference_value(preferences_xml, "active_server")
        .unwrap_or_default()
        .to_uppercase();
    if server.contains("EU") {
        Region::Eu
    } else if server.contains("RU")
        || server.contains("CIS")
        || server.contains("&#208;&#154;")
        || server.contains('К')
    {
        Region::Ru
    } else if server.contains("ASIA") {
        Region::Apac
    } else if server.contains("NA") {
        Region::Na
    } else if server.contains("CN") {
        Region::Cn
    } else {
        Region::None
    }
}

fn read_replay_settings(xml: &str) -> Option<ReplaySettings> {
    let document = Document::parse(xml).ok()?;
    let replays = document
        .descendants()
        .find(|node| node.has_tag_name("replays"))?;
    let path_base = descendant_value(replays, "pathBase")?;
    let directory = descendant_value(replays, "dirPath")?;
    let versioned = descendant_value(replays, "versioned")?;

    Some(ReplaySettings {
        path_base,
        directory,
        versioned: versioned.eq_ignore_ascii_case("true"),
    })
}

fn preference_value(xml: &str, tag: &str) -> Option<String> {
    let lower_xml = xml.to_ascii_lowercase();
    let open_tag = format!("<{tag}>").to_ascii_lowercase();
    let close_tag = format!("</{tag}>").to_ascii_lowercase();
    let content_start = lower_xml.find(&open_tag)? + open_tag.len();
    let content_end = content_start + lower_xml[content_start..].find(&close_tag)?;
    let value = xml[content_start..content_end].trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn descendant_value(node: Node<'_, '_>, tag: &str) -> Option<String> {
    node.descendants()
        .find(|descendant| {
            descendant.is_element() && descendant.tag_name().name().eq_ignore_ascii_case(tag)
        })?
        .text()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn create_configuration_watchers(
    game_root: &Path,
    configuration: &GameRootConfiguration,
) -> Vec<ConfigurationWatcher> {
    let mut watchers = vec![
        ConfigurationWatcher {
            directory: game_root.to_path_buf(),
            directory_path: ".".to_owned(),
            entry_name: "preferences.xml".to_owned(),
        },
        ConfigurationWatcher {
            directory: configuration.version_directory.clone(),
            directory_path: format!("bin/{}", configuration.folder_version),
            entry_name: "res_mods".to_owned(),
        },
        ConfigurationWatcher {
            directory: configuration.res_directory.clone(),
            directory_path: format!("bin/{}/res", configuration.folder_version),
            entry_name: "engine_config.xml".to_owned(),
        },
    ];

    if let Some(directory) = &configuration.modded_res_directory {
        watchers.push(ConfigurationWatcher {
            directory: directory.clone(),
            directory_path: format!("bin/{}/res_mods", configuration.folder_version),
            entry_name: "engine_config.xml".to_owned(),
        });
    }
    watchers
}

fn find_configured_replay_folder(
    game_root: &Path,
    configuration: &GameRootConfiguration,
) -> Result<PathBuf, String> {
    let settings = &configuration.engine_config;
    let mut replay_segments = settings
        .directory
        .replace('\\', "/")
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if replay_segments
        .iter()
        .any(|segment| segment == "." || segment == "..")
    {
        return Err("The replay directory setting contains an unsafe path segment.".to_owned());
    }

    let mut version_segments = Vec::new();
    if settings.versioned {
        if let Some(client_version) = &configuration.client_version {
            version_segments.push(client_version.clone());
        }
    }
    replay_segments.extend(version_segments);

    let candidate_bases = if settings.path_base == "EXE_PATH" {
        vec![
            game_root
                .join("bin")
                .join(&configuration.folder_version)
                .join("bin32"),
            game_root
                .join("bin")
                .join(&configuration.folder_version)
                .join("bin64"),
        ]
    } else {
        vec![game_root.to_path_buf()]
    };

    let mut attempts = Vec::new();
    for base in candidate_bases {
        let mut candidate = base.clone();
        for segment in &replay_segments {
            candidate.push(segment);
        }
        if candidate.is_dir() {
            eprintln!(
                "[ReplayWatcher] Resolved replay folder from bin/{}/{}/engine_config.xml",
                configuration.folder_version, configuration.engine_config_source
            );
            return Ok(candidate);
        }
        attempts.push(candidate);
    }

    let details = attempts
        .iter()
        .map(|attempt| attempt.display().to_string())
        .collect::<Vec<_>>()
        .join("; ");
    Err(format!(
        "Could not find the replay folder configured as {}/{}. Tried: {}",
        settings.path_base, settings.directory, details
    ))
}
