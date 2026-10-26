//! Paths used by YAD. All of them can be moved with `YAD_CONFIG_DIR`, which tests and
//! development builds use to keep a separate database and socket.

use std::{
    env,
    path::{Path, PathBuf},
};

pub const APP_NAME: &str = "Yad";
pub const PIPE_NAME: &str = "ke.innv8.yad";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub struct Paths {
    /// Holds the database, the socket and the native-host manifest copy.
    pub config_dir: PathBuf,
    /// `~/Downloads/Yad` unless the user picks another folder.
    pub default_download_dir: PathBuf,
}

pub fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
}

impl Paths {
    pub fn from_env() -> Self {
        let home = home_dir();
        let config_dir = match env::var_os("YAD_CONFIG_DIR") {
            Some(dir) => PathBuf::from(dir),
            None => match env::consts::OS {
                "windows" => env::var_os("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join("AppData").join("Local"))
                    .join(APP_NAME),
                "macos" => home.join("Library").join("Application Support").join(APP_NAME),
                _ => env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".config"))
                    .join(APP_NAME),
            },
        };
        // YAD_DOWNLOAD_DIR moves the default download folder; with only YAD_CONFIG_DIR set
        // (tests, development) it defaults to a folder next to the config, never ~/Downloads.
        let default_download_dir = match (env::var_os("YAD_DOWNLOAD_DIR"), env::var_os("YAD_CONFIG_DIR")) {
            (Some(d), _) => PathBuf::from(d),
            (None, Some(_)) => config_dir.join("downloads"),
            (None, None) => home.join("Downloads").join(APP_NAME),
        };
        Paths { config_dir, default_download_dir }
    }

    pub fn with_config_dir(dir: &Path) -> Self {
        Paths {
            config_dir: dir.to_path_buf(),
            default_download_dir: home_dir().join("Downloads").join(APP_NAME),
        }
    }

    pub fn db_path(&self) -> PathBuf {
        // Same file name as YAD 1.x so existing history is migrated.
        self.config_dir.join("yad.db")
    }

    /// Unix sockets have a ~104-byte path limit. A long config path falls back to the
    /// per-user runtime/temp directory, keyed by the config path so test instances differ.
    pub fn socket_path(&self) -> PathBuf {
        let preferred = self.config_dir.join("yadd.sock");
        if preferred.as_os_str().len() <= 100 {
            return preferred;
        }
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.config_dir.to_string_lossy().bytes() {
            hash = (hash ^ b as u64).wrapping_mul(0x0100_0000_01b3);
        }
        let dir = env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(env::temp_dir);
        dir.join(format!("yad-{:08x}.sock", hash as u32))
    }

    pub fn log_path(&self) -> PathBuf {
        self.config_dir.join("yadd.log")
    }

    /// Creates the config directory, readable only by the current user on Unix.
    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.config_dir, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_download_dir_ends_with_yad() {
        let p = Paths::from_env();
        assert!(p.default_download_dir.ends_with(APP_NAME));
        assert!(p.db_path().ends_with("yad.db"));
    }

    #[test]
    fn long_socket_paths_fall_back() {
        let long = Path::new("/tmp").join("x".repeat(120));
        let p = Paths::with_config_dir(&long).socket_path();
        assert!(p.as_os_str().len() < 104, "{p:?}");
        assert_ne!(p, Paths::with_config_dir(&long.join("y")).socket_path());
    }

    #[test]
    fn config_dir_can_be_overridden() {
        let p = Paths::with_config_dir(Path::new("/tmp/yad-test"));
        assert_eq!(p.socket_path(), PathBuf::from("/tmp/yad-test/yadd.sock"));
    }
}
