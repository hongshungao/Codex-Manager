#[test]
fn shutdown_flag_can_toggle() {
    assert_eq!(codexmanager_service::shutdown_requested(), false);
    codexmanager_service::request_shutdown("localhost:0");
    assert_eq!(codexmanager_service::shutdown_requested(), true);
    codexmanager_service::clear_shutdown_flag();
    assert_eq!(codexmanager_service::shutdown_requested(), false);
}
