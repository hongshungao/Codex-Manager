#[derive(Debug, Clone)]
pub(super) enum RequestLogQuery {
    All,
    GlobalLike(String),
    FieldLike { column: &'static str, pattern: String },
    FieldExact { column: &'static str, value: String },
    StatusExact(i64),
    StatusRange(i64, i64),
}

pub(super) fn parse_request_log_query(query: Option<&str>) -> RequestLogQuery {
    let Some(raw) = query.map(str::trim).filter(|v| !v.is_empty()) else {
        return RequestLogQuery::All;
    };

    // 中文注释：优先解析字段前缀（如 method:/status:），不这样做会把所有搜索都退化为多列 OR LIKE，数据量上来后会明显变慢。
    if let Some(parsed) = parse_prefixed_request_log_query(raw) {
        return parsed;
    }

    RequestLogQuery::GlobalLike(format!("%{}%", raw))
}

fn parse_prefixed_request_log_query(raw: &str) -> Option<RequestLogQuery> {
    let (prefix, value) = raw.split_once(':')?;
    let normalized_prefix = prefix.trim().to_ascii_lowercase();
    let normalized_value = value.trim();
    if normalized_value.is_empty() {
        return None;
    }
    let (is_exact, needle) = parse_match_mode(normalized_value)?;

    match normalized_prefix.as_str() {
        "account" | "account_id" => Some(parse_field_query("account_id", is_exact, needle)),
        "path" | "request_path" => Some(parse_field_query("request_path", is_exact, needle)),
        "method" => Some(parse_field_query("method", is_exact, needle)),
        "model" => Some(parse_field_query("model", is_exact, needle)),
        "reasoning" | "reason" => Some(parse_field_query("reasoning_effort", is_exact, needle)),
        "error" => Some(parse_field_query("error", is_exact, needle)),
        "key" | "key_id" => Some(parse_field_query("key_id", is_exact, needle)),
        "upstream" | "url" => Some(parse_field_query("upstream_url", is_exact, needle)),
        "status" => parse_status_query(needle),
        _ => None,
    }
}

fn parse_match_mode(raw: &str) -> Option<(bool, &str)> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(exact) = value.strip_prefix('=') {
        let exact = exact.trim();
        if exact.is_empty() {
            return None;
        }
        return Some((true, exact));
    }
    Some((false, value))
}

fn parse_field_query(column: &'static str, is_exact: bool, value: &str) -> RequestLogQuery {
    if is_exact {
        return RequestLogQuery::FieldExact {
            column,
            value: value.to_string(),
        };
    }
    RequestLogQuery::FieldLike {
        column,
        pattern: format!("%{}%", value),
    }
}

fn parse_status_query(raw: &str) -> Option<RequestLogQuery> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.len() == 3 && normalized.ends_with("xx") {
        let digit = normalized.chars().next()?.to_digit(10)? as i64;
        let start = digit * 100;
        return Some(RequestLogQuery::StatusRange(start, start + 99));
    }

    normalized
        .parse::<i64>()
        .ok()
        .map(RequestLogQuery::StatusExact)
}

#[cfg(test)]
mod tests {
    use super::{parse_request_log_query, RequestLogQuery};

    #[test]
    fn prefixed_field_query_supports_exact_mode() {
        let query = parse_request_log_query(Some("method:=POST"));
        assert!(matches!(
            query,
            RequestLogQuery::FieldExact {
                column: "method",
                value
            } if value == "POST"
        ));
    }

    #[test]
    fn prefixed_field_query_keeps_like_mode_by_default() {
        let query = parse_request_log_query(Some("key:key-alpha"));
        assert!(matches!(
            query,
            RequestLogQuery::FieldLike {
                column: "key_id",
                pattern
            } if pattern == "%key-alpha%"
        ));
    }

    #[test]
    fn prefixed_account_query_supports_alias() {
        let query = parse_request_log_query(Some("account:acc-1"));
        assert!(matches!(
            query,
            RequestLogQuery::FieldLike {
                column: "account_id",
                pattern
            } if pattern == "%acc-1%"
        ));
    }
}
