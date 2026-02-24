#[test]
fn default_addr_is_localhost() {
    assert_eq!(codexmanager_service::DEFAULT_ADDR, "localhost:48760");
}
