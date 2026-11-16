/* Minimal offline shell for the installable PWA. API calls are never cached. */
const CACHE = 'agora-shell-v4';
const SHELL = ['/', '/app.css', '/js/main.js', '/js/util.js', '/js/md.js', '/js/state.js', '/js/home.js', '/js/session.js', '/js/digest.js', '/js/usage.js', '/js/settings.js', '/js/experiment.js', '/js/pair.js', '/js/voice.js', '/js/attention.js', '/js/shortcuts.js', '/icon.svg', '/icon-192.png', '/manifest.webmanifest'];

self.addEventListener('install', (e) => {
  e.waitUntil(caches.open(CACHE).then((c) => c.addAll(SHELL)).then(() => self.skipWaiting()));
});

self.addEventListener('activate', (e) => {
  e.waitUntil(
    caches.keys().then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k)))).then(() => self.clients.claim()),
  );
});

self.addEventListener('fetch', (e) => {
  const url = new URL(e.request.url);
  if (e.request.method !== 'GET' || url.origin !== location.origin || url.pathname.startsWith('/api/')) return;
  // Network first so updates show immediately; fall back to the cached shell offline.
  e.respondWith(
    fetch(e.request)
      .then((res) => {
        if (res.ok && SHELL.includes(url.pathname)) {
          const copy = res.clone();
          caches.open(CACHE).then((c) => c.put(e.request, copy));
        }
        return res;
      })
      .catch(() => caches.match(e.request).then((r) => r || caches.match('/'))),
  );
});
