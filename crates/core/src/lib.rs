pub mod auth;
pub mod rpc;
pub mod storage;
pub mod usage;

pub fn core_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
