use codexmanager_core::rpc::types::AccountSummary;

use crate::storage_helpers::open_storage;

pub(crate) fn read_accounts() -> Result<Vec<AccountSummary>, String> {
    // 读取账户列表并转成前端展示结构
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let accounts = storage
        .list_accounts()
        .map_err(|err| format!("list accounts failed: {err}"))?;
    Ok(accounts
        .into_iter()
        .map(|acc| AccountSummary {
            id: acc.id,
            label: acc.label,
            group_name: acc.group_name,
            sort: acc.sort,
        })
        .collect())
}

