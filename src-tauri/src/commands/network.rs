use std::time::{Duration, Instant};

use reqwest::{redirect::Policy, Client};
use serde::Serialize;
use tauri::State;

use crate::{network, state::AppState};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkConnectionReport {
    proxy_mode: String,
    proxy_source: String,
    proxy_address: Option<String>,
    probes: Vec<NetworkConnectionProbe>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkConnectionProbe {
    id: &'static str,
    ok: bool,
    status_code: Option<u16>,
    duration_ms: u64,
    message: String,
}

#[tauri::command]
pub async fn test_web_network_connection(
    state: State<'_, AppState>,
) -> Result<NetworkConnectionReport, String> {
    let settings = state
        .app_settings
        .read()
        .map_err(|_| "应用设置暂时不可用。".to_string())?
        .update_proxy
        .clone();
    let builder = Client::builder()
        .redirect(Policy::limited(3))
        .connect_timeout(Duration::from_secs(12))
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("Mnemora/", env!("CARGO_PKG_VERSION")));
    let (builder, resolved) = network::configure_reqwest_builder(builder, &settings)?;
    let client = builder
        .build()
        .map_err(|error| format!("无法创建网页连接测试客户端：{error}"))?;

    let (search, page) = tokio::join!(
        probe(
            &client,
            "search",
            "https://html.duckduckgo.com/html/?q=mnemora+connection+test"
        ),
        probe(&client, "page", "https://developers.openai.com/")
    );
    Ok(NetworkConnectionReport {
        proxy_mode: format!("{:?}", resolved.mode).to_ascii_lowercase(),
        proxy_source: resolved.source.to_string(),
        proxy_address: resolved.address,
        probes: vec![search, page],
    })
}

async fn probe(client: &Client, id: &'static str, url: &'static str) -> NetworkConnectionProbe {
    let started = Instant::now();
    match client.get(url).send().await {
        Ok(response) => {
            let status = response.status();
            NetworkConnectionProbe {
                id,
                ok: status.is_success(),
                status_code: Some(status.as_u16()),
                duration_ms: duration_ms(started.elapsed()),
                message: if status.is_success() {
                    "连接成功".to_string()
                } else {
                    format!("服务返回 HTTP {}", status.as_u16())
                },
            }
        }
        Err(error) => NetworkConnectionProbe {
            id,
            ok: false,
            status_code: error.status().map(|status| status.as_u16()),
            duration_ms: duration_ms(started.elapsed()),
            message: if error.is_timeout() {
                "连接超时，请检查代理地址或网络状态。".to_string()
            } else if error.is_connect() {
                "无法建立连接，请确认代理软件正在运行。".to_string()
            } else {
                format!("连接失败：{error}")
            },
        },
    }
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}
