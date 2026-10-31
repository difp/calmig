//! Offline, value-free summary of selected Instagram HAR request shapes.
use reqwest::Url;
use serde_json::{json, Value};
use std::{collections::BTreeSet, env, fs::File, io::Read};

const LIMIT: u64 = 100 * 1024 * 1024;
const KEYS: &[&str] = &[
    "after",
    "before",
    "end_cursor",
    "has_next_page",
    "page_info",
    "edges",
    "next_max_id",
    "max_id",
    "pagination_source",
    "paginationSource",
    "variant",
    "media_type",
    "carousel_media",
    "taken_at",
    "is_paid_partnership",
    "sponsored",
    "user",
    "items",
    "node",
    "data",
    "following",
    "timeline",
];

fn keys(value: &Value) -> Vec<&str> {
    let mut found = BTreeSet::new();
    let mut pending = vec![value];
    for _ in 0..100_000 {
        let Some(item) = pending.pop() else { break };
        match item {
            Value::Object(object) => {
                for (key, value) in object {
                    if KEYS.contains(&key.as_str()) {
                        found.insert(key.as_str());
                    }
                    pending.push(value);
                }
            }
            Value::Array(array) => pending.extend(array),
            _ => {}
        }
    }
    found.into_iter().collect()
}
fn summary(har: &Value) -> Value {
    let entries = har.pointer("/log/entries").and_then(Value::as_array);
    let mut result = Vec::new();
    for entry in entries.into_iter().flatten() {
        let Some(url) = entry
            .pointer("/request/url")
            .and_then(Value::as_str)
            .and_then(|s| Url::parse(s).ok())
        else {
            continue;
        };
        if !matches!(
            url.host_str(),
            Some("www.instagram.com" | "i.instagram.com")
        ) {
            continue;
        }
        let endpoint = match url.path() {
            "/graphql/query" | "/graphql/query/" => "graphql",
            "/api/v1/feed/timeline/" => "timeline",
            _ => continue,
        };
        let post = entry
            .pointer("/request/postData/text")
            .and_then(Value::as_str)
            .unwrap_or("");
        let mut parameters: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        if let Ok(form) = Url::parse(&format!("https://localhost/?{post}")) {
            parameters.extend(form.query_pairs().into_owned());
        }
        let variable = parameters
            .into_iter()
            .find(|(key, _)| key == "variables")
            .and_then(|(_, value)| serde_json::from_str::<Value>(&value).ok())
            .unwrap_or(Value::Null);
        let content = entry.pointer("/response/content").unwrap_or(&Value::Null);
        let body = if content.get("encoding").and_then(Value::as_str) == Some("base64") {
            None
        } else {
            content
                .get("text")
                .and_then(Value::as_str)
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
        };
        let response_keys = body.as_ref().map(keys).unwrap_or_default();
        let status = entry
            .pointer("/response/status")
            .and_then(Value::as_u64)
            .filter(|s| (100..=599).contains(s));
        result.push(json!({"endpoint": endpoint, "status":status, "request_keys": keys(&variable), "following_marker_in_variables": variable.to_string().to_lowercase().contains("following"), "known_response_keys_found": !response_keys.is_empty(), "response_keys":response_keys}));
    }
    Value::Array(result)
}
fn main() {
    let run = || -> Result<(), ()> {
        let mut args = env::args_os().skip(1);
        let path = args.next().ok_or(())?;
        if args.next().is_some() {
            return Err(());
        }
        let mut data = Vec::new();
        File::open(path)
            .map_err(|_| ())?
            .take(LIMIT + 1)
            .read_to_end(&mut data)
            .map_err(|_| ())?;
        if data.len() as u64 > LIMIT {
            return Err(());
        }
        let har: Value = serde_json::from_slice(&data).map_err(|_| ())?;
        println!(
            "{}",
            serde_json::to_string_pretty(&summary(&har)).map_err(|_| ())?
        );
        Ok(())
    };
    if run().is_err() {
        eprintln!("Provide one valid HAR file (maximum 100 MiB).");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn excludes_values_and_untrusted_origins() {
        let har = json!({"log":{"entries":[{"request":{"url":"https://www.instagram.com/graphql/query/?variables=%7B%22after%22%3A%22PRIVATE_SECRET%22%2C%22variant%22%3A%22following%22%7D", "headers":[{"value":"PRIVATE_SECRET"}]},"response":{"status":200,"content":{"text":"{\"data\":{\"page_info\":{\"end_cursor\":\"PRIVATE_SECRET\"}}}"}}},{"request":{"url":"https://example.com/graphql/query"}}]}});
        let result = summary(&har);
        assert!(!result.to_string().contains("PRIVATE_SECRET"));
        assert_eq!(result.as_array().unwrap().len(), 1);
        assert_eq!(result[0]["following_marker_in_variables"], true);
        assert!(result[0]["response_keys"]
            .as_array()
            .unwrap()
            .contains(&json!("end_cursor")));
    }
}
