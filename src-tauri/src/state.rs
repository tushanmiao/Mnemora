use reqwest::Client;
use std::time::Duration;

/** Tauri 全局共享状态。HTTP Client 在整个应用生命周期内复用。 */
pub struct AppState {
    pub http: Client,
}

impl AppState {
    pub fn new() -> Result<Self, String> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("Mnemora/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| format!("Failed to create HTTP client: {error}"))?;

        Ok(Self { http })
    }
}
