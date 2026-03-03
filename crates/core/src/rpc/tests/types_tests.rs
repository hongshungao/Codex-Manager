use super::AccountSummary;

#[test]
fn account_summary_serialization_matches_compact_contract() {
    let summary = AccountSummary {
        id: "acc-1".to_string(),
        label: "主账号".to_string(),
        group_name: Some("TEAM".to_string()),
        sort: 10,
    };

    let value = serde_json::to_value(summary).expect("serialize account summary");
    let obj = value.as_object().expect("account summary object");

    for key in ["id", "label", "groupName", "sort"] {
        assert!(obj.contains_key(key), "missing key: {key}");
    }

    for key in [
        "workspaceId",
        "workspaceName",
        "note",
        "tags",
        "status",
        "updatedAt",
    ] {
        assert!(!obj.contains_key(key), "unexpected key: {key}");
    }
}
