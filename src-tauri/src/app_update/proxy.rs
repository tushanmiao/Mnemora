use std::time::Duration;

use reqwest::Client;
use tauri_plugin_updater::UpdaterBuilder;

use crate::{network, settings::app_types::UpdateProxySettings};

pub fn build_update_client(settings: &UpdateProxySettings) -> Result<Client, String> {
    let builder = Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(120))
        .user_agent(concat!("Mnemora/", env!("CARGO_PKG_VERSION")));
    let (builder, _) = network::configure_reqwest_builder(builder, settings)?;
    builder
        .build()
        .map_err(|error| format!("Unable to create update client: {error}"))
}

pub fn configure_signed_updater(
    builder: UpdaterBuilder,
    settings: &UpdateProxySettings,
) -> Result<UpdaterBuilder, String> {
    let resolved = network::resolve_proxy(settings)?;
    match resolved.url() {
        Some(url) => Ok(builder.proxy(url.clone())),
        None => Ok(builder.no_proxy()),
    }
}
