// /api/trial — server-side trial tracking for ChannelLetters.app
//
// POST { hwHash: "<sha256(HW_UUID).hex()>", version: "1.0.15" }
// Returns: { ok, trialStartSec, daysLeft, trialDays, expired, nowSec }

import { kv } from '@vercel/kv';

const TRIAL_DAYS = 14;

const CORS = {
    'Access-Control-Allow-Origin':  '*',
    'Access-Control-Allow-Methods': 'POST, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type',
    'Access-Control-Max-Age':       '86400',
    'Cache-Control':                'no-store',
};

export default async function handler(req, res) {
    for (const [k, v] of Object.entries(CORS)) res.setHeader(k, v);
    if (req.method === 'OPTIONS') return res.status(204).end();
    if (req.method !== 'POST')    return res.status(405).json({ ok: false, err: 'method_not_allowed' });

  const body = typeof req.body === 'string' ? JSON.parse(req.body || '{}') : (req.body || {});
    const hwHash  = (body.hwHash  || '').toString().toLowerCase();
    const version = (body.version || '').toString().slice(0, 32);
    if (!/^[a-f0-9]{64}$/.test(hwHash)) return res.status(400).json({ ok: false, err: 'bad_hwhash' });

  const key = `trial:${hwHash}`;
    const now = Math.floor(Date.now() / 1000);

  let entry;
    try { entry = await kv.get(key); }
    catch { return res.status(503).json({ ok: false, err: 'kv_unavailable' }); }

  if (!entry) {
        entry = { firstSeenSec: now, firstVersion: version, lastSeenSec: now, lastVersion: version, hits: 1 };
        try { await kv.set(key, entry); }
        catch { return res.status(503).json({ ok: false, err: 'kv_write_failed' }); }
  } else {
        entry.lastSeenSec = now;
        entry.lastVersion = version;
        entry.hits = (entry.hits || 0) + 1;
        kv.set(key, entry).catch(() => {});
  }

  const daysLeft = Math.max(0, Math.ceil(TRIAL_DAYS - (now - entry.firstSeenSec) / 86400));
    return res.status(200).json({
          ok: true,
          trialStartSec: entry.firstSeenSec,
          daysLeft, trialDays: TRIAL_DAYS,
          expired: daysLeft === 0,
          nowSec: now,
    });
}
