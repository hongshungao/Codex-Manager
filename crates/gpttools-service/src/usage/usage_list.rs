use gpttools_core::rpc::types::UsageSnapshotResult;

use crate::storage_helpers::open_storage;
use crate::usage_read::usage_snapshot_result_from_record;

pub(crate) fn read_usage_snapshots() -> Vec<UsageSnapshotResult> {
    // 读取所有账号最新用量
    let storage = match open_storage() {
        Some(storage) => storage,
        None => return Vec::new(),
    };
    let items = match storage.latest_usage_snapshots_by_account() {
        Ok(items) => items,
        Err(_) => return Vec::new(),
    };
    items
        .into_iter()
        .map(usage_snapshot_result_from_record)
        .collect()
}
