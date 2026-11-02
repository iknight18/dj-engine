# DJ Engine

An automatic DJ: point it at a folder of music and it plays a continuous, beatmatched, harmonically mixed
set, streamed live to Icecast. It picks what to play next and how to blend it, the way a DJ would.

## How it works

**Analyzer (Python)** listens to every track once and writes what it learns to a JSON sidecar:
tempo and a per-bar beat grid (madmom), downbeats and phrase boundaries, musical key, energy curve,
frequency bands, cue points for the intro and outro, whether the track is mixable at all, and stems
(drums, bass, vocals, other) split with Demucs.

**Engine (Rust)** runs two decks like a real mixer. For each change it:

- picks the next track by tempo, key compatibility on the Camelot wheel, energy and recent plays;
- picks a transition "recipe" that suits the pair: a filter fade, a drop, a loop-out, a hard cut, or a
  stem-aware move (a rhythm swap that brings the new drums in before the melody, or a loop bridge that
  holds a phrase of the outgoing track while the next one takes over);
- time-stretches and phase-aligns the incoming track on the downbeat, automates EQ, filters and faders,
  and lands the change on a phrase boundary;
- encodes the mix with ffmpeg and streams it to Icecast, with now-playing metadata.

The two halves only share `schema/sidecar.schema.json`, so either can change without the other.

## Layout

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

## Design docs

- Spec: `docs/superpowers/specs/2026-05-26-dj-engine-design.md`
- Plan 1 (Analyzer): `docs/superpowers/plans/2026-05-26-dj-engine-01-analyzer.md`
- Plan 2 (Engine):   `docs/superpowers/plans/2026-05-26-dj-engine-02-engine.md`

## Licence

MIT, see `LICENSE`.
