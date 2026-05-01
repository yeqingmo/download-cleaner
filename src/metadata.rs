use anyhow::{Context, Result};
use plist::Value;
use std::io::Cursor;
use std::path::Path;
use url::Url;

const WHERE_FROMS_XATTR: &str = "com.apple.metadata:kMDItemWhereFroms";

pub fn extract_source_domain(path: &Path) -> Result<String> {
    let Some(bytes) = xattr::get(path, WHERE_FROMS_XATTR)
        .with_context(|| format!("读取 xattr 失败: {}", path.display()))?
    else {
        return Ok("未知来源".to_string());
    };

    let value =
        Value::from_reader(Cursor::new(bytes)).context("解析 kMDItemWhereFroms plist 失败")?;
    let mut candidates = Vec::new();
    collect_plist_strings(&value, &mut candidates);

    for candidate in candidates {
        if let Some(domain) = first_domain_in_text(&candidate) {
            return Ok(domain);
        }
    }

    Ok("未知来源".to_string())
}

fn collect_plist_strings(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::String(value) => output.push(value.clone()),
        Value::Array(values) => {
            for value in values {
                collect_plist_strings(value, output);
            }
        }
        Value::Dictionary(values) => {
            for value in values.values() {
                collect_plist_strings(value, output);
            }
        }
        _ => {}
    }
}

fn first_domain_in_text(text: &str) -> Option<String> {
    for token in text.split(|ch: char| {
        ch.is_whitespace() || matches!(ch, '"' | '\'' | '(' | ')' | '<' | '>' | ',')
    }) {
        let trimmed = token.trim_matches(|ch: char| matches!(ch, '.' | ';' | ']' | '['));

        if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
            continue;
        }

        if let Ok(url) = Url::parse(trimmed) {
            if let Some(host) = url.host_str() {
                return Some(host.strip_prefix("www.").unwrap_or(host).to_string());
            }
        }
    }

    None
}
