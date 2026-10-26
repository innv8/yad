// Tiny static server for src/ (no dependencies). startServer() → { url, close }.
import http from 'node:http';
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../src');
const TYPES = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8',
  '.woff2': 'font/woff2', '.png': 'image/png', '.ico': 'image/x-icon', '.svg': 'image/svg+xml', '.txt': 'text/plain; charset=utf-8',
};

export function startServer(port = 0) {
  const server = http.createServer(async (req, res) => {
    try {
      const u = new URL(req.url, 'http://x');
      let p = path.normalize(decodeURIComponent(u.pathname)).replace(/^([/\\])+/, '');
      if (!p || p.endsWith('/')) p += 'index.html';
      const file = path.join(ROOT, p);
      if (!file.startsWith(ROOT)) { res.writeHead(403).end(); return; }
      const body = await fs.readFile(file);
      res.writeHead(200, {
        'content-type': TYPES[path.extname(file)] || 'application/octet-stream',
        'cache-control': 'no-store',
        // Same spirit as the app's CSP: nothing inline, nothing remote. Violations show up as console errors.
        'content-security-policy': "default-src 'self'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self' data:",
      });
      res.end(body);
    } catch {
      res.writeHead(404).end('not found');
    }
  });
  return new Promise((resolve) => {
    server.listen(port, '127.0.0.1', () => {
      const { port: p } = server.address();
      resolve({ url: `http://127.0.0.1:${p}`, close: () => new Promise((r) => server.close(r)) });
    });
  });
}

// `node tests/ui/server.js` serves src/ for manual previews (open /index.html?mock).
if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  startServer(Number(process.env.PORT) || 5173).then((s) => console.log(`serving src/ at ${s.url}/index.html?mock`));
}
