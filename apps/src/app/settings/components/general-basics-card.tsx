import { AppWindow, Check, Download, FolderOpen, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import type { UpdateCheckResult, UpdatePrepareResult } from "@/lib/api/app-updates";
import type { AppSettings } from "@/types";

type GeneralBasicsSnapshot = Pick<
  AppSettings,
  | "updateAutoCheck"
  | "removeRequiresOpenaiAuth"
  | "autoStartEnabled"
  | "autoStartSupported"
  | "showMainWindowOnStartup"
  | "closeToTrayOnClose"
  | "closeToTraySupported"
  | "keepWindowUiMounted"
  | "lowTransparency"
>;

export function GeneralBasicsCard({
  t,
  updateActionLabel,
  updateActionDescription,
  lastUpdateCheck,
  preparedUpdate,
  shouldShowUpdateLogsEntry,
  handleOpenUpdateLogsDir,
  canSelfUpdate,
  updateActionBusy,
  handleUpdateAction,
  manualUpdateCheckPending,
  prepareUpdatePending,
  applyPreparedUpdatePending,
  hasPreparedUpdate,
  canDownloadUpdate,
  updateActionBusyLabel,
  snapshot,
  canAutoStart,
  canCloseToTray,
  updateSettings,
}: {
  t: (value: string) => string;
  updateActionLabel: string;
  updateActionDescription: string;
  lastUpdateCheck: UpdateCheckResult | null;
  preparedUpdate: UpdatePrepareResult | null;
  shouldShowUpdateLogsEntry: boolean;
  handleOpenUpdateLogsDir: () => void;
  canSelfUpdate: boolean;
  updateActionBusy: boolean;
  handleUpdateAction: () => void;
  manualUpdateCheckPending: boolean;
  prepareUpdatePending: boolean;
  applyPreparedUpdatePending: boolean;
  hasPreparedUpdate: boolean;
  canDownloadUpdate: boolean;
  updateActionBusyLabel: string;
  snapshot: GeneralBasicsSnapshot;
  canAutoStart: boolean;
  canCloseToTray: boolean;
  updateSettings: { mutate: (patch: Partial<AppSettings>) => void };
}) {
  return (
    <Card className="glass-card mission-panel shadow-sm">
      <CardHeader>
        <div className="flex items-center gap-2">
          <AppWindow className="h-4 w-4 text-primary" />
          <CardTitle className="text-base">{t("基础设置")}</CardTitle>
        </div>
        <CardDescription>{t("控制应用启动和窗口行为")}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-6">
        <Card size="sm">
          <CardContent className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
            <div className="space-y-1">
              <Label>{updateActionLabel}</Label>
              <p className="text-xs text-muted-foreground">{updateActionDescription}</p>
              {lastUpdateCheck ? (
                <p className="text-xs text-muted-foreground">
                  {preparedUpdate
                    ? `${t("已下载")} ${preparedUpdate.latestVersion || preparedUpdate.releaseTag || t("新版本")}${t("，等待替换更新")}`
                    : lastUpdateCheck.hasUpdate
                      ? `${t("发现新版本")} ${lastUpdateCheck.latestVersion || lastUpdateCheck.releaseTag || t("可用")}`
                      : lastUpdateCheck.reason || `${t("当前版本")} ${lastUpdateCheck.currentVersion || t("未知")} ${t("已是最新")}`}
                </p>
              ) : null}
              {shouldShowUpdateLogsEntry ? (
                <div className="pt-1">
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-auto px-0 text-xs text-muted-foreground hover:text-foreground"
                    onClick={handleOpenUpdateLogsDir}
                  >
                    <FolderOpen className="h-3.5 w-3.5" />
                    {t("打开日志目录")}
                  </Button>
                </div>
              ) : null}
            </div>
            <Button
              variant="outline"
              className="gap-2 self-start md:self-auto"
              disabled={!canSelfUpdate || updateActionBusy}
              onClick={handleUpdateAction}
            >
              {manualUpdateCheckPending ? (
                <RefreshCw className="h-4 w-4 animate-spin" />
              ) : prepareUpdatePending ? (
                <Download className="h-4 w-4 animate-pulse" />
              ) : applyPreparedUpdatePending ? (
                <RefreshCw className="h-4 w-4 animate-spin" />
              ) : hasPreparedUpdate ? (
                <Check className="h-4 w-4" />
              ) : canDownloadUpdate ? (
                <Download className="h-4 w-4" />
              ) : (
                <RefreshCw className="h-4 w-4" />
              )}
              {updateActionBusyLabel}
            </Button>
          </CardContent>
        </Card>
        <div className="flex items-center justify-between">
          <div className="space-y-0.5">
            <Label>{t("自动检查更新")}</Label>
            <p className="text-xs text-muted-foreground">
              {t("启动完成后在后台检查更新，并每 7 小时检查一次")}
            </p>
          </div>
          <Switch
            checked={snapshot.updateAutoCheck}
            disabled={!canSelfUpdate}
            onCheckedChange={(value) =>
              updateSettings.mutate({ updateAutoCheck: value })
            }
          />
        </div>
        <div className="flex items-center justify-between gap-4">
          <div className="space-y-0.5">
            <Label>{t("移除 requires_openai_auth")}</Label>
            <p className="text-xs text-muted-foreground">
              {t(
                "开启后，在切换到 CodexManager 网关或同步配置时移除 requires_openai_auth；关闭时保留现有配置，默认关闭",
              )}
            </p>
          </div>
          <Switch
            checked={snapshot.removeRequiresOpenaiAuth}
            onCheckedChange={(value) =>
              updateSettings.mutate({ removeRequiresOpenaiAuth: value })
            }
          />
        </div>
        <div className="flex items-center justify-between">
          <div className="space-y-0.5">
            <Label>{t("开机自动启动")}</Label>
            <p className="text-xs text-muted-foreground">{t("系统登录后自动启动桌面端并保持网关可用")}</p>
          </div>
          <Switch
            checked={snapshot.autoStartEnabled}
            disabled={!canAutoStart || !snapshot.autoStartSupported}
            onCheckedChange={(value) => updateSettings.mutate({ autoStartEnabled: value })}
          />
        </div>
        <div className="flex items-center justify-between">
          <div className="space-y-0.5">
            <Label>{t("启动时显示主界面")}</Label>
            <p className="text-xs text-muted-foreground">
              {t("开启时直接显示主界面；关闭此项时保持隐藏，可从托盘菜单打开")}
            </p>
          </div>
          <Switch
            checked={snapshot.showMainWindowOnStartup}
            disabled={!snapshot.closeToTraySupported}
            onCheckedChange={(value) =>
              updateSettings.mutate({ showMainWindowOnStartup: value })
            }
          />
        </div>
        <div className="flex items-center justify-between">
          <div className="space-y-0.5">
            <Label>{t("关闭时最小化到托盘")}</Label>
            <p className="text-xs text-muted-foreground">{t("点击关闭按钮不会直接退出程序")}</p>
          </div>
          <Switch
            checked={snapshot.closeToTrayOnClose}
            disabled={!canCloseToTray || !snapshot.closeToTraySupported}
            onCheckedChange={(value) => updateSettings.mutate({ closeToTrayOnClose: value })}
          />
        </div>
        <div className="flex items-center justify-between">
          <div className="space-y-0.5">
            <Label>{t("窗口界面资源常驻")}</Label>
            <p className="text-xs text-muted-foreground">
              {!snapshot.closeToTrayOnClose
                ? t("需先开启关闭时最小化到托盘，才能选择窗口关闭后的资源策略")
                : snapshot.keepWindowUiMounted
                  ? t("快速唤醒：关闭后隐藏并保留界面，重开更快，但会占用更多内存")
                  : t("低资源：关闭后销毁界面，后台服务继续运行，重开时重新加载")}
            </p>
          </div>
          <Switch
            checked={snapshot.keepWindowUiMounted}
            disabled={
              !canCloseToTray ||
              !snapshot.closeToTraySupported ||
              !snapshot.closeToTrayOnClose
            }
            onCheckedChange={(value) => updateSettings.mutate({ keepWindowUiMounted: value })}
          />
        </div>
        <div className="flex items-center justify-between">
          <div className="space-y-0.5">
            <Label>{t("视觉性能模式")}</Label>
            <p className="text-xs text-muted-foreground">{t("关闭毛玻璃等特效以提升低配电脑性能")}</p>
          </div>
          <Switch
            checked={snapshot.lowTransparency}
            onCheckedChange={(value) => updateSettings.mutate({ lowTransparency: value })}
          />
        </div>
      </CardContent>
    </Card>
  );
}
