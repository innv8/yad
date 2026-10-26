//! YAD core: download engine, storage, IPC and service management shared by the
//! `yadd` daemon, the YAD window and the native-messaging host.

pub mod config;
pub mod engine;
pub mod files;
pub mod http;
pub mod ipc;
pub mod manager;
pub mod protocol;
pub mod service;
pub mod storage;
pub mod util;
