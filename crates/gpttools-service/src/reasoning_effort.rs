pub(crate) fn normalize_reasoning_effort(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "low" => Some("low"),
        "medium" => Some("medium"),
        "high" => Some("high"),
        "xhigh" => Some("xhigh"),
        // 兼容历史写法；统一改写为官方使用的 xhigh，避免不同拼写在上游行为不一致。
        "extra_high" => Some("xhigh"),
        _ => None,
    }
}

pub(crate) fn normalize_reasoning_effort_owned(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .and_then(normalize_reasoning_effort)
        .map(str::to_string)
}
