//! Starting the daemon at login and registering the browser native-messaging host.
//!
//! - macOS: a LaunchAgent in `~/Library/LaunchAgents`.
//! - Linux: a systemd user unit.
//! - Windows: a per-user logon task. A system service would run as SYSTEM and create files
//!   owned by the wrong user, so it isn't used.

use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
};

use crate::config::{home_dir, Paths, PIPE_NAME};

pub const LABEL: &str = "ke.innv8.yad";
/// ID of the unpacked development build (pinned by the `key` in the extension manifest).
pub const DEV_EXTENSION_ID: &str = "nhclkmkhoglkcoeiijoonmdmfmgigdhp";

/// How to start the daemon: the `yadd` binary, or the app binary with `--daemon`.
#[derive(Debug, Clone)]
pub struct LaunchCommand {
    pub exe: PathBuf,
    pub args: Vec<String>,
}

impl LaunchCommand {
    pub fn current(args: &[&str]) -> LaunchCommand {
        LaunchCommand {
            exe: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("yadd")),
            args: args.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Starts the daemon detached from the caller, so closing the window doesn't stop it.
    pub fn spawn_detached(&self) -> io::Result<()> {
        let mut cmd = Command::new(&self.exe);
        cmd.args(&self.args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
        }
        cmd.spawn().map(|_| ())
    }
}

fn run(cmd: &mut Command) -> io::Result<bool> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output()?;
    if !out.status.success() {
        log::warn!("{:?} failed: {}", cmd, String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.status.success())
}

#[cfg(target_os = "macos")]
fn agent_path() -> PathBuf {
    home_dir().join("Library/LaunchAgents").join(format!("{LABEL}.plist"))
}

#[cfg(target_os = "macos")]
fn uid() -> String {
    Command::new("id")
        .arg("-u")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "501".into())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn install(cmd: &LaunchCommand, paths: &Paths) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let args: String = std::iter::once(cmd.exe.to_string_lossy().to_string())
            .chain(cmd.args.iter().cloned())
            .map(|a| format!("    <string>{}</string>\n", xml_escape(&a)))
            .collect();
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
{args}  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
  <key>ProcessType</key><string>Background</string>
  <key>StandardErrorPath</key><string>{}</string>
</dict>
</plist>
"#,
            xml_escape(&paths.log_path().to_string_lossy())
        );
        let path = agent_path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, plist)?;
        let domain = format!("gui/{}", uid());
        // Reload so an updated binary path takes effect.
        let _ = run(Command::new("launchctl").args(["bootout", &format!("{domain}/{LABEL}")]));
        run(Command::new("launchctl").args(["bootstrap", &domain]).arg(&path))?;
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
        let exec: Vec<String> = std::iter::once(cmd.exe.to_string_lossy().to_string())
            .chain(cmd.args.iter().cloned())
            .map(|a| quote(&a))
            .collect();
        let unit = format!(
            "[Unit]\nDescription=YAD download service\nAfter=network-online.target\n\n[Service]\nExecStart={}\nRestart=on-failure\nRestartSec=5\n\n[Install]\nWantedBy=default.target\n",
            exec.join(" ")
        );
        let dir = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home_dir().join(".config")).join("systemd/user");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("yad.service"), unit)?;
        let _ = paths;
        run(Command::new("systemctl").args(["--user", "daemon-reload"]))?;
        run(Command::new("systemctl").args(["--user", "enable", "yad.service"]))?;
        Ok(())
    }
    #[cfg(windows)]
    {
        let _ = paths;
        let mut tr = format!("\"{}\"", cmd.exe.to_string_lossy());
        for a in &cmd.args {
            tr.push(' ');
            tr.push_str(a);
        }
        let ok = run(Command::new("schtasks").args(["/Create", "/F", "/SC", "ONLOGON", "/RL", "LIMITED", "/TN", "YAD", "/TR", &tr]))?;
        if ok {
            Ok(())
        } else {
            Err(io::Error::other("Windows didn't accept the logon task."))
        }
    }
}

pub fn uninstall() -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let path = agent_path();
        let _ = run(Command::new("launchctl").args(["bootout", &format!("gui/{}/{LABEL}", uid())]));
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        let _ = run(Command::new("systemctl").args(["--user", "disable", "yad.service"]));
        let unit = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home_dir().join(".config")).join("systemd/user/yad.service");
        if unit.exists() {
            std::fs::remove_file(unit)?;
        }
        let _ = run(Command::new("systemctl").args(["--user", "daemon-reload"]));
        Ok(())
    }
    #[cfg(windows)]
    {
        let _ = run(Command::new("schtasks").args(["/Delete", "/F", "/TN", "YAD"]));
        Ok(())
    }
}

pub fn is_installed() -> bool {
    #[cfg(target_os = "macos")]
    {
        agent_path().exists()
    }
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home_dir().join(".config")).join("systemd/user/yad.service").exists()
    }
    #[cfg(windows)]
    {
        let mut cmd = Command::new("schtasks");
        cmd.args(["/Query", "/TN", "YAD"]);
        run(&mut cmd).unwrap_or(false)
    }
}

/// Extension IDs allowed to talk to the native host: the development ID plus any listed in
/// `YAD_EXTENSION_IDS` (comma separated), e.g. the Chrome Web Store ID.
pub fn extension_ids() -> Vec<String> {
    let mut ids = vec![DEV_EXTENSION_ID.to_string()];
    if let Ok(extra) = std::env::var("YAD_EXTENSION_IDS") {
        ids.extend(extra.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string));
    }
    ids
}

pub fn native_host_manifest(host_exe: &Path) -> String {
    let origins: Vec<String> = extension_ids().iter().map(|id| format!("chrome-extension://{id}/")).collect();
    serde_json::json!({
        "name": PIPE_NAME,
        "description": "YAD download manager",
        "path": host_exe.to_string_lossy(),
        "type": "stdio",
        "allowed_origins": origins,
    })
    .to_string()
}

/// Browser profile folders that hold `NativeMessagingHosts` (only browsers that are installed).
#[cfg(not(windows))]
fn browser_dirs() -> Vec<PathBuf> {
    let home = home_dir();
    #[cfg(target_os = "macos")]
    let (base, names) = (
        home.join("Library/Application Support"),
        ["Google/Chrome", "Google/Chrome Beta", "Google/Chrome Canary", "Chromium", "BraveSoftware/Brave-Browser", "Microsoft Edge", "Vivaldi", "Arc/User Data"],
    );
    #[cfg(not(target_os = "macos"))]
    let (base, names) = (
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config")),
        ["google-chrome", "google-chrome-beta", "google-chrome-unstable", "chromium", "BraveSoftware/Brave-Browser", "microsoft-edge", "vivaldi", "opera"],
    );
    names.iter().map(|n| base.join(n)).filter(|p| p.is_dir()).collect()
}

/// Writes the native-messaging manifest for every installed Chromium browser.
/// Returns where it was registered.
pub fn register_native_host(host_exe: &Path, paths: &Paths) -> io::Result<Vec<PathBuf>> {
    let manifest = native_host_manifest(host_exe);
    let mut done = Vec::new();
    #[cfg(not(windows))]
    {
        let _ = paths;
        for dir in browser_dirs() {
            let target = dir.join("NativeMessagingHosts");
            std::fs::create_dir_all(&target)?;
            let file = target.join(format!("{PIPE_NAME}.json"));
            if std::fs::read_to_string(&file).ok().as_deref() != Some(manifest.as_str()) {
                std::fs::write(&file, &manifest)?;
            }
            done.push(file);
        }
    }
    #[cfg(windows)]
    {
        let file = paths.config_dir.join("native-host.json");
        std::fs::write(&file, &manifest)?;
        for key in [
            "Software\\Google\\Chrome\\NativeMessagingHosts",
            "Software\\Chromium\\NativeMessagingHosts",
            "Software\\BraveSoftware\\Brave-Browser\\NativeMessagingHosts",
            "Software\\Microsoft\\Edge\\NativeMessagingHosts",
        ] {
            let full = format!("HKCU\\{key}\\{PIPE_NAME}");
            if run(Command::new("reg").args(["add", &full, "/ve", "/t", "REG_SZ", "/d"]).arg(&file).arg("/f"))? {
                done.push(PathBuf::from(full));
            }
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_lists_dev_extension() {
        let m: serde_json::Value = serde_json::from_str(&native_host_manifest(Path::new("/Applications/YAD.app/Contents/MacOS/yad"))).unwrap();
        assert_eq!(m["name"], "ke.innv8.yad");
        assert_eq!(m["type"], "stdio");
        assert_eq!(m["allowed_origins"][0], format!("chrome-extension://{DEV_EXTENSION_ID}/"));
    }

    #[test]
    fn xml_is_escaped() {
        assert_eq!(xml_escape("a&b<c>"), "a&amp;b&lt;c&gt;");
    }
}
