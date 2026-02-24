use codexmanager_core::auth::build_authorize_url;

#[test]
fn build_authorize_url_matches_codex() {
    let issuer = "https://auth.openai.com";
    let client_id = "app_123";
    let redirect_uri = "http://localhost:1455/auth/callback";
    let code_challenge = "challenge";
    let state = "state123";
    let originator = "codex_cli";

    let url = build_authorize_url(
        issuer,
        client_id,
        redirect_uri,
        code_challenge,
        state,
        originator,
        Some("org_abc"),
    );

    assert!(url.starts_with("https://auth.openai.com/oauth/authorize?"));
    assert!(url.contains("response_type=code"));
    assert!(url.contains("client_id=app_123"));
    assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback"));
    assert!(url.contains("scope=openid%20profile%20email%20offline_access"));
    assert!(url.contains("code_challenge=challenge"));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("id_token_add_organizations=true"));
    assert!(url.contains("codex_cli_simplified_flow=true"));
    assert!(url.contains("state=state123"));
    assert!(url.contains("originator=codex_cli"));
    assert!(url.contains("allowed_workspace_id=org_abc"));
}

#[test]
fn parse_id_token_claims_extracts_email_and_sub() {
    let token =
        "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyLTEiLCJlbWFpbCI6InRlc3RAZXhhbXBsZS5jb20ifQ.sig";
    let claims = codexmanager_core::auth::parse_id_token_claims(token).expect("claims");
    assert_eq!(claims.sub, "user-1");
    assert_eq!(claims.email.as_deref(), Some("test@example.com"));
}
