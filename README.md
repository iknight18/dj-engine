# DJ Engine

Two-program system that DJs a directory of audio files and streams the live mix to Icecast.

- **`analyzer/`** — Python CLI: scans music, writes one JSON sidecar per track.
- **`engine/`** — Rust binary: reads sidecars, beatmatches + harmonically mixes, streams to Icecast.
- **`schema/sidecar.schema.json`** — the contract between the two halves.
- **`deploy/`** — Kubernetes manifests for Rancher deployment.

See `docs/superpowers/specs/` for the design and `docs/superpowers/plans/` for implementation plans.

## Dev quickstart

### Analyzer

```bash
cd analyzer
python -m venv .venv
.venv\Scripts\activate
pip install -e ".[dev]"
dj-analyze C:/path/to/music ./analyzed
```

Each track in `C:/path/to/music` (recursive) gets a `.json` sidecar mirrored under `./analyzed`. Re-running is cheap (only new/changed tracks re-analyzed). See `schema/sidecar.schema.json` for the output format.

### Engine

See plan `docs/superpowers/plans/2026-05-26-dj-engine-02-engine.md`.
