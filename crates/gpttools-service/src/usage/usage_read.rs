use gpttools_core::rpc::types::UsageSnapshotResult;
use gpttools_core::storage::UsageSnapshotRecord;

use crate::storage_helpers::open_storage;

pub(crate) fn usage_snapshot_result_from_record(
    snap: UsageSnapshotRecord,
) -> UsageSnapshotResult {
    // 将存储记录转换为 API 返回结构
    UsageSnapshotResult {
        account_id: Some(snap.account_id),
        used_percent: snap.used_percent,
        window_minutes: snap.window_minutes,
        resets_at: snap.resets_at,
        secondary_used_percent: snap.secondary_used_percent,
        secondary_window_minutes: snap.secondary_window_minutes,
        secondary_resets_at: snap.secondary_resets_at,
        credits_json: snap.credits_json,
        captured_at: Some(snap.captured_at),
    }
}

pub(crate) fn read_usage_snapshot(account_id: Option<&str>) -> Option<UsageSnapshotResult> {
    // 读取最新用量快照
    let storage = open_storage()?;
    let snap = match account_id {
        Some(account_id) => storage
            .latest_usage_snapshots_by_account()
            .ok()
            .and_then(|snaps| snaps.into_iter().find(|snap| snap.account_id == account_id)),
        None => storage.latest_usage_snapshot().ok().flatten(),
    }?;
    Some(usage_snapshot_result_from_record(snap))
}
