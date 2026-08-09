# PLDDS Dashboard (Phase 11)

Read-only React + TypeScript UI over the backend (`server/`): fleet
overview, device detail, boot history, report detail.

## Run it

```bash
cp .env.example .env.local   # edit VITE_API_BASE_URL if the backend
                              # isn't on http://localhost:8000
npm install
npm run dev                  # http://localhost:3000
```

Or from the repo root: `make dashboard` (does the same two steps), or
`scripts/dev.sh` to bring up Postgres + the backend + this together.

The backend needs `PLDDS_DASHBOARD_ORIGINS` to include this app's
origin (defaults to `http://localhost:3000` on both sides) — see
`server/.env.example`.

## Pages

- `/` — fleet overview: every registered device, last seen, latest
  score, a client-side filter box, and pagination once you pass 50
  devices. Auto-refreshes every 15s (toggle in the toolbar).
- `/devices/:deviceId` — one device's identity plus its full boot
  history (every report it has submitted, newest first, paginated).
- `/reports/:reportId` — one report: summary counts, then all nine
  diagnostic sections (cpu / memory / kernel / storage / pci / gpu /
  network / usb / sensors), each collapsible, each showing both the
  hardware info the collector read and the PASS/WARN/FAIL results for
  that section. Sections with a FAIL or WARN result start expanded;
  clean sections start collapsed so a long report is scannable.

## Notes

- No write paths. Every call in `src/api.ts` is a GET — this mirrors
  `docs/architecture.md`'s "Dashboard: read-only view over backend
  data."
- `src/types.ts` is hand-mirrored from `server/app/schemas.py` /
  `schemas/diagnostic-report.schema.json`, the same way `schemas.py`
  itself is hand-mirrored from the JSON schema. Keep it in sync by hand
  when either of those change.
- Built and reviewed without a network-enabled sandbox to run
  `npm install` against — dependency versions in `package.json` are
  pinned to real published releases, but this repo's first `npm install`
  here is a good moment to also run `npm run lint` (`tsc --noEmit`)
  before trusting it further.
