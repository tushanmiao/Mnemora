//! 统一的出站网络代理策略。
//!
//! reqwest 的“系统代理”只读取进程环境变量；Windows GUI 进程通常拿不到
//! 代理软件写入注册表的设置。这里把环境变量、Windows Internet Settings、
//! 直连和手动代理收敛为同一套解析与 ClientBuilder 配置，供网页工具和更新器复用。

use reqwest::{ClientBuilder, Proxy, Url};
use serde::Serialize;

use crate::settings::app_types::{UpdateProxyMode, UpdateProxySettings};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedProxy {
    pub mode: UpdateProxyMode,
    pub source: &'static str,
    pub address: Option<String>,
    #[serde(skip)]
    url: Option<Url>,
}

impl ResolvedProxy {
    pub fn url(&self) -> Option<&Url> {
        self.url.as_ref()
    }
}

pub fn configure_reqwest_builder(
    builder: ClientBuilder,
    settings: &UpdateProxySettings,
) -> Result<(ClientBuilder, ResolvedProxy), String> {
    let resolved = resolve_proxy(settings)?;
    // 先关闭 reqwest 对环境变量的隐式读取，确保三种模式完全由设置决定。
    let mut builder = builder.no_proxy();
    if let Some(url) = resolved.url.as_ref() {
        builder = builder.proxy(proxy_from_url(url)?);
    }
    Ok((builder, resolved))
}

pub fn resolve_proxy(settings: &UpdateProxySettings) -> Result<ResolvedProxy, String> {
    match settings.mode {
        UpdateProxyMode::Direct => Ok(ResolvedProxy {
            mode: settings.mode,
            source: "direct",
            address: None,
            url: None,
        }),
        UpdateProxyMode::Manual => {
            let url = settings.manual_url()?;
            Ok(resolved(settings.mode, "manual", Some(url)))
        }
        UpdateProxyMode::System => {
            if let Some(url) = environment_proxy_url()? {
                return Ok(resolved(settings.mode, "environment", Some(url)));
            }
            if let Some(url) = windows_system_proxy_url()? {
                return Ok(resolved(settings.mode, "windowsSystem", Some(url)));
            }
            Ok(resolved(settings.mode, "systemDirect", None))
        }
    }
}

fn resolved(mode: UpdateProxyMode, source: &'static str, url: Option<Url>) -> ResolvedProxy {
    let address = url.as_ref().map(redacted_proxy_address);
    ResolvedProxy {
        mode,
        source,
        address,
        url,
    }
}

fn redacted_proxy_address(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port_or_known_default() {
        Some(port) => format!("{}://{host}:{port}", url.scheme()),
        None => format!("{}://{host}", url.scheme()),
    }
}

fn proxy_from_url(url: &Url) -> Result<Proxy, String> {
    Proxy::all(url.as_str()).map_err(|error| format!("无法配置网络代理：{error}"))
}

fn environment_proxy_url() -> Result<Option<Url>, String> {
    for name in [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        if let Ok(value) = std::env::var(name) {
            let value = value.trim();
            if !value.is_empty() {
                return parse_system_proxy(value).map(Some);
            }
        }
    }
    Ok(None)
}

fn parse_system_proxy(value: &str) -> Result<Url, String> {
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
    let url = Url::parse(&normalized).map_err(|error| format!("系统代理地址无效：{error}"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("系统代理必须是有效的 HTTP 或 HTTPS 地址。".to_string());
    }
    Ok(url)
}

#[cfg(target_os = "windows")]
fn windows_system_proxy_url() -> Result<Option<Url>, String> {
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
fn windows_system_proxy_url() -> Result<Option<Url>, String> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use crate::settings::app_types::{UpdateProxyMode, UpdateProxySettings};

    use super::{parse_system_proxy, redacted_proxy_address, resolve_proxy};

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

    #[test]
    fn diagnostics_never_expose_proxy_credentials() {
        let url = reqwest::Url::parse("http://user:secret@proxy.example:8080").unwrap();
        assert_eq!(redacted_proxy_address(&url), "http://proxy.example:8080");
    }

    #[test]
    fn direct_and_manual_modes_are_resolved_deterministically() {
        let direct = resolve_proxy(&UpdateProxySettings {
            mode: UpdateProxyMode::Direct,
            url: String::new(),
        })
        .unwrap();
        assert_eq!(direct.source, "direct");
        assert!(direct.url().is_none());

        let manual = resolve_proxy(&UpdateProxySettings {
            mode: UpdateProxyMode::Manual,
            url: "127.0.0.1:7890".to_string(),
        })
        .unwrap();
        assert_eq!(manual.source, "manual");
        assert_eq!(manual.address.as_deref(), Some("http://127.0.0.1:7890"));
    }
}
