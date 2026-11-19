use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, io};

use directories::ProjectDirs;

use crate::domain::config::Config;

/// Saving the settings failed.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("cannot write settings to {path}: {source}")]
    Write { path: PathBuf, source: io::Error },
    #[error("cannot serialize settings: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// Settings read at startup.
#[derive(Debug)]
pub struct Loaded {
    pub config: Config,
    /// Set when a settings file exists but could not be used; defaults were loaded instead.
    pub warning: Option<String>,
    /// False when the existing file could neither be used nor set aside: saving would overwrite
    /// the user's settings with defaults, so the app must not save this session.
    pub may_save: bool,
}

/// Reads and writes the settings file.
#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `config.toml` in the per-user config directory, e.g. `%APPDATA%\streamer-giveaway\config`.
    pub fn in_user_config_dir() -> Option<Self> {
        Self::in_named_config_dir("streamer-giveaway")
    }

    /// Settings of the `--demo` mode, kept apart so trying the demo never changes real settings.
    pub fn demo_in_user_config_dir() -> Option<Self> {
        ProjectDirs::from("", "", "streamer-giveaway")
            .map(|dirs| Self::at(dirs.config_dir().join("demo.toml")))
    }

    /// Folder for the giveaway logs (CSV), e.g. `%APPDATA%\streamer-giveaway\data\logs`.
    pub fn logs_dir() -> Option<PathBuf> {
        ProjectDirs::from("", "", "streamer-giveaway").map(|dirs| dirs.data_dir().join("logs"))
    }

    /// Where the app kept its settings while it was called yep-giveaway.
    pub fn legacy_user_config_dir() -> Option<Self> {
        Self::in_named_config_dir("yep-giveaway")
    }

    fn in_named_config_dir(app: &str) -> Option<Self> {
        ProjectDirs::from("", "", app).map(|dirs| Self::at(dirs.config_dir().join("config.toml")))
    }

    /// Copies the legacy settings file here once, when this store has no file yet. The legacy file
    /// is never changed. Returns a warning when a legacy file exists but could not be copied.
    pub fn adopt_legacy(&self, legacy: &ConfigStore) -> Option<String> {
        if self.path.exists() || !legacy.path.exists() {
            return None;
        }
        let copied = self
            .path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::read(&legacy.path))
            .and_then(|bytes| fs::write(&self.path, bytes));
        copied.err().map(|err| {
            format!(
                "could not take over old settings from {}: {err}",
                legacy.path.display()
            )
        })
    }

    pub fn load(&self) -> Loaded {
        let path = self.path.display();
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Self::defaults(None, true),
            // Locked, no permission, not UTF-8: the file may be fine, so leave it alone.
            Err(err) => {
                let warning = format!("cannot read {path}: {err}; settings are not saved");
                return Self::defaults(Some(warning), false);
            }
        };
        let err = match Config::from_toml(&text) {
            Ok(config) => {
                return Loaded {
                    config,
                    warning: None,
                    may_save: true,
                };
            }
            Err(err) => err,
        };
        match self.set_aside() {
            Ok(backup) => {
                let warning = format!(
                    "invalid settings in {path}, using defaults (kept a copy in {}): {err}",
                    backup.display()
                );
                Self::defaults(Some(warning), true)
            }
            Err(rename_err) => {
                let warning = format!(
                    "invalid settings in {path}, using defaults and not saving                      (could not keep a copy: {rename_err}): {err}"
                );
                Self::defaults(Some(warning), false)
            }
        }
    }

    /// Moves the settings file to a backup name that is not taken yet, so the next save does not
    /// overwrite the user's settings with defaults and an older backup survives.
    fn set_aside(&self) -> io::Result<PathBuf> {
        let mut backup = self.path.with_extension("toml.bak");
        if backup.exists() {
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            backup = self.path.with_extension(format!("toml.{secs}.bak"));
        }
        fs::rename(&self.path, &backup)?;
        Ok(backup)
    }

    pub fn save(&self, config: &Config) -> Result<(), StorageError> {
        let text = config.to_toml()?;
        let write_error = |source| StorageError::Write {
            path: self.path.clone(),
            source,
        };
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(write_error)?;
        }
        // Write a sibling file, flush it to disk and rename it over the old one, so a crash or
        // power loss never leaves half a file.
        let partial = self.path.with_extension("toml.partial");
        let mut file = fs::File::create(&partial).map_err(write_error)?;
        file.write_all(text.as_bytes()).map_err(write_error)?;
        file.sync_all().map_err(write_error)?;
        drop(file);
        fs::rename(&partial, &self.path).map_err(write_error)
    }

    fn defaults(warning: Option<String>, may_save: bool) -> Loaded {
        Loaded {
            config: Config::default(),
            warning,
            may_save,
        }
    }
}

/// Writes one giveaway log as `<dir>/giveaway-<stamp>.csv` and returns its path.
pub fn write_session_log(dir: &Path, stamp: &str, csv: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = dir.join(format!("giveaway-{stamp}.csv"));
    fs::write(&path, csv)?;
    Ok(path)
}

/// Adds one row to a log written earlier (a winner drawn after entries closed).
pub fn append_session_row(path: &Path, row: &str) -> io::Result<()> {
    use std::io::Write as _;
    fs::OpenOptions::new()
        .append(true)
        .open(path)?
        .write_all(row.as_bytes())
}

/// Giveaway logs in `dir` as `(stamp, path)`, newest first. A missing folder means no logs yet.
pub fn session_logs(dir: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut logs = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let stamp = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix("giveaway-"))
            .and_then(|name| name.strip_suffix(".csv"))
            .map(str::to_owned);
        if let Some(stamp) = stamp {
            logs.push((stamp, path));
        }
    }
    // Stamps are UTC `YYYYMMDD-HHMMSSZ`, so text order is time order.
    logs.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(logs)
}

/// Deletes every giveaway log in `dir` and returns how many it removed. Other files stay.
pub fn delete_session_logs(dir: &Path) -> io::Result<usize> {
    let logs = session_logs(dir)?;
    for (_, path) in &logs {
        fs::remove_file(path)?;
    }
    Ok(logs.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::role::Role;

    #[test]
    fn legacy_settings_are_copied_when_new_file_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = ConfigStore::at(dir.path().join("old").join("config.toml"));
        let mut config = Config::default();
        config.weights.set(Role::Vip, 7);
        legacy.save(&config).unwrap();
        let store = ConfigStore::at(dir.path().join("new").join("config.toml"));

        assert_eq!(store.adopt_legacy(&legacy), None);
        assert_eq!(store.load().config.weights.get(Role::Vip), 7);
        assert!(legacy.path.exists(), "legacy file must stay");
    }

    #[test]
    fn legacy_copy_never_overwrites_new_settings() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = ConfigStore::at(dir.path().join("old").join("config.toml"));
        let mut old = Config::default();
        old.weights.set(Role::Vip, 7);
        legacy.save(&old).unwrap();
        let store = ConfigStore::at(dir.path().join("new").join("config.toml"));
        store.save(&Config::default()).unwrap();

        assert_eq!(store.adopt_legacy(&legacy), None);
        assert_eq!(store.load().config, Config::default());
    }

    #[test]
    fn missing_legacy_file_is_not_a_problem() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = ConfigStore::at(dir.path().join("old").join("config.toml"));
        let store = ConfigStore::at(dir.path().join("new").join("config.toml"));
        assert_eq!(store.adopt_legacy(&legacy), None);
        assert!(!store.path.exists());
    }

    #[test]
    fn unreadable_legacy_file_is_skipped_with_the_cause() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_path = dir.path().join("old").join("config.toml");
        fs::create_dir_all(&legacy_path).unwrap();
        let store = ConfigStore::at(dir.path().join("new").join("config.toml"));

        let warning = store
            .adopt_legacy(&ConfigStore::at(&legacy_path))
            .expect("warning for an unreadable legacy file");
        assert!(warning.contains("old"), "{warning}");
        assert!(!store.path.exists());
    }

    #[test]
    fn session_log_is_written_under_its_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_session_log(&dir.path().join("logs"), "20261120-180311Z", "a\n").unwrap();
        assert!(path.ends_with("giveaway-20261120-180311Z.csv"));
        assert_eq!(fs::read_to_string(path).unwrap(), "a\n");
    }

    #[test]
    fn appended_rows_follow_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_session_log(dir.path(), "20261120-180311Z", "a\n").unwrap();
        append_session_row(&path, "b\n").unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "a\nb\n");
    }

    #[test]
    fn session_logs_are_listed_newest_first_and_other_files_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        for stamp in ["20261119-090000Z", "20261120-180311Z"] {
            write_session_log(dir.path(), stamp, "x").unwrap();
        }
        fs::write(dir.path().join("notes.txt"), "x").unwrap();
        let stamps: Vec<String> = session_logs(dir.path())
            .unwrap()
            .into_iter()
            .map(|(stamp, _)| stamp)
            .collect();
        assert_eq!(stamps, ["20261120-180311Z", "20261119-090000Z"]);
    }

    #[test]
    fn deleting_logs_keeps_other_files() {
        let dir = tempfile::tempdir().unwrap();
        write_session_log(dir.path(), "20261119-090000Z", "x").unwrap();
        fs::write(dir.path().join("notes.txt"), "x").unwrap();
        assert_eq!(delete_session_logs(dir.path()).unwrap(), 1);
        assert!(session_logs(dir.path()).unwrap().is_empty());
        assert!(dir.path().join("notes.txt").exists());
    }

    #[test]
    fn a_missing_logs_folder_lists_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(session_logs(&dir.path().join("logs")).unwrap().is_empty());
    }

    #[test]
    fn missing_file_gives_defaults_without_warning() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = ConfigStore::at(dir.path().join("config.toml")).load();
        assert_eq!(loaded.config, Config::default());
        assert!(loaded.warning.is_none());
    }

    #[test]
    fn saved_weight_change_is_loaded_back() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::at(dir.path().join("nested").join("config.toml"));
        let mut config = Config::default();
        config.weights.set(Role::Mod, 0);
        store.save(&config).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.config.weights.get(Role::Mod), 0);
        assert!(loaded.warning.is_none());
    }

    #[test]
    fn corrupt_file_gives_defaults_and_a_warning_with_the_cause() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "weights = [not toml").unwrap();
        let loaded = ConfigStore::at(&path).load();
        assert_eq!(loaded.config, Config::default());
        let warning = loaded.warning.expect("warning for a corrupt file");
        assert!(warning.contains("config.toml"), "{warning}");
        let backup = fs::read_to_string(dir.path().join("config.toml.bak")).unwrap();
        assert_eq!(backup, "weights = [not toml");
    }

    #[test]
    fn unreadable_path_gives_defaults_and_a_warning() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = ConfigStore::at(dir.path()).load();
        assert_eq!(loaded.config, Config::default());
        let warning = loaded.warning.expect("warning for an unreadable path");
        assert!(warning.starts_with("cannot read"), "{warning}");
        assert!(!loaded.may_save);
    }

    #[test]
    fn second_corrupt_file_keeps_the_first_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let store = ConfigStore::at(&path);
        fs::write(&path, "first = [").unwrap();
        assert!(store.load().may_save);
        fs::write(&path, "second = [").unwrap();
        assert!(store.load().may_save);
        let first = fs::read_to_string(dir.path().join("config.toml.bak")).unwrap();
        assert_eq!(first, "first = [");
        let backups = fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(backups, 2);
    }

    #[test]
    fn save_replaces_existing_file_without_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::at(dir.path().join("config.toml"));
        store.save(&Config::default()).unwrap();
        let mut config = Config::default();
        config.weights.set(Role::Vip, 9);
        store.save(&config).unwrap();
        assert_eq!(store.load().config.weights.get(Role::Vip), 9);
        let files: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn save_into_unwritable_location_reports_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("not-a-dir");
        fs::write(&blocker, "").unwrap();
        let err = ConfigStore::at(blocker.join("config.toml"))
            .save(&Config::default())
            .unwrap_err();
        assert!(err.to_string().contains("not-a-dir"), "{err}");
    }
}
