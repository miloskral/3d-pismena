// /api/visits — shared global visitor counter (Vercel KV)
//
// GET /api/visits        -> { count }   (reads the shared total)
// GET /api/visits?hit=1   -> { count }   (atomically +1, then returns it)
//
// Uses the same Vercel KV as /api/trial, so no extra setup is needed.

import { kv } from '@vercel/kv';

// Baseline for first-run seed (and the floor below which the counter is
// auto-reset, so a re-platform from counterapi.dev → Vercel KV doesn't make
// the displayed number drop). On 2026-05-31 we migrated from counterapi.dev;
// on 2026-06-13 we unified the homepage onto this endpoint and re-baselined
// to 418 (= the value the old counterapi.dev-based counter was showing on
// the day of the cut-over, so visitors don't see a regression).
const BASE = 418;

const CORS = {
  'Access-Control-Allow-Origin': '*',
  'Access-Control-Allow-Methods': 'GET, OPTIONS',
  'Access-Control-Allow-Headers': 'Content-Type',
  'Cache-Control': 'no-store',
};

export default async function handler(req, res) {
  for (const [k, v] of Object.entries(CORS)) res.setHeader(k, v);
  if (req.method === 'OPTIONS') return res.status(204).end();

  const key = 'visits:total';
  try {
    let count = await kv.get(key);
    if (typeof count !== 'number' || count < BASE) {
      // First run, or BASE was bumped (e.g. cut-over from another counter
      // backend). Seed/floor the stored value so the displayed number can
      // never go BACKWARDS — visitors expect monotonic growth.
      count = BASE;
      await kv.set(key, count);
    }
    if (req.query && req.query.hit === '1') {
      count = await kv.incr(key);        // atomic +1
    }
    return res.status(200).json({ count });
  } catch (e) {
    return res.status(503).json({ count: null, err: 'kv_unavailable' });
  }
}
