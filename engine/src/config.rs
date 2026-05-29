//! CLI arguments + environment configuration.

use std::path::PathBuf;

use clap::Parser;

use crate::ffmpeg::encoder::EncoderFormat;

#[derive(Debug, Parser)]
#[command(
    name = "dj-engine",
    version,
    about = "Stream a beatmatched mix to Icecast."
)]
pub struct Cli {
    /// Directory containing analyzer sidecar JSONs.
    #[arg(long)]
    pub sidecars: PathBuf,

    /// Root directory of audio files (for resolving sidecar source_path entries).
    #[arg(long)]
    pub music_root: PathBuf,

    /// Optional seed track filename or path; otherwise picked automatically.
    #[arg(long)]
    pub seed_track: Option<String>,

    /// `pretty` or `json` log output.
    #[arg(long, default_value = "pretty")]
    pub log_format: LogFormat,

    /// Force every transition to use this recipe, overriding `pick_recipe`.
    /// Mainly for manual testing of new recipes.
    #[arg(long, value_enum)]
    pub force_recipe: Option<ForceRecipe>,

    /// Penalize recipes used in the last N transitions when scoring (so the same
    /// recipe doesn't fire repeatedly). Default 5. Pass 0 to disable.
    #[arg(long)]
    pub recipe_recency_window: Option<usize>,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum LogFormat {
    Pretty,
    Json,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum, PartialEq, Eq)]
pub enum ForceRecipe {
    EqualPower,
    HardCut,
    BassSwap,
    FilterFade,
    FilterKill,
    LoopOut,
    Drop,
    BassSwapStemAware,
    RhythmSwap,
}

impl ForceRecipe {
    pub fn to_recipe_choice(self) -> crate::recipe::RecipeChoice {
        match self {
            ForceRecipe::EqualPower => crate::recipe::RecipeChoice::EqualPower,
            ForceRecipe::HardCut => crate::recipe::RecipeChoice::HardCut,
            ForceRecipe::BassSwap => crate::recipe::RecipeChoice::BassSwap,
            ForceRecipe::FilterFade => crate::recipe::RecipeChoice::FilterFade,
            ForceRecipe::FilterKill => crate::recipe::RecipeChoice::FilterKill,
            ForceRecipe::LoopOut => crate::recipe::RecipeChoice::LoopOut,
            ForceRecipe::Drop => crate::recipe::RecipeChoice::Drop,
            ForceRecipe::BassSwapStemAware => crate::recipe::RecipeChoice::BassSwapStemAware,
            ForceRecipe::RhythmSwap => crate::recipe::RecipeChoice::RhythmSwap,
        }
    }
}

#[derive(Debug, Clone)]
pub struct IcecastEnv {
    pub url: String,
    pub user: String,
    pub password: String,
    pub format: EncoderFormat,
    pub bitrate: String,
    pub admin_url: String,
    pub admin_user: String,
    pub admin_password: String,
    pub mount: String,
}

impl IcecastEnv {
    pub fn from_env() -> Result<Self, String> {
        let url =
            std::env::var("ICECAST_URL").map_err(|_| "ICECAST_URL is required".to_string())?;
        let password = std::env::var("ICECAST_PASSWORD")
            .map_err(|_| "ICECAST_PASSWORD is required".to_string())?;
        let user = std::env::var("ICECAST_USER").unwrap_or_else(|_| "source".into());
        let format = match std::env::var("ICECAST_FORMAT").as_deref() {
            Ok("opus") => EncoderFormat::Opus,
            _ => EncoderFormat::Mp3,
        };
        let bitrate = std::env::var("ICECAST_BITRATE").unwrap_or_else(|_| "192k".into());
        let mount = extract_mount(&url).unwrap_or_else(|| "/dj.mp3".into());
        let admin_url = std::env::var("ICECAST_ADMIN_URL")
            .ok()
            .or_else(|| derive_admin_url(&url))
            .ok_or("ICECAST_ADMIN_URL could not be derived from ICECAST_URL")?;
        let admin_user = std::env::var("ICECAST_ADMIN_USER").unwrap_or_else(|_| "admin".into());
        let admin_password =
            std::env::var("ICECAST_ADMIN_PASSWORD").unwrap_or_else(|_| password.clone());
        Ok(Self {
            url,
            user,
            password,
            format,
            bitrate,
            admin_url,
            admin_user,
            admin_password,
            mount,
        })
    }
}

fn extract_mount(url: &str) -> Option<String> {
    // icecast://host:port/mount → "/mount"
    let after_scheme = url.strip_prefix("icecast://")?;
    let slash = after_scheme.find('/')?;
    Some(after_scheme[slash..].to_string())
}

fn derive_admin_url(url: &str) -> Option<String> {
    // icecast://host:port/mount → http://host:port
    let after_scheme = url.strip_prefix("icecast://")?;
    let host_port = after_scheme.split('/').next()?;
    Some(format!("http://{host_port}"))
}
