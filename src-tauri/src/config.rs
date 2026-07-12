//! This module is used to handle application configurations.
//! In future, the user might be able to change some of these configs.


use std::{env, path::Path};
use sys_info;

pub const APP_NAME: &str = "Yad";


/// This is the main configs struct with basic configs for the application.
#[derive(Debug, Clone)]
pub struct Config {
    /// The operating system. Windows, Linux or Darwin (MacOS)
    pub os: String,
    /// The logged in username.
    pub user: String,
    /// The downloads directory usually ~/Downloads
    pub download_dir: String,
    /// The directory where the operating system will store its data.
    pub config_dir: String,
    /// The tmp directory where file chunks are downloaded
    pub tmp_dir: String,
    /// THe name of the sqlite3 database.
    pub db_name: String,
}

impl Default for Config {
    /// The default constructor of configs. This method creates a config instance with default
    /// settings.
    ///
    /// It fills in all the fields in the `Config` struct from default values from the operating
    /// system.
    ///
    /// # Example
    /// ```ignore
    /// let cfg = config::Config::default();
    /// ```
    fn default() -> Self {
        let user = match env::var("USER").or_else(|_| env::var("USERNAME")) {
            Ok(user) => user,
            Err(e) => {
                println!("failed to get user because {}", e);
                "".to_string()
            }
        };

        let os = match sys_info::os_type() {
            Ok(os) => os,
            Err(e) => {
                println!("failed to get operating system because {}", e);
                "".to_string()
            }
        };

        let home_dir = env::var("HOME")
            .or_else(|_| env::var("USERPROFILE"))
            .unwrap_or_else(|_| {
                eprintln!("Warning: neither HOME nor USERPROFILE is set, using /tmp");
                "/tmp".to_string()
            });
        let _os: &str = &os;

        let _home_dir = Path::new(&home_dir);
        let config_dir = match _os {
            "Windows" => _home_dir
                .join("AppData")
                .join("Local")
                .join(APP_NAME)
                .to_str()
                .unwrap_or("_")
                .to_string(),
            "Darwin" => _home_dir
                .join("Library")
                .join("Application Support")
                .join(APP_NAME)
                .to_str()
                .unwrap_or("_")
                .to_string(),
            "Linux" => _home_dir
                .join(".config")
                .join(APP_NAME)
                .to_str()
                .unwrap_or("_")
                .to_string(),
            _ => String::from("~/"),
        };

        let tmp_dir = match _os {
            "Windows" => _home_dir
                .join("AppData")
                .join("Local")
                .join("Temp")
                .join(APP_NAME)
                .to_str()
                .unwrap_or("_")
                .to_string(),
            "Darwin" | "Linux" => format!("/tmp/{APP_NAME}"),
            _ => format!("/tmp/{APP_NAME}").to_string(),
        };

        let download_dir = Path::new(&home_dir)
            .join("Downloads")
            .join(APP_NAME)
            .to_str()
            .unwrap_or("_")
            .to_string();
        let db_name = format!("{}.db", APP_NAME);

        Config {
            user: user.to_string(),
            os: os.to_string(),
            download_dir,
            config_dir,
            tmp_dir,
            db_name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_has_non_empty_fields() {
        let cfg = Config::default();
        assert!(!cfg.os.is_empty(), "OS should not be empty");
        assert!(!cfg.download_dir.is_empty(), "download_dir should not be empty");
        assert!(!cfg.config_dir.is_empty(), "config_dir should not be empty");
        assert!(!cfg.tmp_dir.is_empty(), "tmp_dir should not be empty");
        assert_eq!(cfg.db_name, "Yad.db");
    }

    #[test]
    fn test_download_dir_ends_with_yad() {
        let cfg = Config::default();
        assert!(
            cfg.download_dir.ends_with("Yad") || cfg.download_dir.ends_with("yad"),
            "download_dir should end with Yad: {}",
            cfg.download_dir
        );
    }

    #[test]
    fn test_tmp_dir_is_absolute() {
        let cfg = Config::default();
        let p = Path::new(&cfg.tmp_dir);
        assert!(p.is_absolute(), "tmp_dir should be absolute: {}", cfg.tmp_dir);
    }
}


