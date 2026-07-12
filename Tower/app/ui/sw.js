// Tower service worker: static-shell cache + payload-less push.
// A push carries no body; we fetch fresh state and compose the notification
// locally, so nothing sensitive transits the push service.
const SHELL = 'tower-shell-v3';
const ASSETS = ['/', '/tower.css', '/tower.js', '/icon.svg', '/manifest.webmanifest'];

self.addEventListener('install', (e) => {
  e.waitUntil(caches.open(SHELL).then(c => c.addAll(ASSETS)).then(() => self.skipWaiting()));
});
self.addEventListener('activate', (e) => {
  e.waitUntil(caches.keys().then(keys => Promise.all(keys.filter(k => k !== SHELL).map(k => caches.delete(k)))).then(() => self.clients.claim()));
});

// network-first for everything; cached shell only as offline fallback
self.addEventListener('fetch', (e) => {
  const url = new URL(e.request.url);
  if (e.request.method !== 'GET' || url.pathname.startsWith('/api/') || url.pathname.startsWith('/files/')) return;
  e.respondWith(fetch(e.request).then(r => {
    if (r.ok) { const copy = r.clone(); caches.open(SHELL).then(c => c.put(e.request, copy)); }
    return r;
  }).catch(() => caches.match(e.request)));
});

self.addEventListener('push', (e) => {
  e.waitUntil((async () => {
    let title = 'Tower', body = 'Something needs you.';
    try {
      const s = await (await fetch('/api/state')).json();
      const c = s.counts;
      const bits = [];
      if (c.decide) bits.push(`${c.decide} decision${c.decide > 1 ? 's' : ''}`);
      if (c.unreadForOwner) bits.push(`${c.unreadForOwner} message${c.unreadForOwner > 1 ? 's' : ''}`);
      title = `Tower · ${s.meta.project}`;
      body = bits.length ? bits.join(' · ') + ' waiting on you' : 'All clear — an agent reported in.';
    } catch { /* offline: generic */ }
    await self.registration.showNotification(title, {
      body, icon: '/icon.svg', badge: '/icon.svg', tag: 'tower', renotify: false,
    });
  })());
});

self.addEventListener('notificationclick', (e) => {
  e.notification.close();
  e.waitUntil(self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then(ws => {
    for (const w of ws) if ('focus' in w) return w.focus();
    return self.clients.openWindow('/#now');
  }));
});
