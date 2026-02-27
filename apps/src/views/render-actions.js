export function buildRenderActions({
  updateAccountSort,
  handleOpenUsageModal,
  setManualPreferredAccount,
  deleteAccount,
  toggleApiKeyStatus,
  deleteApiKey,
  updateApiKeyModel,
  copyApiKey,
}) {
  return {
    onUpdateSort: updateAccountSort,
    onOpenUsage: handleOpenUsageModal,
    onSetCurrentAccount: setManualPreferredAccount,
    onDeleteAccount: deleteAccount,
    onToggleApiKeyStatus: toggleApiKeyStatus,
    onDeleteApiKey: deleteApiKey,
    onUpdateApiKeyModel: updateApiKeyModel,
    onCopyApiKey: copyApiKey,
  };
}
