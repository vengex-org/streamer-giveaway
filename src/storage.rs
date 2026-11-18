use std::path::PathBuf;
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

    /// `config.toml` in the per-user config directory, e.g. `%APPDATA%\yep-giveaway\config`.
    pub fn in_user_config_dir() -> Option<Self> {
        ProjectDirs::from("", "", "yep-giveaway")
            .map(|dirs| Self::at(dirs.config_dir().join("config.toml")))
    }

    pub fn load(&self) -> Loaded {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Self::defaults(None),
            Err(err) => {
                return Self::defaults(Some(format!("cannot read {}: {err}", self.path.display())));
            }
        };
        match Config::from_toml(&text) {
            Ok(config) => Loaded {
                config,
                warning: None,
            },
            Err(err) => Self::defaults(Some(self.set_aside_invalid(&err))),
        }
    }

    /// Moves an unreadable settings file to `<name>.bak`, so the next save does not overwrite the
    /// user's settings with defaults.
    fn set_aside_invalid(&self, err: &toml::de::Error) -> String {
        let backup = self.path.with_extension("toml.bak");
        let kept = match fs::rename(&self.path, &backup) {
            Ok(()) => format!("kept a copy in {}", backup.display()),
            Err(rename_err) => format!("could not keep a copy: {rename_err}"),
        };
        format!(
            "invalid settings in {}, using defaults ({kept}): {err}",
            self.path.display()
        )
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
        // Write a sibling file and rename it over the old one, so a crash never leaves half a file.
        let partial = self.path.with_extension("toml.partial");
        fs::write(&partial, text).map_err(write_error)?;
        fs::rename(&partial, &self.path).map_err(write_error)
    }

    fn defaults(warning: Option<String>) -> Loaded {
        Loaded {
            config: Config::default(),
            warning,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::role::Role;

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
