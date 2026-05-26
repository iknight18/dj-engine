# syntax=docker/dockerfile:1
FROM python:3.11-slim AS base

# System deps: ffmpeg for audio decoding; build essentials for any sdists.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ffmpeg \
        build-essential \
        libsndfile1 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Install in two stages so deps are cached across rebuilds.
COPY analyzer/pyproject.toml /app/analyzer/
RUN pip install --no-cache-dir hatchling

COPY analyzer/ /app/analyzer/
COPY schema/ /app/schema/

RUN pip install --no-cache-dir -e /app/analyzer

# Default invocation: `dj-analyze /music /out --workers $(nproc) --log-format json`.
# Users override via `docker run ... <args>`.
ENTRYPOINT ["dj-analyze"]
CMD ["--help"]
