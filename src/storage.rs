use std::io::Write as _;
use std::path::PathBuf;
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
