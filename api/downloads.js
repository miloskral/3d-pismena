// Download counters for lumisign3d.com — separate macOS / Windows totals,
// shared across all visitors via Vercel KV (same backend approach as /api/visits).
//
//   GET /api/downloads            -> { mac, win }            (read only, no increment)
//   GET /api/downloads?hit=mac    -> increments macOS, then returns { mac, win }
//   GET /api/downloads?hit=win    -> increments Windows, then returns { mac, win }
//
// Requires a Vercel KV (Upstash Redis) store connected to the project — the same
// one used by api/visits.js. No extra env setup needed beyond that.

import { kv } from '@vercel/kv';

const KEY_MAC = 'downloads_mac';
const KEY_WIN = 'downloads_win';

export default async function handler(req, res) {
  res.setHeader('Cache-Control', 'no-store, max-age=0');

  try {
    const hit = (req.query && req.query.hit ? String(req.query.hit) : '').toLowerCase();

    if (hit === 'mac') await kv.incr(KEY_MAC);
    else if (hit === 'win') await kv.incr(KEY_WIN);

    const [mac, win] = await kv.mget(KEY_MAC, KEY_WIN);

    return res.status(200).json({
      mac: Number(mac) || 0,
      win: Number(win) || 0,
    });
  } catch (err) {
    return res.status(500).json({ error: 'counter_unavailable' });
  }
}
