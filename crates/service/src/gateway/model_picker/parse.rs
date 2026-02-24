use std::collections::HashSet;

use codexmanager_core::rpc::types::ModelOption;
use serde_json::Value;

pub(super) fn parse_model_options(body: &[u8]) -> Vec<ModelOption> {
    let mut items: Vec<ModelOption> = Vec::new();
    let mut seen = HashSet::new();

    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        parse_models_array(value.get("models").and_then(|v| v.as_array()), &mut seen, &mut items);
        parse_data_array(value.get("data").and_then(|v| v.as_array()), &mut seen, &mut items);
    }

    items.sort_by(|a, b| a.slug.cmp(&b.slug));
    items
}

fn parse_models_array(
    models: Option<&Vec<Value>>,
    seen: &mut HashSet<String>,
    items: &mut Vec<ModelOption>,
) {
    let Some(models) = models else {
        return;
    };
    for item in models {
        let slug = item
            .get("slug")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty());
        if let Some(slug) = slug {
            push_model_option(
                slug,
                item.get("title")
                    .or_else(|| item.get("display_name"))
                    .and_then(|v| v.as_str()),
                seen,
                items,
            );
        }
    }
}

fn parse_data_array(
    data: Option<&Vec<Value>>,
    seen: &mut HashSet<String>,
    items: &mut Vec<ModelOption>,
) {
    let Some(data) = data else {
        return;
    };
    for item in data {
        let slug = item
            .get("id")
            .or_else(|| item.get("slug"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty());
        if let Some(slug) = slug {
            push_model_option(
                slug,
                item.get("display_name")
                    .or_else(|| item.get("title"))
                    .and_then(|v| v.as_str()),
                seen,
                items,
            );
        }
    }
}

fn push_model_option(
    slug: &str,
    display_name: Option<&str>,
    seen: &mut HashSet<String>,
    items: &mut Vec<ModelOption>,
) {
    if seen.insert(slug.to_string()) {
        items.push(ModelOption {
            slug: slug.to_string(),
            display_name: display_name.unwrap_or(slug).to_string(),
        });
    }
}
