use std::time::Duration;

use reqwest::{Client, Proxy};
use tauri_plugin_updater::UpdaterBuilder;

use crate::settings::app_types::{UpdateProxyMode, UpdateProxySettings};

pub fn build_update_client(settings: &UpdateProxySettings) -> Result<Client, String> {
    let mut builder = Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(120))
        .user_agent(concat!("Mnemora/", env!("CARGO_PKG_VERSION")))
        .no_proxy();

    builder = match settings.mode {
        UpdateProxyMode::System => match system_proxy_url()? {
            Some(url) => builder.proxy(proxy_from_url(&url)?),
            None => builder,
        },
        UpdateProxyMode::Direct => builder,
        UpdateProxyMode::Manual => {
            let url = settings.manual_url()?;
            builder.proxy(proxy_from_url(&url)?)
        }
    };

    builder
        .build()
        .map_err(|error| format!("Unable to create update client: {error}"))
}

pub fn configure_signed_updater(
    builder: UpdaterBuilder,
    settings: &UpdateProxySettings,
) -> Result<UpdaterBuilder, String> {
    match settings.mode {
        UpdateProxyMode::System => match system_proxy_url()? {
            Some(url) => Ok(builder.proxy(url)),
            None => Ok(builder.no_proxy()),
        },
        UpdateProxyMode::Direct => Ok(builder.no_proxy()),
        UpdateProxyMode::Manual => Ok(builder.proxy(settings.manual_url()?)),
    }
}

fn proxy_from_url(url: &reqwest::Url) -> Result<Proxy, String> {
    Proxy::all(url.as_str()).map_err(|error| format!("Unable to configure update proxy: {error}"))
}

fn system_proxy_url() -> Result<Option<reqwest::Url>, String> {
    for name in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"] {
        if let Ok(value) = std::env::var(name) {
            let value = value.trim();
            if !value.is_empty() {
                return parse_system_proxy(value).map(Some);
            }
        }
    }
    windows_system_proxy_url()
}

fn parse_system_proxy(value: &str) -> Result<reqwest::Url, String> {
    let entries = value.split(';').map(str::trim).collect::<Vec<_>>();
    let named_proxy = |scheme: &str| {
        entries.iter().find_map(|entry| {
            let (name, address) = entry.split_once('=')?;
            name.eq_ignore_ascii_case(scheme).then_some(address.trim())
        })
    };
    let https_value = named_proxy("https")
        .or_else(|| named_proxy("http"))
        .unwrap_or(value.trim());
    let normalized = if https_value.contains("://") {
        https_value.to_string()
    } else {
        format!("http://{https_value}")
    };
    let url = reqwest::Url::parse(&normalized)
        .map_err(|error| format!("Invalid system update proxy URL: {error}"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("System update proxy must be a valid HTTP or HTTPS URL".to_string());
    }
    Ok(url)
}

#[cfg(target_os = "windows")]
fn windows_system_proxy_url() -> Result<Option<reqwest::Url>, String> {
    let Ok(settings) = windows_registry::CURRENT_USER
        .open("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
    else {
        return Ok(None);
    };
    if settings.get_u32("ProxyEnable").unwrap_or(0) == 0 {
        return Ok(None);
    }
    let Ok(proxy_server) = settings.get_string("ProxyServer") else {
        return Ok(None);
    };
    parse_system_proxy(&proxy_server).map(Some)
}

#[cfg(not(target_os = "windows"))]
fn windows_system_proxy_url() -> Result<Option<reqwest::Url>, String> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::parse_system_proxy;

    #[test]
    fn parses_windows_proxy_server_formats() {
        assert_eq!(
            parse_system_proxy("127.0.0.1:7890").unwrap().as_str(),
            "http://127.0.0.1:7890/"
        );
        assert_eq!(
            parse_system_proxy("http=127.0.0.1:8080;https=127.0.0.1:8443")
                .unwrap()
                .as_str(),
            "http://127.0.0.1:8443/"
        );
        assert!(parse_system_proxy("socks5://127.0.0.1:7890").is_err());
    }
}
