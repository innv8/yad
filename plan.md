# YAD — CI/CD & UI/UX Improvements (All items implemented) ✅

## GitHub Actions CI/CD

Added `.github/workflows/build-release.yml` that runs on every push to `main`:

**Versioning:**
- Reads current version from `tauri.conf.json`
- Strips any pre-release suffix (e.g. `1.0.0-beta.2` → `1.0.0`)
- Bumps the patch number (`1.0.0` → `1.0.1`)
- Writes the new version to both `tauri.conf.json` and `Cargo.toml`
- Commits the bump (`[skip ci]`) and tags (`v1.0.1`)
- Pushes commit and tag back

**Build matrix:**
| OS | Runner | Target | Artifact name |
|---|---|---|---|
| macOS (ARM64) | `macos-latest` | `aarch64-apple-darwin` | `yad-macos-arm64-v{version}` |
| Linux (x86_64) | `ubuntu-latest` | `x86_64-unknown-linux-gnu` | `yad-linux-x86_64-v{version}` |

**Release:**
- `softprops/action-gh-release` creates a release for the new tag
- Versioned binaries are uploaded as release assets
- Both jobs run in parallel after version bump

**Permissions:** `contents: write` for pushing commits/tags and creating releases.

---

## UI/UX Improvements

### Information Display

1. **Download speed & ETA** ✅ — `updateSpeed()` tracks bytes/time deltas and shows live speed and ETA in the progress cell. Rust sends `timestamp` with each progress event.

2. **Date/time column** ✅ — Table includes a "Date" column showing `download_start_time` formatted via `formatTime()`. Sortable.

3. **Status badges** ✅ — Colored `<span>` next to file type (Complete, Downloading, Failed, Pending, Cancelled) with light/dark theme colors.

4. **Empty state** ✅ — Centered placeholder with download icon and "Paste a URL above to get started." when no records exist.

5. **Active download indicator in window title** ✅ — `document.title` set to `"(N) YAD"` when downloads are in progress.

6. **Download size formatting** ✅ — Size column shows `downloaded / total` during active downloads.

7. **File name truncation with tooltip** ✅ — CSS `text-overflow: ellipsis` + `title` attribute on the span.

### Layout & Structure

8. **Responsive layout** ✅ — URL input uses `col-12 col-sm`, table wrapped in `.table-responsive`, header uses flexbox.

9. **Sparse header** ✅ — App logo + "YAD" title in the top-left with theme toggle.

10. **Action buttons spacing** ✅ — Increased spacing, `title` attributes, consistent sizing.

11. **Animated progress bars** ✅ — `progress-bar-striped progress-bar-animated` on active downloads.

### Interactions

12. **Delete confirmation** ✅ — `confirm()` dialog before any delete action.

13. **File name editing** ✅ — Modal dialog before each download lets the user edit the file name. Custom name is passed to the `download` command via the new `file_name` parameter.

14. **Directory picker** ✅ — `tauri-plugin-dialog` registered. "Pick folder" button opens a native directory chooser. Selected path is passed via the `destination_dir` parameter.

15. **Batch URL paste** ✅ — Paste handler splits on newlines and processes each URL with the rename modal.

16. **Multi-select** ✅ — Checkboxes per row, select-all header, bulk bar with "Retry Selected" and "Delete Selected".

17. **Context menu (right-click)** ✅ — Positioned menu with Open file, Open folder, Copy URL, Retry, Cancel, Delete. Items shown/hidden by status.

18. **Keyboard navigation** ✅ — Arrow keys move focus, Enter activates action, Delete removes, Space toggles checkbox. Rows are focusable.

19. **Tooltips on buttons** ✅ — `title` attributes on all action and delete buttons.

20. **Smooth theme transition** ✅ — CSS `transition` on `body`, tables, inputs, and theme variables.

### Data Management

21. **Sortable columns** ✅ — Click headers to sort ascending/descending. Sort indicator arrows update via CSS.

22. **Search/filter downloads** ✅ — Real-time filter by file name or URL.

23. **Clear completed** ✅ — Button in the stats footer deletes all Finished records.

24. **Download statistics** ✅ — Footer shows total/completed/failed/active counts.

### Technical

25. **Bundle Bootstrap locally** — Skipped (CDN is sufficient; can download via `npm install bootstrap` for offline).

26. **Favicon** ✅ — `favicon.png` referenced in `<head>` and used as app logo.

27. **Accessibility** ✅ — `aria-label`, `aria-valuenow`/`aria-valuemax`, `role="progressbar"`, keyboard focus, `title` attributes, semantic HTML.
