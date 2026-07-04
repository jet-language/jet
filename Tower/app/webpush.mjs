// Minimal Web Push, std-only. Pushes are PAYLOAD-LESS (no RFC 8291 payload
// encryption needed): the service worker wakes, fetches /api/state, and
// composes the notification locally. Only VAPID (RFC 8292) is implemented:
// an ES256 JWT proving the server's identity to the push service.
import { generateKeyPairSync, createPrivateKey, sign } from 'node:crypto';

const b64u = (buf) => Buffer.from(buf).toString('base64url');

export function generateVapid() {
  const { publicKey, privateKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' });
  const jwk = publicKey.export({ format: 'jwk' });
  // Browser applicationServerKey = 65-byte uncompressed point (0x04 || X || Y)
  const raw = Buffer.concat([Buffer.from([4]), Buffer.from(jwk.x, 'base64url'), Buffer.from(jwk.y, 'base64url')]);
  return {
    publicKey: b64u(raw),
    privateJwk: privateKey.export({ format: 'jwk' }),
  };
}

function vapidJwt(endpoint, privateJwk, publicKey, sub = 'mailto:tower@localhost') {
  const aud = new URL(endpoint).origin;
  const header = b64u(JSON.stringify({ typ: 'JWT', alg: 'ES256' }));
  const claims = b64u(JSON.stringify({ aud, exp: Math.floor(Date.now() / 1000) + 12 * 3600, sub }));
  const input = `${header}.${claims}`;
  const key = createPrivateKey({ key: privateJwk, format: 'jwk' });
  const sig = sign('sha256', Buffer.from(input), { key, dsaEncoding: 'ieee-p1363' });
  return `${input}.${b64u(sig)}`;
}

// Returns { ok, status, gone } — gone=true means the subscription is dead
// (unsubscribed / expired) and should be dropped.
export async function pushTo(subscription, { privateJwk, publicKey }) {
  try {
    const jwt = vapidJwt(subscription.endpoint, privateJwk, publicKey);
    const r = await fetch(subscription.endpoint, {
      method: 'POST',
      headers: {
        TTL: '120',
        Urgency: 'high',
        Authorization: `vapid t=${jwt}, k=${publicKey}`,
        'Content-Length': '0',
      },
    });
    return { ok: r.status < 300, status: r.status, gone: r.status === 404 || r.status === 410 };
  } catch (e) {
    return { ok: false, status: 0, gone: false, error: String(e.message || e) };
  }
}
