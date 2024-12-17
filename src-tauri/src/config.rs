use std::{env, path::Path};
use sys_info;

pub const APP_NAME: &str = "Yad";

#[derive(Debug)]
pub struct Config {
    pub os: String,
    pub user: String,
    pub download_dir: String,
    pub config_dir: String,
    pub tmp_dir: String,
    pub db_name: String,
}

impl Default for Config {
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

        let home_dir = env::var("HOME").unwrap_or_else(|_| env::var("USERPROFILE").unwrap());
        let _os: &str = &os;

        let _home_dir = Path::new(&home_dir);
        let config_dir = match _os {
            "Windows" => _home_dir
                .join("AppData")
                .join("Local")
                .join(&APP_NAME)
                .to_str()
                .unwrap_or("_")
                .to_string(),
            "Darwin" => _home_dir
                .join("Library")
                .join("Application Support")
                .join(&APP_NAME)
                .to_str()
                .unwrap_or("_")
                .to_string(),
            "Linux" => _home_dir
                .join(".config")
                .join(&APP_NAME)
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
                .to_str()
                .unwrap_or("_")
                .to_string(),
            "Darwin" | "Linux" => String::from("/tmp"),
            _ => String::from("/tmp"),
        };

        let download_dir = Path::new(&home_dir)
            .join("Downloads")
            .join(&APP_NAME)
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
