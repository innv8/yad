// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use yad_lib::{config::Config, storage};

fn main() {
    yad_lib::run()
}
