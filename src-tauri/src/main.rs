// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use yad_core::service::LaunchCommand;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // The same binary is the window, the background service and Chrome's native host.
    let daemon = LaunchCommand::current(&["--daemon"]);
    if yadd::is_native_host_invocation(&args) {
        std::process::exit(yadd::run_native_host(daemon));
    }
    if args.iter().any(|a| a == "--daemon") {
        std::process::exit(yadd::run_daemon(daemon));
    }
    yad_lib::run()
}
