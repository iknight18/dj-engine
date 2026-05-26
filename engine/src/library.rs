//! Library loader: read all sidecars from a directory, drop unusable entries.

use std::path::{Path, PathBuf};

use tracing::warn;
use walkdir::WalkDir;

use crate::error::{EngineError, Result};
use crate::sidecar::Sidecar;

#[derive(Debug, Clone)]
pub struct LoadedTrack {
    pub sidecar_path: PathBuf,
    pub sidecar: Sidecar,
}

pub fn load_library(sidecars_dir: &Path) -> Result<Vec<LoadedTrack>> {
    let mut tracks = Vec::new();
    for entry in WalkDir::new(sidecars_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match load_one(entry.path()) {
            Ok(Some(track)) => tracks.push(track),
            Ok(None) => { /* dropped silently with warning logged in load_one */ }
            Err(e) => warn!(
                path = %entry.path().display(),
                error = %e,
                "skipping sidecar (parse error)"
            ),
        }
    }
    if tracks.is_empty() {
        return Err(EngineError::EmptyLibrary);
    }
    Ok(tracks)
}

fn load_one(path: &Path) -> Result<Option<LoadedTrack>> {
    let body = std::fs::read_to_string(path)?;
    let sc: Sidecar = serde_json::from_str(&body)?;

    if let Some(err) = &sc.error {
        warn!(
            path = %path.display(),
            error = %err,
            "skipping sidecar (analyzer-reported error)"
        );
        return Ok(None);
    }
    if !sc.is_playable() {
        warn!(path = %path.display(), "skipping sidecar (incomplete fields)");
        return Ok(None);
    }
    if !PathBuf::from(&sc.source_path).exists() {
        warn!(
            sidecar = %path.display(),
            source = %sc.source_path,
            "skipping sidecar (audio file not found on disk)"
        );
        return Ok(None);
    }
    Ok(Some(LoadedTrack {
        sidecar_path: path.to_path_buf(),
        sidecar: sc,
    }))
}
