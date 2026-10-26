# Yet Another Downloader

<p align="center">
  <a href="https://skillicons.dev">
    <img src="https://skillicons.dev/icons?i=rust,nodejs,tauri" />
  </a>
</p>

---

![logo](./src-tauri/icons/white-bg.png)

---

Yet Another Downloader (YAD) is a download manager for macOS, Linux and Windows.

- **Fast, resumable downloads.** Large files download over several connections at once and
  stream straight to disk. Pause and resume pick up where they stopped, also after a
  restart. Dropped connections are retried; if the file changes on the server, YAD starts
  over instead of mixing versions.
- **Keeps going in the background.** Downloads run in a small service, so closing the
  window doesn't stop them. It can start at login.
- **Video from the browser.** The YAD Link extension for Chrome and other Chromium browsers
  puts a *Download this video* button on videos as they play, with every quality it finds,
  including streamed (HLS and DASH) video. It can also take over Chrome's own downloads.
- **Same interface everywhere.** YAD draws its own title bar, controls and fonts, so it
  looks the same on every system.

Files are sorted by type into `~/Downloads/Yad` unless you choose another folder.

**This is a beta version. Please report bugs as [issues](https://github.com/innv8/yad/issues).**

This software is released under the [Apache V2 license](https://choosealicense.com/licenses/apache-2.0/). Please read both the [License](./LICENSE) and [Notice](./NOTICE).

## Installation

Download the latest version from the [Releases page](https://github.com/innv8/yad/releases).

| System | File | Notes |
|---|---|---|
| macOS, Apple Silicon (M1 and later) | `Yet another Downloader_<version>_aarch64.dmg` | Built for every release |
| macOS, Intel | `Yet another Downloader_<version>_x64.dmg` | Built on request |
| Windows 10 and 11 | `Yet another Downloader_<version>_x64-setup.exe` | Built on request |
| Linux (x86_64) | `.deb` or `.AppImage` | Built on request |

If your system isn't in the release yet, [open an issue](https://github.com/innv8/yad/issues)
asking for it, or [build from source](#building-from-source).

### macOS

1. Open the `.dmg` and drag **Yet another Downloader** into **Applications**.
2. The app isn't signed with an Apple Developer ID yet, so macOS blocks the first launch.
   Open it once from Finder with right-click → **Open** → **Open**, or run:

   ```sh
   xattr -dr com.apple.quarantine "/Applications/Yet another Downloader.app"
   ```

3. YAD appears in the menu bar. Closing the window keeps downloads going; quit from the
   menu bar icon.

### Windows

Run the `-setup.exe` installer. Windows SmartScreen may warn about an unknown publisher
because the installer isn't signed yet: choose **More info** → **Run anyway**. YAD appears
in the notification area next to the clock.

### Linux

```sh
# Debian, Ubuntu and derivatives
sudo apt install ./yet-another-downloader_<version>_amd64.deb

# Any distribution
chmod +x Yet-another-Downloader_<version>_amd64.AppImage
./Yet-another-Downloader_<version>_amd64.AppImage
```

The tray icon needs a desktop with StatusNotifier/AppIndicator support (on GNOME, the
*AppIndicator and KStatusNotifierItem Support* extension).

### ffmpeg (recommended)

Some sites stream video as separate video and audio tracks. YAD uses
[ffmpeg](https://ffmpeg.org) to join them into one `.mp4`; without it, they are saved as two
files. YAD finds ffmpeg next to the app or on your `PATH`, including Homebrew's folders.

```sh
brew install ffmpeg          # macOS
sudo apt install ffmpeg      # Debian / Ubuntu
winget install ffmpeg        # Windows
```

You can also point YAD at a specific copy with the `YAD_FFMPEG` environment variable.
**Settings → About** shows whether ffmpeg was found.

### The browser extension (YAD Link)

The extension isn't in the Chrome Web Store yet, so it is installed by hand:

1. Download `yad-link-extension-v<version>.zip` from the release and unzip it, or use the
   [`extension`](./extension) folder of this repository.
2. Open `chrome://extensions` (or `brave://extensions`, `edge://extensions`), turn on
   **Developer mode**, choose **Load unpacked** and pick the unzipped folder.
3. Open YAD once. It registers itself with every Chromium browser it finds, and the
   extension shows **Connected**.

Play a video and a **Download this video** button appears on it. Click the YAD Link icon
in the toolbar to see everything found on the page. Hold **Alt** while clicking a link to
let the browser download it instead.

DRM-protected video (most paid streaming services) and live streams can't be downloaded.

### Start at login

Turn on **Settings → General → Start at login**. YAD then starts with your session
(a LaunchAgent on macOS, a systemd user unit on Linux, a logon task on Windows) and
resumes unfinished downloads.

### The standalone service

Each release also has a `yadd` binary: the download service without the window, for
servers or scripting.

```sh
yadd                       # run the service in the foreground
yadd install               # start it at login
yadd uninstall
yadd install-native-host   # let the browser extension talk to it
```

### Uninstalling

1. Turn off **Start at login** (or run `yadd uninstall`), then quit YAD.
2. Delete the app.
3. To remove your download history and settings too, delete the data folder:
   - macOS: `~/Library/Application Support/Yad`
   - Linux: `~/.config/Yad`
   - Windows: `%LOCALAPPDATA%\Yad`
4. Remove the browser link file `ke.innv8.yad.json` from the browser's
   `NativeMessagingHosts` folder (on Windows, the `ke.innv8.yad` registry key under
   `HKCU\Software\Google\Chrome\NativeMessagingHosts`).

Downloaded files in `~/Downloads/Yad` are never deleted.

## How it fits together

```
crates/yad-core   download engine, HLS/DASH, SQLite storage, IPC, login service
crates/yadd       the background service and the browser native-messaging host
src-tauri         the window (Tauri); also runs as the service with --daemon
src               the interface (plain HTML, CSS and JS modules)
extension         YAD Link, the Chrome extension
docs/ipc.md       the JSON protocol between them
```

The window talks to the service over a local socket and starts it when needed. The
extension talks to it through Chrome native messaging, which only the YAD Link extension
is allowed to use. See [docs/ipc.md](./docs/ipc.md).

## Building from source

You need [Rust](https://www.rust-lang.org/) and the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) for your system. Node is only
needed for the interface tests.

```sh
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Tauri CLI
cargo install tauri-cli --version "^2" --locked

# Run in development
cargo tauri dev

# Build the app; the installer goes to target/release/bundle/
cargo tauri build

# Just the background service
cargo build --release -p yadd
```

While developing, keep test data apart from your real downloads and browsers:

```sh
export YAD_CONFIG_DIR=/tmp/yad-dev    # separate database, socket and log
export YAD_SKIP_NATIVE_HOST=1         # don't register this build with your browsers
```

With `YAD_CONFIG_DIR` set, downloads go to `$YAD_CONFIG_DIR/downloads` unless
`YAD_DOWNLOAD_DIR` says otherwise.

## Testing

```sh
cargo test -p yad-core -p yadd   # engine, storage, parsers, end-to-end downloads
npm ci && npm run test:ui        # every screen must lay out the same in Chromium and WebKit
```

The end-to-end tests run real downloads against a local server that drops connections,
ignores ranges and changes files mid-download. With ffmpeg installed they also download
real HLS and DASH video and check the result with `ffprobe`.

## Releases and CI

| Workflow | When it runs | What it does |
|---|---|---|
| `test.yml` | Every pull request to `main` | On macOS: Rust tests (including real HLS/DASH video via ffmpeg) and the interface layout test. Also checks the extension's scripts and that the app and extension versions match. |
| `release.yml` | Every push to `main` | Bumps the version from conventional commits (`feat:`, `fix:`), tags it, builds the **macOS Apple Silicon** app, `yadd` and the **YAD Link extension** (always the same version), and attaches them to the GitHub release. Pushes without a `feat` or `fix` commit still build; the files are kept as workflow artifacts. |
| `build-platforms.yml` | Only by hand: **Actions → Build other platforms → Run workflow** | Builds Windows, Linux and Intel macOS (all or one). Give it a release tag such as `v1.1.0` to attach the files to that release. These platforms aren't tested in CI yet. |

The release builds aren't code-signed. Signing needs an Apple Developer ID and a Windows
code-signing certificate added as repository secrets.

## Contributing

Please follow these guidelines when contributing.

- [Reporting Bugs](./.github/ISSUE_TEMPLATE/bug_report.md)
- [Requesting Features](./.github/ISSUE_TEMPLATE/feature_request.md)
- [How to contribute](./.github/CONTRIBUTING.md)
- [How to make a pull request](./.github/pull_request_template.md)

---

You can read about how I made this and more at [innv8.ke](https://innv8.ke). This is my educational
Github Organisation. Please also check my [personal GitHub](https://github.com/rapando)


> rapando
