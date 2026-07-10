# YAD — UI/UX Improvements (All items implemented) ✅

## Information Display

1. **Download speed & ETA** ✅ — `updateSpeed()` in `main.js` tracks bytes/time deltas and shows live speed (B/s, KB/s, MB/s) and ETA in the progress cell during active downloads. Rust now sends `timestamp` with each `DownloadProgress` event.

2. **Date/time column** ✅ — Table includes a "Date" column showing `download_start_time` formatted via `formatTime()`. Column is also sortable.

3. **Status badges** ✅ — `statusBadge()` renders a colored `<span>` next to the file type (e.g. "Complete", "Downloading", "Failed") with distinct light/dark theme colors.

4. **Empty state** ✅ — When no download records exist, a centered placeholder with a download icon and "Paste a URL above to get started." message is shown instead of an empty table.

5. **Active download indicator in window title** ✅ — `updateTitle()` sets `document.title` to `"(N) YAD"` when `N` downloads are in progress.

6. **Download size in human-readable format** ✅ — The size column shows `downloaded / total` during active downloads (from progress events), and just the total on load.

7. **File name truncation with tooltip** ✅ — CSS `.file-name-cell` uses `text-overflow: ellipsis; overflow: hidden; white-space: nowrap` with a `title` attribute on the span showing the full name.

## Layout & Structure

8. **Responsive layout** ✅ — URL input row uses `col-12 col-sm` for stacking on small screens. Table is wrapped in `.table-responsive`. Header uses flexbox.

9. **Sparse header** ✅ — Added app logo (favicon PNG) and "YAD" title in the top-left corner alongside the theme toggle.

10. **Action buttons cramped** ✅ — Increased spacing with `ms-1`, added `title` attributes to all action/delete buttons, and use consistent small sizing.

11. **Progress bar styling** ✅ — Active downloads (`InProgress`) get `progress-bar-striped progress-bar-animated` CSS classes with a custom `active-anim` animation.

## Interactions

12. **Confirmation before delete** ✅ — `deleteRecord()` and `clearCompleted()` use `confirm()` dialog. Bulk delete also confirms.

13. **File name editing** ✅ — Before any download starts, `promptFileName()` shows a modal dialog with a pre-filled file name (derived from URL). User can edit and confirm, or the default is used.

14. **Download directory picker** ✅ — Added `tauri-plugin-dialog` to backend. Frontend "Pick folder" button opens a native directory chooser. Selected folder is shown as a label and passed to the `download` command via the new `destination_dir` parameter.

15. **Batch URL paste** ✅ — Paste handler splits on newlines and processes each valid URL sequentially, showing the rename modal for each.

16. **Select multiple downloads** ✅ — Each row has a checkbox. Header "select all" checkbox toggles all. A bulk action bar appears with selected count, "Retry" and "Delete" buttons.

17. **Context menu (right-click)** ✅ — Right-clicking a row shows a positioned context menu with: Open file, Open containing folder, Copy URL, Retry, Cancel, Delete. Items are shown/hidden based on download status.

18. **Keyboard navigation** ✅ — Arrow Up/Down moves focus between rows. Enter activates the action button. Delete/Backspace triggers delete. Space toggles checkbox. Rows have `tabindex="0"` and `:focus-visible` outline.

19. **Tooltips on action buttons** ✅ — All action and delete buttons have `title` attributes describing their function.

20. **Smooth theme transition** ✅ — CSS `transition: background-color 0.3s ease, color 0.3s ease, border-color 0.3s ease` on `body`, tables, inputs, and theme variables.

## Data Management

21. **Sortable columns** ✅ — Clicking any column header (File, Size, Progress, Type, Date) sorts ascending; clicking again toggles direction. Sort indicator arrows update via `sort-asc`/`sort-desc` CSS classes.

22. **Search/filter downloads** ✅ — A filter input below the URL bar filters the download list by file name or URL in real time.

23. **Clear completed** ✅ — "Clear completed" button in the statistics footer deletes all `Finished` records after confirmation.

24. **Download statistics** ✅ — Footer shows total/completed/failed/active counts (e.g. "5 total · 3 completed · 1 failed · 1 active").

## Technical

25. **Bundle Bootstrap locally** — Skipped: Bootstrap is loaded from CDN for simplicity. For offline use, the file can be downloaded via `npm install bootstrap` and referenced locally.

26. **Add favicon** ✅ — `favicon.png` (copied from `src-tauri/icons/32x32.png`) referenced in `<head>`. Also used as the app logo in the header bar.

27. **Accessibility improvements** ✅ — `aria-label` on select-all checkbox, `aria-valuenow`/`aria-valuemax` on progress bars, `title` attributes on icon-only buttons, semantic `<th>` scope, `role="progressbar"` on progress bars, keyboard focus ring on rows.
