export const REASONING_OPTIONS = [
  { value: "", label: "跟随请求等级" },
  { value: "low", label: "Low" },
  { value: "medium", label: "Medium" },
  { value: "high", label: "High" },
  { value: "xhigh", label: "XHigh" },
];

export function mapReasoningEffortToSelectValue(reasoningEffort) {
  return reasoningEffort === "extra_high"
    ? "xhigh"
    : (reasoningEffort || "");
}

export function getProtocolProfileLabel(protocolType) {
  if (protocolType === "azure_openai") {
    return "Azure OpenAI";
  }
  return protocolType === "anthropic_native"
    ? "Claude Code 兼容"
    : "OpenAI Compatible";
}

export function getStatusViewModel(status) {
  const normalizedStatus = String(status || "").toLowerCase();
  if (normalizedStatus === "active") {
    return {
      className: "status-ok",
      label: "启用",
      isDisabled: false,
    };
  }
  if (normalizedStatus === "disabled") {
    return {
      className: "status-bad",
      label: "禁用",
      isDisabled: true,
    };
  }
  return {
    className: "status-unknown",
    label: status || "未知",
    isDisabled: false,
  };
}
