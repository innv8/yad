// Entry point for both windows. Loads the dev mock only when the URL has ?mock
// (browser previews and the UI tests); the real app never does.
const entry = document.documentElement.dataset.entry || 'main';
if (/[?&]mock(=|&|$)/.test(location.search)) await import('../../dev/mock.js');
await import(entry === 'tray' ? './tray.js' : './main.js');
