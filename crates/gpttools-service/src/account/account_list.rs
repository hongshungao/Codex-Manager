use gpttools_core::rpc::types::AccountSummary;

use crate::storage_helpers::open_storage;

pub(crate) fn read_accounts() -> Vec<AccountSummary> {
    // 读取账户列表并转成前端展示结构
    let storage = match open_storage() {
        Some(storage) => storage,
        None => return Vec::new(),
    };
    let accounts = match storage.list_accounts() {
        Ok(items) => items,
        Err(_) => return Vec::new(),
    };
    accounts
        .into_iter()
        .map(|acc| AccountSummary {
            id: acc.id,
            label: acc.label,
            status: acc.status,
            workspace_id: acc.workspace_id,
            workspace_name: acc.workspace_name,
            note: acc.note,
            tags: acc.tags,
            group_name: acc.group_name,
            sort: acc.sort,
            updated_at: acc.updated_at,
        })
        .collect()
}
