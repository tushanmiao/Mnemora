use reqwest::{Client, RequestBuilder};
use serde_json::Value;

use crate::ai::http::endpoint_url;

pub fn model_list_request(client: &Client, base_url: &str) -> Result<RequestBuilder, String> {
    let mut url = endpoint_url(base_url, "models")?;
    url.query_pairs_mut().append_pair("limit", "1000");
    Ok(client.get(url).header("anthropic-version", "2023-06-01"))
}

pub fn parse_models(value: &Value) -> Result<Vec<String>, String> {
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "Invalid Anthropic model list: missing data array".to_string())?;

    Ok(data
        .iter()
        .filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string))
        .collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn parses_anthropic_models() {
        let models = super::parse_models(&json!({
            "data": [{ "id": "claude-sonnet" }, { "id": "claude-haiku" }]
        }))
        .unwrap();
        assert_eq!(models, vec!["claude-sonnet", "claude-haiku"]);
    }
}
