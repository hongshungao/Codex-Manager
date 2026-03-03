use super::*;

#[test]
fn login_complete_requires_params() {
    let req = JsonRpcRequest {
        id: 1,
        method: "account/login/complete".to_string(),
        params: None,
    };
    let resp = handle_request(req);
    let err = resp
        .result
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(err.contains("missing"));

    let req = JsonRpcRequest {
        id: 2,
        method: "account/login/complete".to_string(),
        params: Some(serde_json::json!({ "code": "x" })),
    };
    let resp = handle_request(req);
    let err = resp
        .result
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(err.contains("missing"));

    let req = JsonRpcRequest {
        id: 3,
        method: "account/login/complete".to_string(),
        params: Some(serde_json::json!({ "state": "y" })),
    };
    let resp = handle_request(req);
    let err = resp
        .result
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(err.contains("missing"));
}
