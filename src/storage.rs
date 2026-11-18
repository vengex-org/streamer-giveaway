use std::path::{Path, PathBuf};
use std::{fs, io};

use directories::ProjectDirs;

use crate::domain::config::Config;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("cannot write settings to {path}: {source}")]
    Write { path: PathBuf, source: io::Error },
    #[error("cannot serialize settings: {0}")]
    Serialize(#[from] toml::ser::Error),
}

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

    pub fn path(&self) -> &Path {
        &self.path
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
            Err(err) => Self::defaults(Some(format!(
                "invalid settings in {}: {err}",
                self.path.display()
            ))),
        }
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
        fs::write(&self.path, text).map_err(write_error)
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
    }
}
