# YAD IPC contract

YAD 2 is split into three processes that talk JSON:

```
Chrome extension ──native messaging──▶ yad --native-host ──local socket──▶ yadd (daemon)
YAD window (Tauri) ─────────────────────────────────────────local socket──▶ yadd
```

- **yadd** owns the download engine, the queue and the SQLite database. It runs as
  `yadd` (standalone binary) or as `yad --daemon` (the app binary in daemon mode).
- **The window** is a thin client. It forwards calls to the daemon and relays daemon
  events to the web view.
- **The extension** talks to `yad --native-host` (or `yadd native-host`), which relays
  each request to the daemon. Only user actions (`add`, `probe`, `start`, …) start the
  daemon; `hello`, `ping`, `status`, `getSettings` and `setSettings` fail with
  "YAD isn't running." when it is down, so quitting YAD isn't undone by the extension.

## Wire format

Newline-delimited JSON over the local socket (Unix domain socket at
`<config_dir>/yadd.sock`, or `$XDG_RUNTIME_DIR`/temp dir when that path is too long for a
socket — the path is written to `yadd.log` as `listening on <path>`; named pipe `\\.\pipe\ke.innv8.yad` on Windows).
Native messaging uses Chrome's 4-byte length prefix instead of newlines; the
messages are identical.

```jsonc
// request
{ "id": 1, "method": "add", "params": { "url": "https://…" } }
// response
{ "id": 1, "result": { … } }
{ "id": 1, "error": { "message": "Invalid URL" } }
// event (only after a `subscribe` call on that connection)
{ "event": { "type": "progress", "items": [ … ] } }
```

All keys are camelCase. Sizes are bytes, speeds bytes/second, times Unix seconds.

## Types

```ts
type Status = "Queued" | "Downloading" | "Paused" | "Completed" | "Failed" | "Cancelled";
type FileType = "Videos" | "Audio" | "Documents" | "Programs" | "Images" | "Compressed" | "Others";
type Kind = "file" | "hls" | "dash";

interface Download {
  id: number;
  url: string;
  fileName: string;
  fileType: FileType;
  kind: Kind;
  destinationDir: string;
  destinationPath: string;
  totalSize: number | null;     // null when the server does not say
  downloaded: number;
  status: Status;
  error: string | null;         // human readable, e.g. "Connection lost"
  resumable: boolean;
  connections: number;          // active connections now (0 when not downloading)
  speed: number;                // 0 when not downloading
  eta: number | null;           // seconds
  retries: number;              // retries used so far
  source: "app" | "extension" | "clipboard";
  pageUrl: string | null;       // page the extension found it on
  variant: string | null;       // quality label, e.g. "1080p", "Audio"
  createdAt: number;
  finishedAt: number | null;
}

interface DownloadDetails extends Download {
  segments: { start: number; end: number; downloaded: number }[];
  speedHistory: number[];       // last 60 one-second samples, oldest first
  etag: string | null;
  lastModified: string | null;
}

interface Variant {
  id: string;                   // pass back as `variant` to add()
  label: string;                // "1080p", "720p", "Audio"
  resolution: string | null;    // "1920x1080"
  bandwidth: number | null;     // bits/s
  codecs: string | null;
  estimatedSize: number | null;
  audioOnly: boolean;
}

interface Probe {
  url: string;
  fileName: string;
  fileType: FileType;
  size: number | null;
  contentType: string | null;
  resumable: boolean;
  kind: Kind;
  variants: Variant[];          // empty for plain files; a stream without a quality list
                                // (a media playlist) has one variant labelled "Video"
  duration: number | null;      // seconds, streams only
}

interface Settings {
  downloadDir: string;
  sortByType: boolean;
  askBeforeDownload: boolean;
  keepRunning: boolean;         // keep daemon alive when the window closes
  startAtLogin: boolean;
  notifications: boolean;
  maxActive: number;            // 1..10, default 3
  connectionsPerDownload: number; // 1..16, default 8
  speedLimit: number | null;    // bytes/s for all downloads together
  retries: number;              // default 5
  videoQuality: "best" | "1080" | "720" | "480" | "audio";
  mergeTracks: boolean;
  theme: "system" | "light" | "dark";
  browser: {
    videoButton: boolean;
    takeOver: boolean;
    types: FileType[];          // which types takeOver applies to
    minSize: number;            // bytes, default 1 MiB
    excludedSites: string[];    // host names
    sendCookies: boolean;
    contextMenu: boolean;
  };
}

interface Status {
  version: string;
  pid: number;
  active: number;
  totalSpeed: number;
  serviceInstalled: boolean;
  ffmpeg: string | null;        // path, or null when not found
  extension: { connected: boolean; lastSeen: number | null; browser: string | null; version: string | null };
}
```

## Methods

| method | params | result |
|---|---|---|
| `ping` | – | `"pong"` |
| `status` | – | `Status` |
| `subscribe` | – | `true`, then events on this connection |
| `list` | – | `Download[]` (newest first) |
| `get` | `{ id }` | `DownloadDetails` |
| `probe` | `{ url, headers? }` | `Probe` |
| `add` | `{ url, fileName?, directory?, headers?, source?, pageUrl?, variant?, title?, start? }` | `Download` |
| `pause` / `resume` / `cancel` / `retry` | `{ id }` | `Download` |
| `remove` | `{ id }` | `true` (row only, file kept) |
| `deleteFile` | `{ id }` | `true` (row and file) |
| `rename` | `{ id, fileName }` | `Download` |
| `pauseAll` / `resumeAll` | – | `true` |
| `getSettings` | – | `Settings` |
| `setSettings` | partial `Settings` (deep-merged for `browser`) | `Settings` |
| `installService` / `uninstallService` | – | `Status` |
| `hello` | `{ browser, version }` (extension only) | `Status` |
| `start` | – (native host only) | `Status`; starts the service if needed |
| `shutdown` | – | `true` |

`headers` is a string map. The extension sends `Cookie`, `Referer` and `User-Agent`;
they are stored with the download and sent on every request.

`start: false` adds the download as `Paused` so it waits until resumed (used by
"Add to queue"). Adding a link that is already queued, downloading or paused returns the
existing download instead of a duplicate.

`title` (the page title) names streamed videos: `"<title> <quality>.mp4"`. Without ffmpeg
the tracks can't be remuxed, so a TS-based stream is saved as `.ts` (and separate audio as
`<name>.audio.m4a`); `Download.error` then carries a note saying so. For streams,
`add` probes the playlist first, so errors such as "Live streams aren't supported" or
"This video is protected (DRM) and can't be downloaded" come back from `add` directly.

## Events

```ts
type Event =
  | { type: "added"; download: Download }
  | { type: "updated"; download: Download }      // any status/name change
  | { type: "progress"; items: { id: number; downloaded: number; totalSize: number | null;
                                 speed: number; eta: number | null; connections: number }[] }
  | { type: "removed"; id: number }
  | { type: "settings"; settings: Settings }
  | { type: "extension"; connected: boolean; browser: string | null };
```

`progress` is batched across all downloads and sent at most 4 times a second.

## Tauri bridge (window ⇄ Rust)

- `invoke("ipc", { method, params })` forwards any method above and returns its result.
- `invoke("open_path", { path })` opens a file with the default app.
- `invoke("reveal_path", { path })` shows a file in Finder / Explorer / the file manager.
- `invoke("show_main")` shows and focuses the main window (used by the tray popover).
- `invoke("quit_app", { stopDaemon })` quits the window process.
- Event `yad-event` carries the `Event` objects above.
- Event `yad-connection` carries `{ connected: boolean }` when the daemon link drops or returns.
- Event `tray-anchor` (tray window only) carries `{ edge: "top" | "bottom" }` just before the
  popover is shown: `bottom` means it opened upwards from a bottom taskbar.

## Environment variables

| variable | effect |
|---|---|
| `YAD_CONFIG_DIR` | Use another config directory (database, socket, log). Tests and development. Downloads then default to `<config_dir>/downloads`. |
| `YAD_DOWNLOAD_DIR` | Default download folder (normally `~/Downloads/Yad`). |
| `YAD_SKIP_NATIVE_HOST` | Don't register the native-messaging host in browser profiles on start. |
| `YAD_EXTENSION_IDS` | Extra extension IDs (comma separated) allowed to use the native host, e.g. the Chrome Web Store ID. |
| `YAD_FFMPEG` | Path to ffmpeg, used to join video and audio tracks. |
