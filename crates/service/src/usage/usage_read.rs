use codexmanager_core::rpc::types::UsageSnapshotResult;
use codexmanager_core::storage::UsageSnapshotRecord;

use crate::storage_helpers::open_storage;

pub(crate) fn usage_snapshot_result_from_record(
    snap: UsageSnapshotRecord,
) -> UsageSnapshotResult {
    let availability_status = classify_availability_status(&snap).to_string();
    // 将存储记录转换为 API 返回结构
    UsageSnapshotResult {
        account_id: Some(snap.account_id),
        availability_status: Some(availability_status),
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

fn classify_availability_status(snap: &UsageSnapshotRecord) -> &'static str {
    let primary_missing = snap.used_percent.is_none() || snap.window_minutes.is_none();
    if primary_missing {
        return "unknown";
    }
    if snap.used_percent.map(|value| value >= 100.0).unwrap_or(false) {
        return "unavailable";
    }

    let secondary_present = snap.secondary_used_percent.is_some() || snap.secondary_window_minutes.is_some();
    let secondary_complete = snap.secondary_used_percent.is_some() && snap.secondary_window_minutes.is_some();

    if !secondary_present {
        return "primary_window_available_only";
    }
    if !secondary_complete {
        return "unknown";
    }
    if snap
        .secondary_used_percent
        .map(|value| value >= 100.0)
        .unwrap_or(false)
    {
        return "unavailable";
    }
    "available"
}

pub(crate) fn read_usage_snapshot(account_id: Option<&str>) -> Option<UsageSnapshotResult> {
    // 读取最新用量快照
    let storage = open_storage()?;
    let snap = match account_id {
        Some(account_id) => storage.latest_usage_snapshot_for_account(account_id).ok().flatten(),
        None => storage.latest_usage_snapshot().ok().flatten(),
    }?;
    Some(usage_snapshot_result_from_record(snap))
}
