"use client";

import { useCallback, useMemo, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Clock3, Loader2, RotateCcw, ShieldCheck, TicketCheck } from "lucide-react";
import { toast } from "sonner";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { resetCreditClient } from "@/lib/api/reset-credit-client";
import { getAppErrorMessage } from "@/lib/api/transport";
import { CODEX_PROFILE_CANDIDATES_QUERY_KEY } from "@/lib/api/codex-profile-client";
import { useI18n } from "@/lib/i18n/provider";
import { cn } from "@/lib/utils";
import {
  isResetCreditAvailable,
  readCachedResetCredits,
} from "@/lib/utils/reset-credits";
import type { Account, ResetCredit, ResetCreditsSnapshot } from "@/types";

interface AccountResetCreditControlProps {
  account: Account;
  disabled?: boolean;
}

function formatTimestamp(
  timestamp: number | null,
  fallback: string,
  locale: string,
): string {
  if (!timestamp || !Number.isFinite(timestamp)) return fallback;
  return new Intl.DateTimeFormat(locale, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(timestamp * 1000));
}

function resetCreditStatusLabel(credit: ResetCredit, t: (value: string) => string) {
  const status = (credit.status || credit.rawStatus || "").trim().toLowerCase();
  if (["redeemed", "used", "consumed"].includes(status)) return t("已使用");
  if (status === "expired") return t("已过期");
  if (status === "available") {
    return isResetCreditAvailable(credit) ? t("可用") : t("已过期");
  }
  if (credit.expiresAt != null && credit.expiresAt <= Date.now() / 1000) {
    return t("已过期");
  }
  return credit.rawStatus || credit.status || t("未知");
}

export function AccountResetCreditControl({
  account,
  disabled = false,
}: AccountResetCreditControlProps) {
  const { locale, t } = useI18n();
  const queryClient = useQueryClient();
  const cached = useMemo(
    () => readCachedResetCredits(account.usage),
    [account.usage],
  );
  const [open, setOpen] = useState(false);
  const [snapshot, setSnapshot] = useState<ResetCreditsSnapshot | null>(null);
  const [loading, setLoading] = useState(false);
  const [consuming, setConsuming] = useState(false);
  const [loadFailed, setLoadFailed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestSequence = useRef(0);

  const loadSnapshot = useCallback(async () => {
    const sequence = requestSequence.current + 1;
    requestSequence.current = sequence;
    setLoading(true);
    setLoadFailed(false);
    setError(null);
    try {
      const next = await resetCreditClient.get(account.id);
      if (requestSequence.current !== sequence) return;
      setSnapshot(next);
    } catch (loadError) {
      if (requestSequence.current !== sequence) return;
      setLoadFailed(true);
      setError(getAppErrorMessage(loadError));
    } finally {
      if (requestSequence.current === sequence) setLoading(false);
    }
  }, [account.id]);

  const handleOpenChange = useCallback(
    (nextOpen: boolean) => {
      if (consuming) return;
      setOpen(nextOpen);
      if (nextOpen) {
        setSnapshot(null);
        void loadSnapshot();
      } else {
        requestSequence.current += 1;
        setError(null);
        setLoadFailed(false);
      }
    },
    [consuming, loadSnapshot],
  );

  const availableCount = snapshot?.availableCount ?? cached.availableCount;
  if (!cached.present && !open) return null;

  const consume = async () => {
    if (loading || loadFailed || (snapshot?.availableCount ?? 0) <= 0) return;
    setConsuming(true);
    setError(null);
    try {
      const result = await resetCreditClient.consume(account.id);
      if (!result.consumed) throw new Error(t("重置请求未完成，请稍后重试"));
      setSnapshot(result.snapshot);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["usage"] }),
        queryClient.invalidateQueries({ queryKey: ["usage-aggregate"] }),
        queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
        queryClient.invalidateQueries({ queryKey: CODEX_PROFILE_CANDIDATES_QUERY_KEY }),
      ]);
      if (result.warning) {
        toast.warning(t("额度已重置，但最新用量同步失败，请稍后手动刷新"));
      } else {
        toast.success(t("5 小时和 7 天额度已重置"));
      }
      setOpen(false);
    } catch (consumeError) {
      setError(getAppErrorMessage(consumeError));
    } finally {
      setConsuming(false);
    }
  };

  const nextExpiresAt = snapshot?.nextExpiresAt ?? null;
  const details = snapshot?.credits ?? [];
  const canConsume =
    !disabled &&
    !loading &&
    !loadFailed &&
    !consuming &&
    (snapshot?.availableCount ?? 0) > 0;
  const buttonCount = availableCount ?? 0;
  const buttonCountLabel = availableCount == null ? "--" : String(availableCount);

  return (
    <>
      <Tooltip>
        <TooltipTrigger render={<span />} className="inline-flex">
          <Button
            type="button"
            variant="outline"
            size="sm"
            className={cn(
              "h-8 gap-1.5 rounded-full px-3 text-xs font-semibold transition-all duration-200 active:scale-[0.98]",
              buttonCount > 0
                ? "border-emerald-500/55 bg-gradient-to-r from-emerald-500/20 via-teal-500/15 to-cyan-500/20 text-emerald-800 shadow-[0_4px_14px_-8px_rgba(16,185,129,0.95)] hover:-translate-y-px hover:border-emerald-500/75 hover:from-emerald-500/30 hover:via-teal-500/25 hover:to-cyan-500/30 hover:text-emerald-900 hover:shadow-[0_6px_18px_-8px_rgba(16,185,129,1)] dark:text-emerald-200 dark:hover:text-emerald-100"
                : "border-border/70 bg-background/55 text-muted-foreground shadow-sm hover:border-emerald-500/35 hover:bg-emerald-500/8",
            )}
            disabled={disabled}
            onClick={() => handleOpenChange(true)}
            aria-label={
              availableCount == null
                ? t("重置 5 小时和 7 天额度，次数待核对")
                : t("重置 5 小时和 7 天额度，可用 {count} 次", {
                    count: availableCount,
                  })
            }
          >
            <span
              className={cn(
                "flex h-5 w-5 items-center justify-center rounded-full",
                buttonCount > 0
                  ? "bg-emerald-500/20 text-emerald-700 dark:text-emerald-200"
                  : "bg-muted text-muted-foreground",
              )}
            >
              <RotateCcw className="h-3 w-3" />
            </span>
            {t("重置 5h + 7d")}
            <span
              className={cn(
                "rounded-full px-1.5 py-0.5 text-[11px] font-bold tabular-nums",
                buttonCount > 0
                  ? "bg-emerald-600 text-white dark:bg-emerald-400 dark:text-emerald-950"
                  : "bg-muted text-muted-foreground",
              )}
            >
              {buttonCountLabel}
            </span>
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          {availableCount == null
            ? t("打开后实时核对可用次数和发放记录")
            : buttonCount > 0
              ? t("可消耗一次重置券，同时恢复当前 5 小时和 7 天额度")
              : t("当前没有可用重置券，可查看发放记录")}
        </TooltipContent>
      </Tooltip>

      <Dialog open={open} onOpenChange={handleOpenChange}>
        <DialogContent className="glass-card overflow-hidden p-0 sm:max-w-[560px]">
          <div className="border-b border-emerald-500/15 bg-gradient-to-br from-emerald-500/14 via-cyan-500/8 to-transparent px-6 py-5">
            <DialogHeader className="text-left">
              <div className="mb-2 flex h-11 w-11 items-center justify-center rounded-2xl border border-emerald-500/25 bg-emerald-500/12 text-emerald-600 shadow-sm dark:text-emerald-300">
                <RotateCcw className="h-5 w-5" />
              </div>
              <DialogTitle>{t("重置当前 5 小时和 7 天额度")}</DialogTitle>
              <DialogDescription className="max-w-[48ch]">
                {t(
                  "此操作会消耗 1 次重置券，并同时恢复当前 5 小时和 7 天额度。提交成功后无法撤销。",
                )}
              </DialogDescription>
            </DialogHeader>
          </div>

          <div className="space-y-4 px-6 py-5">
            <div className="grid gap-3 sm:grid-cols-[1fr_auto]">
              <div className="min-w-0 rounded-xl border border-border/60 bg-background/45 px-4 py-3">
                <div className="text-[11px] font-medium uppercase tracking-[0.12em] text-muted-foreground">
                  {t("目标账号")}
                </div>
                <div className="mt-1 truncate text-sm font-semibold" title={account.label || account.name}>
                  {account.label || account.name}
                </div>
              </div>
              <div className="flex min-w-[132px] items-center gap-3 rounded-xl border border-emerald-500/25 bg-emerald-500/8 px-4 py-3">
                {loading ? (
                  <Loader2 className="h-5 w-5 animate-spin text-emerald-600" />
                ) : (
                  <TicketCheck className="h-5 w-5 text-emerald-600 dark:text-emerald-300" />
                )}
                <div>
                  <div className="text-[11px] text-muted-foreground">{t("可用次数")}</div>
                  <div className="text-lg font-semibold tabular-nums">{availableCount ?? "--"}</div>
                </div>
              </div>
            </div>

            {nextExpiresAt ? (
              <div className="flex items-center gap-2 text-xs text-muted-foreground">
                <Clock3 className="h-3.5 w-3.5" />
                {t("最近一张将在 {time} 到期", {
                  time: formatTimestamp(nextExpiresAt, t("时间未知"), locale),
                })}
              </div>
            ) : null}

            <div className="rounded-xl border border-border/60 bg-background/35">
              <div className="flex items-center justify-between border-b border-border/50 px-4 py-2.5">
                <span className="text-xs font-semibold">{t("重置券记录")}</span>
                <ShieldCheck className="h-4 w-4 text-muted-foreground" />
              </div>
              <div className="max-h-44 space-y-2 overflow-y-auto p-3">
                {loading ? (
                  <div className="flex items-center justify-center gap-2 py-6 text-xs text-muted-foreground">
                    <Loader2 className="h-4 w-4 animate-spin" />
                    {t("正在核对可用次数...")}
                  </div>
                ) : loadFailed ? (
                  <div className="py-6 text-center text-xs text-muted-foreground">
                    {t("无法读取重置券详情，请重新核对。")}
                  </div>
                ) : details.length > 0 ? (
                  details.map((credit, index) => {
                    const available = isResetCreditAvailable(credit);
                    return (
                      <div
                        key={credit.id || `${credit.status || "credit"}-${index}`}
                        className="flex items-center justify-between gap-3 rounded-lg border border-border/45 bg-card/50 px-3 py-2"
                      >
                        <div className="min-w-0">
                          <div className="truncate text-xs font-medium">
                            {credit.resetType || t("5 小时和 7 天额度重置券")}
                          </div>
                          <div className="mt-0.5 text-[10px] text-muted-foreground">
                            {t("到期：{time}", {
                              time: formatTimestamp(
                                credit.expiresAt,
                                t("时间未知"),
                                locale,
                              ),
                            })}
                          </div>
                        </div>
                        <span
                          className={cn(
                            "shrink-0 rounded-full border px-2 py-0.5 text-[10px] font-medium",
                            available
                              ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
                              : "border-border/60 bg-muted/50 text-muted-foreground",
                          )}
                        >
                          {resetCreditStatusLabel(credit, t)}
                        </span>
                      </div>
                    );
                  })
                ) : (
                  <div className="py-6 text-center text-xs text-muted-foreground">
                    {t("暂无重置券记录")}
                  </div>
                )}
              </div>
            </div>

            {error ? (
              <Alert variant="destructive">
                <AlertDescription className="break-words">{error}</AlertDescription>
              </Alert>
            ) : null}
          </div>

          <DialogFooter className="mx-0 mb-0 border-t border-border/60 bg-background/35 px-6 py-4 sm:justify-between">
            <Button
              type="button"
              variant="ghost"
              disabled={consuming}
              onClick={() => handleOpenChange(false)}
            >
              {t("取消")}
            </Button>
            <div className="flex gap-2">
              {loadFailed ? (
                <Button type="button" variant="outline" onClick={() => void loadSnapshot()}>
                  <RotateCcw className="h-4 w-4" />
                  {t("重新核对")}
                </Button>
              ) : null}
              <Button type="button" disabled={!canConsume} onClick={() => void consume()}>
                {consuming ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <RotateCcw className="h-4 w-4" />
                )}
                {consuming ? t("正在重置...") : t("消耗 1 次并重置")}
              </Button>
            </div>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
