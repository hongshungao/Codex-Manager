use codexmanager_core::storage::{now_ts, Account};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::storage_helpers::open_storage;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountExportResult {
    output_dir: String,
    total_accounts: usize,
    exported: usize,
    skipped_missing_token: usize,
    files: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ExportAccountPayload {
    tokens: ExportTokensPayload,
    meta: ExportMetaPayload,
}

#[derive(Debug, Serialize)]
struct ExportTokensPayload {
    access_token: String,
    id_token: String,
    refresh_token: String,
    account_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportMetaPayload {
    label: String,
    issuer: String,
    group_name: Option<String>,
    status: String,
    workspace_id: Option<String>,
    chatgpt_account_id: Option<String>,
    exported_at: i64,
}

pub(crate) fn export_accounts_to_directory(output_dir: &str) -> Result<AccountExportResult, String> {
    let normalized_output_dir = output_dir.trim();
    if normalized_output_dir.is_empty() {
        return Err("missing outputDir".to_string());
    }

    let output_path = PathBuf::from(normalized_output_dir);
    std::fs::create_dir_all(&output_path).map_err(|err| {
        format!(
            "create output directory failed ({}): {err}",
            output_path.display()
        )
    })?;

    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let accounts = storage.list_accounts().map_err(|err| err.to_string())?;
    let total_accounts = accounts.len();
    let mut exported = 0usize;
    let mut skipped_missing_token = 0usize;
    let mut files = Vec::new();
    let mut file_name_counter: HashMap<String, usize> = HashMap::new();

    for account in accounts {
        let token = storage
            .find_token_by_account_id(&account.id)
            .map_err(|err| err.to_string())?;
        let Some(token) = token else {
            skipped_missing_token += 1;
            continue;
        };

        let payload = ExportAccountPayload {
            tokens: ExportTokensPayload {
                access_token: token.access_token,
                id_token: token.id_token,
                refresh_token: token.refresh_token,
                account_id: account.id.clone(),
            },
            meta: ExportMetaPayload {
                label: account.label.clone(),
                issuer: account.issuer.clone(),
                group_name: account.group_name.clone(),
                status: account.status.clone(),
                workspace_id: account.workspace_id.clone(),
                chatgpt_account_id: account.chatgpt_account_id.clone(),
                exported_at: now_ts(),
            },
        };

        let file_path = build_account_export_file_path(&output_path, &account, &mut file_name_counter);
        let json = serde_json::to_vec_pretty(&payload)
            .map_err(|err| format!("encode export json failed: {err}"))?;
        std::fs::write(&file_path, json)
            .map_err(|err| format!("write export file failed ({}): {err}", file_path.display()))?;

        exported += 1;
        files.push(file_path.display().to_string());
    }

    Ok(AccountExportResult {
        output_dir: output_path.display().to_string(),
        total_accounts,
        exported,
        skipped_missing_token,
        files,
    })
}

fn build_account_export_file_path(
    output_dir: &Path,
    account: &Account,
    file_name_counter: &mut HashMap<String, usize>,
) -> PathBuf {
    let label_part = sanitize_file_stem(&account.label);
    let id_part = sanitize_file_stem(&account.id);
    let mut stem = if label_part.is_empty() {
        id_part.clone()
    } else if id_part.is_empty() {
        label_part.clone()
    } else {
        format!("{label_part}_{id_part}")
    };
    if stem.is_empty() {
        stem = "account".to_string();
    }

    let sequence = file_name_counter.entry(stem.clone()).or_insert(0);
    let file_stem = if *sequence == 0 {
        stem
    } else {
        format!("{stem}_{}", *sequence)
    };
    *sequence += 1;

    output_dir.join(format!("{file_stem}.json"))
}

fn sanitize_file_stem(value: &str) -> String {
    let mut out = String::with_capacity(value.len().min(96));
    for ch in value.trim().chars() {
        if out.len() >= 96 {
            break;
        }
        let invalid = matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
            || ch.is_control();
        if invalid {
            out.push('_');
            continue;
        }
        out.push(ch);
    }

    out.trim_matches(|ch: char| ch == ' ' || ch == '.')
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::sanitize_file_stem;

    #[test]
    fn sanitize_file_stem_replaces_windows_invalid_chars() {
        let actual = sanitize_file_stem(r#"a<b>c:d"e/f\g|h?i*j"#);
        assert_eq!(actual, "a_b_c_d_e_f_g_h_i_j");
    }

    #[test]
    fn sanitize_file_stem_trims_tailing_space_and_dot() {
        let actual = sanitize_file_stem(" demo. ");
        assert_eq!(actual, "demo");
    }
}
