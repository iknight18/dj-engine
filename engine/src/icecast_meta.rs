//! Fire-and-forget Icecast metadata updates (now-playing).

use reqwest::Client;
use tracing::warn;

#[derive(Debug, Clone)]
pub struct MetadataConfig {
    pub admin_url: String,  // e.g., http://host:27897
    pub admin_user: String, // e.g., "admin"
    pub admin_password: String,
    pub mount: String, // e.g., "/dj.mp3"
}

pub async fn update_now_playing(client: &Client, cfg: &MetadataConfig, song: &str) {
    let url = format!(
        "{}/admin/metadata?mount={}&mode=updinfo&song={}",
        cfg.admin_url.trim_end_matches('/'),
        urlencoding::encode(&cfg.mount),
        urlencoding::encode(song),
    );
    let result = client
        .get(&url)
        .basic_auth(&cfg.admin_user, Some(&cfg.admin_password))
        .send()
        .await;
    match result {
        Ok(resp) if resp.status().is_success() => { /* ok */ }
        Ok(resp) => warn!(status = %resp.status(), "metadata update failed"),
        Err(e) => warn!(error = %e, "metadata update HTTP error"),
    }
}
