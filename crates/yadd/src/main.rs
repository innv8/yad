//! `yadd` — the YAD download service.
//!
//! ```text
//! yadd [run]                 run the service (default)
//! yadd install               start the service at login
//! yadd uninstall             stop starting it at login
//! yadd install-native-host   let the Chrome extension talk to YAD
//! yadd native-host           used by Chrome; speaks native messaging on stdio
//! ```

use yad_core::{config::Paths, service::{self, LaunchCommand}};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let daemon = LaunchCommand::current(&["run"]);
    if yadd::is_native_host_invocation(&args) {
        std::process::exit(yadd::run_native_host(daemon));
    }
    let code = match args.get(1).map(String::as_str).unwrap_or("run") {
        "run" => yadd::run_daemon(daemon),
        "install" => match service::install(&daemon, &Paths::from_env()) {
            Ok(()) => {
                println!("YAD will start at login.");
                0
            }
            Err(e) => {
                eprintln!("Couldn't install: {e}");
                1
            }
        },
        "uninstall" => match service::uninstall() {
            Ok(()) => {
                println!("YAD won't start at login any more.");
                0
            }
            Err(e) => {
                eprintln!("Couldn't uninstall: {e}");
                1
            }
        },
        "install-native-host" => yadd::install_native_host(),
        "--version" | "-V" | "version" => {
            println!("yadd {}", yad_core::config::VERSION);
            0
        }
        _ => {
            eprintln!("usage: yadd [run|install|uninstall|install-native-host|native-host|version]");
            2
        }
    };
    std::process::exit(code);
}
