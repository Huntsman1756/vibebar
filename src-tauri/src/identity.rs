pub fn normalize_provider_id(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

pub fn provider_label(id: &str) -> String {
    match id {
        "nan" => "NaN".into(),
        "opencode-go" => "OpenCode Go".into(),
        other => other
            .split(['-', '_'])
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_provider_id, provider_label};

    #[test]
    fn normalizes_and_labels_known_and_unknown_providers() {
        assert_eq!(normalize_provider_id("NaN"), "nan");
        assert_eq!(normalize_provider_id("opencode-go"), "opencode-go");
        assert_eq!(normalize_provider_id("custom-provider"), "custom-provider");
        assert_eq!(provider_label("nan"), "NaN");
        assert_eq!(provider_label("opencode-go"), "OpenCode Go");
    }
}
