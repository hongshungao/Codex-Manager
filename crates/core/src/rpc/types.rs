use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub id: u64,
    pub result: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InitializeResult {
    pub server_name: String,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub id: String,
    pub label: String,
    pub group_name: Option<String>,
    pub sort: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountListResult {
    pub items: Vec<AccountSummary>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAuthInfo {
    pub user_code_url: String,
    pub token_url: String,
    pub verification_url: String,
    pub redirect_uri: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStartResult {
    pub auth_url: String,
    pub login_id: String,
    pub login_type: String,
    pub issuer: String,
    pub client_id: String,
    pub redirect_uri: String,
    #[serde(default)]
    pub warning: Option<String>,
    pub device: Option<DeviceAuthInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshotResult {
    pub account_id: Option<String>,
    pub used_percent: Option<f64>,
    pub window_minutes: Option<i64>,
    pub resets_at: Option<i64>,
    pub secondary_used_percent: Option<f64>,
    pub secondary_window_minutes: Option<i64>,
    pub secondary_resets_at: Option<i64>,
    pub credits_json: Option<String>,
    pub captured_at: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UsageReadResult {
    pub snapshot: Option<UsageSnapshotResult>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UsageListResult {
    pub items: Vec<UsageSnapshotResult>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeySummary {
    pub id: String,
    pub name: Option<String>,
    pub model_slug: Option<String>,
    pub reasoning_effort: Option<String>,
    pub client_type: String,
    pub protocol_type: String,
    pub auth_scheme: String,
    pub upstream_base_url: Option<String>,
    pub static_headers_json: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiKeyListResult {
    pub items: Vec<ApiKeySummary>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyCreateResult {
    pub id: String,
    pub key: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub slug: String,
    pub display_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiKeyModelListResult {
    pub items: Vec<ModelOption>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestLogSummary {
    pub key_id: Option<String>,
    pub request_path: String,
    pub method: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub upstream_url: Option<String>,
    pub status_code: Option<i64>,
    pub error: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RequestLogListResult {
    pub items: Vec<RequestLogSummary>,
}

#[cfg(test)]
mod tests {
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

        for key in ["workspaceId", "workspaceName", "note", "tags", "status", "updatedAt"] {
            assert!(!obj.contains_key(key), "unexpected key: {key}");
        }
    }
}

