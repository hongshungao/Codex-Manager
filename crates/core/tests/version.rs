#[test]
fn core_version_is_set() {
    assert!(!codexmanager_core::core_version().is_empty());
}
