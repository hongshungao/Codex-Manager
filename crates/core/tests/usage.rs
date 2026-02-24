use codexmanager_core::usage::{parse_usage_snapshot, usage_endpoint};
use serde_json::json;

#[test]
fn usage_snapshot_parsed() {
    let payload = json!({
        "rate_limit": {
            "primary_window": {
                "used_percent": 25.0,
                "limit_window_seconds": 900,
                "reset_at": 1730947200
            },
            "secondary_window": {
                "used_percent": 80.0,
                "limit_window_seconds": 120,
                "reset_at": 1730947260
            }
        },
        "credits": { "balance": 12.5 }
    });

    let snap = parse_usage_snapshot(&payload);
    assert_eq!(snap.used_percent, Some(25.0));
    assert_eq!(snap.window_minutes, Some(15));
    assert_eq!(snap.resets_at, Some(1730947200));
    assert_eq!(snap.secondary_used_percent, Some(80.0));
    assert_eq!(snap.secondary_window_minutes, Some(2));
    assert_eq!(snap.secondary_resets_at, Some(1730947260));
    assert!(snap.credits_json.as_ref().unwrap().contains("balance"));

    let url = usage_endpoint("https://chatgpt.com");
    assert_eq!(url, "https://chatgpt.com/backend-api/wham/usage");
}
