use reqwest::{Client, RequestBuilder};
use serde_json::Value;

use crate::ai::http::endpoint_url;

pub fn model_list_request(client: &Client, base_url: &str) -> Result<RequestBuilder, String> {
    Ok(client.get(endpoint_url(base_url, "models")?))
}

pub fn parse_models(value: &Value) -> Result<Vec<String>, String> {
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "Invalid OpenAI model list: missing data array".to_string())?;

    Ok(data
        .iter()
        .filter_map(|item| {
            item.as_str()
                .or_else(|| item.get("id").and_then(Value::as_str))
                .map(str::to_string)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn parses_openai_models() {
        let models = super::parse_models(&json!({
            "data": [{ "id": "gpt-5" }, { "id": "gpt-4.1" }]
        }))
        .unwrap();
        assert_eq!(models, vec!["gpt-5", "gpt-4.1"]);
    }
}
