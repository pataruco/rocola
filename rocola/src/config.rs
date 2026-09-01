use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub spotify: SpotifyConfig,
    pub apple: AppleConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyConfig {
    pub client_id: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppleConfig {
    pub team_id: String,
    pub key_id: String,
    pub p8_path: PathBuf,
    #[serde(default)]
    pub music_user_token: Option<String>,
    #[serde(default)]
    pub storefront: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("couldn't read {path}: {source}. Fix or delete the file and run rocola again.")]
    Unreadable {
        path: String,
        source: std::io::Error,
    },
    // toml::de::Error's Display renders a snippet of the offending source
    // line, which can contain a secret (e.g. a token on a broken line) — so
    // the message uses `message()` (the short description) instead, never
    // the source error's own Display.
    #[error("{path} isn't valid config: {message}. Fix or delete the file and run rocola again.")]
    Invalid {
        path: String,
        message: String,
        source: Box<toml::de::Error>,
    },
    #[error(
        "couldn't write {path}: {source}. Check you can write to that folder, then run rocola again."
    )]
    Unwritable {
        path: String,
        source: std::io::Error,
    },
}

impl Config {
    #[must_use]
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rocola/config.toml")
    }

    /// `Ok(None)` means first run — the caller starts setup, not an error path.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Unreadable`] if the file exists but can't be read,
    /// or [`ConfigError::Invalid`] if it exists but isn't valid config TOML.
    pub fn load(path: &Path) -> Result<Option<Self>, ConfigError> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(ConfigError::Unreadable {
                    path: path.display().to_string(),
                    source: e,
                });
            }
        };
        toml::from_str(&text)
            .map(Some)
            .map_err(|e| ConfigError::Invalid {
                path: path.display().to_string(),
                message: e.message().to_string(),
                source: Box::new(e),
            })
    }

    /// Written 0600 at open time — never chmod-ed after the bytes land.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Unwritable`] if the parent directory, file open,
    /// or write fails.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        use std::os::unix::fs::OpenOptionsExt as _;
        use std::os::unix::fs::PermissionsExt as _;
        let wrap = |source| ConfigError::Unwritable {
            path: path.display().to_string(),
            source,
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(wrap)?;
        }
        let text = toml::to_string_pretty(self).expect("config serialises");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(wrap)?;
        // `.mode(0o600)` above only applies on creation — a pre-existing file
        // (e.g. left at 0o644 by another tool) keeps its old permissions
        // through truncate+write otherwise. fchmod the open handle (no path
        // race) before writing so secret bytes are never briefly readable
        // under looser permissions.
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(wrap)?;
        file.write_all(text.as_bytes()).map_err(wrap)
    }

    /// Spec §Storage: warn when the .p8 sits inside a git working tree.
    #[must_use]
    pub fn p8_inside_git_worktree(&self) -> bool {
        self.apple
            .p8_path
            .ancestors()
            .any(|dir| dir.join(".git").exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Config {
        Config {
            spotify: SpotifyConfig {
                client_id: "cid".into(),
                refresh_token: Some("rt".into()),
            },
            apple: AppleConfig {
                team_id: "TEAM".into(),
                key_id: "KEY".into(),
                p8_path: "/tmp/AuthKey_X.p8".into(),
                music_user_token: None,
                storefront: Some("gb".into()),
            },
        }
    }

    #[test]
    fn roundtrips_and_is_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        sample().save(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "config must be owner-read/write only");
        let loaded = Config::load(&path).unwrap().expect("config exists");
        assert_eq!(loaded.spotify.client_id, "cid");
        assert_eq!(loaded.apple.storefront.as_deref(), Some("gb"));
    }

    #[test]
    fn tightens_a_preexisting_looser_permission_file() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "stale contents").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        sample().save(&path).unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "save must tighten a pre-existing looser-permission file"
        );
        let loaded = Config::load(&path).unwrap().expect("config exists");
        assert_eq!(loaded.spotify.client_id, "cid");
    }

    #[test]
    fn missing_file_is_first_run_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            Config::load(&dir.path().join("nope.toml"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn corrupt_file_names_the_file_in_the_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "refresh_token = \"TOPSECRET123\" junk").unwrap();
        let err = Config::load(&path).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("config.toml"), "got: {message}");
        assert!(
            !message.contains("TOPSECRET123"),
            "error must not echo file contents: {message}"
        );
    }
}
