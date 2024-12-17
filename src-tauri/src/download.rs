use crate::{config, files, storage};
use chrono::Utc;
use reqwest::blocking::get;
use std::{fs, io::Write, path::Path};

pub fn download(
    file_url: &str,
    cfg: &config::Config,
) -> Result<files::File, Box<dyn std::error::Error>> {
    let mut file = files::File::new(file_url, cfg);

    // check whether the record exists
    match storage::search_by_url(&file.file_url, cfg) {
        Ok(record) => {
            println!("Found record from the db");
            Ok(files::File::from(record))
        }
        Err(e) => {
            println!("Failed to get record from db because {}", e);
            fs::create_dir_all(&file.destination_dir)?;

            let download_record = storage::DownloadRecord::from(file.clone());
            storage::insert_record(&download_record, cfg)?;

            // download the file
            let now = Utc::now();
            file.download_start_time = now.timestamp();
            let response = get(file_url)?;
            let content = response.bytes()?;

            // write the content to the file
            let mut downloaded_file = fs::File::create(&file.destination_path)?;
            downloaded_file.write_all(&content)?;

            let stop_time = Utc::now();
            file.download_stop_time = stop_time.timestamp();
            file.download_duration = stop_time.signed_duration_since(now).num_milliseconds();

            let path = Path::new(&file.destination_path);

            file.file_size = match fs::metadata(path) {
                Ok(metadata) => metadata.len(),
                Err(_) => 1234,
            };
            storage::update_download_record(
                &file.file_url,
                "Finished",
                file.download_stop_time,
                file.download_duration,
                cfg,
            )?;
            Ok(file)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn test_download_csv() {
        let cfg = config::Config::default();

        let file_url = "https://www.stats.govt.nz/assets/Uploads/Annual-enterprise-survey/Annual-enterprise-survey-2023-financial-year-provisional/Download-data/annual-enterprise-survey-2023-financial-year-provisional-size-bands.csv";

        let file = files::File::new(file_url, &cfg);
        let expected_file_name =
            "annual-enterprise-survey-2023-financial-year-provisional-size-bands.csv";

        println!("path: {}", &file.destination_path);

        // clean up before testing
        if Path::new(&file.destination_dir).exists() {
            fs::remove_dir_all(&file.destination_dir).expect("Failed to clean up test directory");
        }

        println!("extension: {}", &file.extension);

        let result = download(&file.file_url, &cfg);

        assert!(result.is_ok(), "Function returned an error: {:?}", result);

        let downloaded_file_path = Path::new(&file.destination_dir).join(expected_file_name);
        assert!(
            downloaded_file_path.exists(),
            "File was not downloaded to the expected dir"
        );

        // clean up after test
        fs::remove_dir_all(&file.destination_dir).expect("Failed to clean up dir after tests");

        match result {
            Ok(r) => {
                println!(
                    ":::: Finished downloading {}, duration: {}, file_size: {}",
                    &r.file_name, &r.download_duration, &r.file_size,
                );
            }
            Err(_) => println!("NO FILE WAS DOWNLOADED"),
        };
    }

    #[test]
    fn test_download_vlc() {
        let cfg = config::Config::default();
        let file_url = "https://veronanetworks.mm.fcix.net/videolan-ftp/vlc/3.0.21/macosx/vlc-3.0.21-arm64.dmg";

        let file = files::File::new(file_url, &cfg);
        let expected_file_name = "vlc-3.0.21-arm64.dmg";

        println!("path: {}", &file.destination_path);

        // clean up before testing
        if Path::new(&file.destination_dir).exists() {
            fs::remove_dir_all(&file.destination_dir).expect("Failed to clean up test directory");
        }

        println!("extension: {}", &file.extension);

        let result = download(&file.file_url, &cfg);

        assert!(result.is_ok(), "Function returned an error: {:?}", result);

        let downloaded_file_path = Path::new(&file.destination_dir).join(expected_file_name);
        assert!(
            downloaded_file_path.exists(),
            "File was not downloaded to the expected dir"
        );

        // clean up after test
        fs::remove_dir_all(&file.destination_dir).expect("Failed to clean up dir after tests");

        match result {
            Ok(r) => {
                println!(
                    ":::: Finished downloading {}, duration: {}, file_size: {}",
                    &r.file_name, &r.download_duration, &r.file_size,
                );
            }
            Err(_) => println!("NO FILE WAS DOWNLOADED"),
        };
    }
}
