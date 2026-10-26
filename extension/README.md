# YAD Link (Chrome extension)

YAD Link hands videos and files from Chrome to YAD, the download manager on your computer.
It talks to YAD only through Chrome native messaging (`ke.innv8.yad`); it opens no network port
and loads no remote code. The API it uses is described in [`docs/ipc.md`](../docs/ipc.md).

What it does:

- Puts a **Download this video** button over the video that is playing. Clicking it sends the
  preferred quality (Settings › Downloads › Preferred video quality); the arrow opens a menu with
  every stream YAD found (HLS and DASH qualities, audio only).
- **Takes over downloads**: files Chrome would download go to YAD instead, filtered by type,
  minimum size and excluded sites. Hold **Alt** while clicking a link to let Chrome keep it.
  When YAD isn't reachable Chrome downloads as usual.
- Adds **Download with YAD** to the right-click menu for links, videos, audio and images.
- The toolbar popup lists the video on the tab and the files linked on the page.
- Protected (DRM) videos are labelled "Protected video" and are not offered.

## Layout

```
manifest.json          MV3 manifest (key pins the ID to nhclkmkhoglkcoeiijoonmdmfmgigdhp)
background.js          service worker (ES module): detection, badge, take-over, context menu, native port
content.js             video button (closed shadow DOM) and Alt-click tracking
popup/                 toolbar popup
options/               options page (Capture, Sites, Connection, About)
lib/native.js          native messaging client (request ids, 10 s timeouts, reconnect with backoff)
lib/filetypes.js       extension/MIME → YAD file type, same table as YAD
lib/format.js          sizes, names, variant labels, quality picking
lib/icons.js           inline SVG sprite
lib/ui.css             colour tokens (light/dark) and shared components
fonts/                 Archivo Narrow + Tenor Sans (latin, woff2) and OFL.txt
icons/                 16/32/48/128 px icons
```

No build step and no runtime dependencies.

## Load it unpacked

1. Open `chrome://extensions` and switch on **Developer mode**.
2. Click **Load unpacked** and pick this `extension/` folder.
3. The ID shown must be `nhclkmkhoglkcoeiijoonmdmfmgigdhp` (pinned by `key` in the manifest).

## Connect it to YAD (native host)

Chrome starts `yad --native-host` through a native messaging host manifest named
`ke.innv8.yad.json`. YAD writes it when it is opened for the first time; you can also run:

```sh
yadd install-native-host
```

The manifest lives in Chrome's `NativeMessagingHosts` folder (on Windows, a registry key points to
it). It must allow the extension's origin:

```json
"allowed_origins": ["chrome-extension://nhclkmkhoglkcoeiijoonmdmfmgigdhp/"]
```

Then open the extension's popup and click **Start YAD**, or the **Connection** page of the options.

**Chrome Web Store builds:** the store gives the published extension its own ID and does not use the
`key` from this manifest. Remove `key` before uploading, and add the store ID
(`chrome-extension://<store id>/`) to `allowed_origins` in the native host manifest that YAD
installs, next to the development ID. Without that, Chrome refuses the connection
("Access to the specified native messaging host is forbidden").

## Permissions (text for the Chrome Web Store listing)

- **Host access to all sites (`<all_urls>`)**: videos and downloads can come from any site. The
  extension needs it to notice video streams as a page loads them, to show the download button on
  any page, and to read the cookies a file needs. Nothing is sent anywhere except to YAD on the
  same computer.
- **webRequest**: reads response headers (content type, size, file name) to recognise video files,
  HLS/DASH playlists and file downloads. It does not block or change requests.
- **cookies**: files behind a sign-in need the site's cookies. When "Send cookies with downloads"
  is on, the cookies for that file's address are passed to YAD on this computer so it can download
  it. They are never sent to any other server. The setting can be turned off.
- **downloads**: to hand downloads Chrome starts over to YAD (cancel in Chrome, start in YAD) when
  "Take over browser downloads" is on, and to give them back to Chrome if YAD can't take them.
- **nativeMessaging**: the only channel to YAD, the desktop app.
- **scripting**: lists the files linked on the current page when the popup is opened.
- **tabs**: knows which page a video or download came from (used as the Referer and shown in YAD).
- **contextMenus**: the "Download with YAD" menu item.
- **storage**: keeps a copy of the settings so the page button reacts without asking YAD each time.

## Native messaging calls

| when | method | params |
|---|---|---|
| "Start YAD" (popup, options) | `start` | – (the host starts YAD; then `hello` + `getSettings`) |
| port opened | `hello` | `{ browser: "Chrome 142", version: "1.0.2" }` |
| after `hello`, popup/options opened | `getSettings` | – |
| an option changed | `setSettings` | `{ browser: { <changed keys> } }` |
| HLS/DASH manifest seen, popup file sizes | `probe` | `{ url, headers }` |
| button, menu, popup, context menu, take-over | `add` | `{ url, source: "extension", headers, pageUrl?, variant?, title?, fileName? }` |

`headers` carries `User-Agent`, `Referer` (the page) and, when enabled, `Cookie`.
For HLS/DASH links the extension sends the page title as `title` (YAD names the file
`"<title> <quality>.mp4"`); for take-over it passes the server's file name as `fileName`.
Errors YAD returns (for example "Live streams aren't supported.") are shown to the user as they are.

`hello`, `getSettings` and `setSettings` never start YAD; `hello` answering "YAD isn't running." means
offline (take-over leaves downloads to Chrome, `hello` is retried with backoff). `start`, `add` and
`probe` start it, so `probe` is only sent on its own while YAD is running.

Options changed while YAD is not running are kept and sent with `setSettings` on the next
connection.
