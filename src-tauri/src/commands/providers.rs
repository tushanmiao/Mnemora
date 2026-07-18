use tauri::State;

use crate::{
    ai::{
        providers,
        types::{ConnectionTestResult, ProviderConnectionInput},
    },
    state::AppState,
};

/** 用户点击“获取模型”后调用；不会由启动、保存或页面打开自动触发。 */
#[tauri::command]
pub async fn fetch_provider_models(
    state: State<'_, AppState>,
    provider: ProviderConnectionInput,
) -> Result<Vec<String>, String> {
    providers::fetch_models(&state.http, &provider).await
}

/** 用户点击“测试连接”后调用；只发送一次请求，不重试，也不轮换 Key。 */
#[tauri::command]
pub async fn test_provider_connection(
    state: State<'_, AppState>,
    provider: ProviderConnectionInput,
) -> Result<ConnectionTestResult, String> {
    Ok(providers::test_connection(&state.http, &provider).await)
}
