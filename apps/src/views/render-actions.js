export function buildRenderActions({
  updateAccountSort,
  handleOpenUsageModal,
  deleteAccount,
  toggleApiKeyStatus,
  deleteApiKey,
  updateApiKeyModel,
  copyApiKey,
}) {
  return {
    onUpdateSort: updateAccountSort,
    onOpenUsage: handleOpenUsageModal,
    onDeleteAccount: deleteAccount,
    onToggleApiKeyStatus: toggleApiKeyStatus,
    onDeleteApiKey: deleteApiKey,
    onUpdateApiKeyModel: updateApiKeyModel,
    onCopyApiKey: copyApiKey,
  };
}
