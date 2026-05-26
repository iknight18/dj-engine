# DJ Engine

Two-program system that DJs a directory of audio files and streams the beatmatched mix to Icecast.

- **`analyzer/`** — Python CLI: scans music, writes one JSON sidecar per track.
- **`engine/`** — Rust binary: reads sidecars, beatmatches + harmonically mixes, streams to Icecast via ffmpeg.
- **`schema/sidecar.schema.json`** — the contract between the two halves.
- **`deploy/`** — Kubernetes manifests for Rancher deployment.
- **`docs/superpowers/specs/`** — design docs.
- **`docs/superpowers/plans/`** — implementation plans.

## Dev quickstart

### Prerequisites

- Python 3.11+
- Rust toolchain (`rustup`)
- `ffmpeg.exe` on `PATH`
- Icecast (configured to listen on `:27897`)

### Analyzer

```bash
cd analyzer
python -m venv .venv
.venv\Scripts\activate
pip install -e ".[dev]"
dj-analyze C:/path/to/music ../analyzed
```

Each track gets a `.json` sidecar mirrored under `../analyzed`. Re-running is cheap (incremental). See `schema/sidecar.schema.json` for the output format.

### Engine

Set up Icecast and start it on `:27897`. Then:

```powershell
$env:ICECAST_URL = "icecast://localhost:27897/dj.mp3"
$env:ICECAST_USER = "source"
$env:ICECAST_PASSWORD = "hackme"
cd engine
cargo run --release -- --sidecars ../analyzed --music-root C:/path/to/music
```

Open `http://localhost:27897/dj.mp3` in VLC to listen.

## Deployment to Rancher

```bash
docker build -f analyzer/Dockerfile -t dj-analyzer:0.1.0 .
docker build -f engine/Dockerfile   -t dj-engine:0.1.0   .

# (push to your registry, then)
kubectl apply -f deploy/analyzer-job.yaml
# Edit deploy/icecast-secret.example.yaml → icecast-secret.yaml with real creds
kubectl apply -f deploy/icecast-secret.yaml
kubectl apply -f deploy/engine-deployment.yaml
```

## Tests

```bash
# Analyzer
cd analyzer && pytest -v

# Engine
cd engine && cargo test
```

## Spec & plans

- Spec: `docs/superpowers/specs/2026-05-26-dj-engine-design.md`
- Plan 1 (Analyzer): `docs/superpowers/plans/2026-05-26-dj-engine-01-analyzer.md`
- Plan 2 (Engine):   `docs/superpowers/plans/2026-05-26-dj-engine-02-engine.md`
