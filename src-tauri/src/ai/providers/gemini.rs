use reqwest::{Client, RequestBuilder};
use serde_json::Value;

use crate::ai::http::endpoint_url;

pub fn model_list_request(client: &Client, base_url: &str) -> Result<RequestBuilder, String> {
    Ok(client.get(endpoint_url(base_url, "models")?))
}

pub fn parse_models(value: &Value) -> Result<Vec<String>, String> {
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| "Invalid Gemini model list: missing models array".to_string())?;

    Ok(models
        .iter()
        .filter_map(|item| item.get("name").and_then(Value::as_str))
        .map(|name| name.strip_prefix("models/").unwrap_or(name).to_string())
        .collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn parses_gemini_models_without_models_prefix() {
        let models = super::parse_models(&json!({
            "models": [
                { "name": "models/gemini-pro" },
                { "name": "models/gemini-flash" }
            ]
        }))
        .unwrap();
        assert_eq!(models, vec!["gemini-pro", "gemini-flash"]);
    }
}
