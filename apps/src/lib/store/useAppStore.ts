import { create } from "zustand";
import { AppSettings, RuntimeCapabilities, ServiceStatus } from "../../types";
import {
  DEFAULT_CODEX_ORIGINATOR,
  DEFAULT_CODEX_USER_AGENT,
  DEFAULT_CODEX_USER_AGENT_VERSION,
} from "../constants/codex";
import {
  DEFAULT_AUTHOR_SERVER_RECOMMENDATIONS,
  DEFAULT_AUTHOR_SPONSORS,
} from "../sponsor-links";
import {
  type TopLevelRoutePath,
  toTopLevelRoutePath,
} from "../app-shell/top-level-routes";
import { buildStaticRouteUrl } from "../utils/static-routes";
import { DEFAULT_GATEWAY_TRANSPORT_VALUES } from "../gateway/transport-settings";

interface AppState {
  serviceStatus: ServiceStatus;
  appSettings: AppSettings;
  runtimeCapabilities: RuntimeCapabilities | null;
  isSidebarOpen: boolean;
  isCodexCliGuideOpen: boolean;
  currentShellPath: TopLevelRoutePath;
  openShellTabs: TopLevelRoutePath[];
  
  setServiceStatus: (status: Partial<ServiceStatus>) => void;
  setAppSettings: (settings: Partial<AppSettings>) => void;
  setRuntimeCapabilities: (capabilities: RuntimeCapabilities | null) => void;
  toggleSidebar: () => void;
  setSidebarOpen: (open: boolean) => void;
  openCodexCliGuide: () => void;
  closeCodexCliGuide: () => void;
  syncShellPathFromLocation: (path: string) => void;
  navigateShellPath: (path: string, options?: { replace?: boolean }) => void;
  pruneShellTabs: (allowedPaths: string[], fallbackPath: string) => void;
  closeShellTab: (path: string) => TopLevelRoutePath | null;
}

const initialShellPath: TopLevelRoutePath = "/";

function hasPartialStateChanges<T extends object>(
  current: T,
  patch: Partial<T>,
): boolean {
  for (const key of Object.keys(patch) as Array<keyof T>) {
    if (!Object.is(current[key], patch[key])) {
      return true;
    }
  }
  return false;
}

function areTabsEqual(left: TopLevelRoutePath[], right: TopLevelRoutePath[]): boolean {
  return left.length === right.length && left.every((item, index) => item === right[index]);
}

export const useAppStore = create<AppState>((set) => ({
  serviceStatus: {
    connected: false,
    version: "",
    uptime: 0,
    addr: "localhost:48760",
  },
  appSettings: {
    updateAutoCheck: true,
    removeRequiresOpenaiAuth: false,
    autoStartEnabled: false,
    autoStartSupported: false,
    showMainWindowOnStartup: true,
    closeToTrayOnClose: false,
    closeToTraySupported: false,
    keepWindowUiMounted: true,
    lowTransparency: false,
    zoomFactor: 1,
    lightweightModeOnCloseToTray: false,
    codexCliGuideDismissed: false,
    webAccessPasswordConfigured: false,
    webAuthMode: "none",
    webAuthModeOptions: ["none", "password", "accounts"],
    distributionEnabled: false,
    billingModeLock: {
      accountModeLocked: false,
      distributionLocked: false,
      reasons: [],
    },
    appUsersConfigured: false,
    appUserCount: 0,
    locale: "zh-CN",
    localeOptions: ["zh-CN", "en", "ru", "ko"],
    serviceAddr: "localhost:48760",
    serviceListenMode: "loopback",
    serviceListenModeOptions: ["loopback", "all_interfaces"],
    routeStrategy: "ordered",
    routeStrategyOptions: ["ordered", "balanced"],
    freeAccountMaxModel: "auto",
    freeAccountMaxModelOptions: [
      "auto",
      "gpt-5",
      "gpt-5-codex",
      "gpt-5-codex-mini",
      "gpt-5.1",
      "gpt-5.1-codex",
      "gpt-5.1-codex-max",
      "gpt-5.1-codex-mini",
      "gpt-5.2",
      "gpt-5.2-codex",
      "gpt-5.3-codex",
      "gpt-5.4-mini",
      "gpt-5.4",
    ],
    modelForwardRules: "",
    compactModelForwardRules: "",
    accountMaxInflight: 1,
    threadAwareAccountDistributionEnabled: true,
    quotaGuard: {
      enabled: true,
      primaryMinRemainingPercent: 5,
      secondaryMinRemainingPercent: 10,
      allowAllLowQuotaFallback: true,
    },
    gatewayOriginator: DEFAULT_CODEX_ORIGINATOR,
    gatewayOriginatorDefault: DEFAULT_CODEX_ORIGINATOR,
    gatewayUserAgent: "",
    gatewayUserAgentDefault: DEFAULT_CODEX_USER_AGENT,
    gatewayUserAgentVersion: DEFAULT_CODEX_USER_AGENT_VERSION,
    gatewayUserAgentVersionDefault: DEFAULT_CODEX_USER_AGENT_VERSION,
    gatewayResidencyRequirement: "",
    gatewayResidencyRequirementOptions: ["", "us"],
    pluginMarketMode: "builtin",
    pluginMarketSourceUrl: "",
    authorSponsors: DEFAULT_AUTHOR_SPONSORS,
    authorServerRecommendations: DEFAULT_AUTHOR_SERVER_RECOMMENDATIONS,
    upstreamProxyUrl: "",
    upstreamProxyBypassHosts: "",
    ...DEFAULT_GATEWAY_TRANSPORT_VALUES,
    backgroundTasks: {
      usagePollingEnabled: true,
      usagePollIntervalSecs: 600,
      gatewayKeepaliveEnabled: true,
      gatewayKeepaliveIntervalSecs: 180,
      tokenRefreshPollingEnabled: true,
      tokenRefreshPollIntervalSecs: 60,
      usageRefreshWorkers: 4,
      httpWorkerFactor: 4,
      httpWorkerMin: 8,
      httpStreamWorkerFactor: 1,
      httpStreamWorkerMin: 2,
      warmupCronEnabled: false,
      warmupCronExpression: "",
    },
    runtimeTimeZone: {
      name: "Local",
      offset: "",
      source: "system",
    },
    envOverrides: {},
    envOverrideCatalog: [],
    envOverrideReservedKeys: [],
    envOverrideUnsupportedKeys: [],
    theme: "tech",
    appearancePreset: "classic",
  },
  runtimeCapabilities: null,
  isSidebarOpen: true,
  isCodexCliGuideOpen: false,
  currentShellPath: initialShellPath,
  openShellTabs: [initialShellPath],

  setServiceStatus: (status) =>
    set((state) =>
      hasPartialStateChanges(state.serviceStatus, status)
        ? { serviceStatus: { ...state.serviceStatus, ...status } }
        : state,
    ),
  
  setAppSettings: (settings) =>
    set((state) =>
      hasPartialStateChanges(state.appSettings, settings)
        ? { appSettings: { ...state.appSettings, ...settings } }
        : state,
    ),

  setRuntimeCapabilities: (runtimeCapabilities) =>
    set((state) =>
      Object.is(state.runtimeCapabilities, runtimeCapabilities)
        ? state
        : { runtimeCapabilities },
    ),
    
  toggleSidebar: () => set((state) => ({ isSidebarOpen: !state.isSidebarOpen })),
  
  setSidebarOpen: (open) =>
    set((state) => (state.isSidebarOpen === open ? state : { isSidebarOpen: open })),

  openCodexCliGuide: () =>
    set((state) =>
      state.isCodexCliGuideOpen ? state : { isCodexCliGuideOpen: true },
    ),

  closeCodexCliGuide: () =>
    set((state) =>
      state.isCodexCliGuideOpen ? { isCodexCliGuideOpen: false } : state,
    ),

  syncShellPathFromLocation: (path) =>
    set((state) => {
      const nextPath = toTopLevelRoutePath(path);
      if (state.currentShellPath === nextPath && state.openShellTabs.includes(nextPath)) {
        return state;
      }
      return {
        currentShellPath: nextPath,
        openShellTabs: state.openShellTabs.includes(nextPath)
          ? state.openShellTabs
          : [...state.openShellTabs, nextPath],
      };
    }),

  navigateShellPath: (path, options) =>
    set((state) => {
      const nextPath = toTopLevelRoutePath(path);
      const nextTabs = state.openShellTabs.includes(nextPath)
        ? state.openShellTabs
        : [...state.openShellTabs, nextPath];

      if (typeof window !== "undefined") {
        const nextUrl = buildStaticRouteUrl(nextPath);
        if (options?.replace) {
          window.history.replaceState(window.history.state, "", nextUrl);
        } else if (window.location.pathname !== nextUrl) {
          window.history.pushState(window.history.state, "", nextUrl);
        }
      }

      return {
        currentShellPath: nextPath,
        openShellTabs: nextTabs,
      };
    }),

  pruneShellTabs: (allowedPaths, fallbackPath) =>
    set((state) => {
      const allowedSet = new Set(
        allowedPaths.map((path) => toTopLevelRoutePath(path)),
      );
      const fallback = allowedSet.has(toTopLevelRoutePath(fallbackPath))
        ? toTopLevelRoutePath(fallbackPath)
        : "/";
      const nextTabs = state.openShellTabs.filter((path) =>
        allowedSet.has(path),
      );
      const normalizedTabs = nextTabs.length > 0 ? nextTabs : [fallback];
      const nextCurrent = allowedSet.has(state.currentShellPath)
        ? state.currentShellPath
        : normalizedTabs[0] ?? fallback;

      if (
        typeof window !== "undefined" &&
        window.location.pathname !== buildStaticRouteUrl(nextCurrent)
      ) {
        window.history.replaceState(
          window.history.state,
          "",
          buildStaticRouteUrl(nextCurrent),
        );
      }

      if (
        state.currentShellPath === nextCurrent &&
        areTabsEqual(state.openShellTabs, normalizedTabs)
      ) {
        return state;
      }

      return {
        currentShellPath: nextCurrent,
        openShellTabs: normalizedTabs,
      };
    }),

  closeShellTab: (path) => {
    let nextActivePath: TopLevelRoutePath | null = null;

    set((state) => {
      const normalizedPath = toTopLevelRoutePath(path);
      if (normalizedPath === "/") {
        return state;
      }

      const targetIndex = state.openShellTabs.indexOf(normalizedPath);
      if (targetIndex === -1) {
        return state;
      }

      const nextTabs = state.openShellTabs.filter((tab) => tab !== normalizedPath);
      nextActivePath =
        state.currentShellPath === normalizedPath
          ? nextTabs[targetIndex - 1] ?? nextTabs[targetIndex] ?? "/"
          : state.currentShellPath;

      if (
        typeof window !== "undefined" &&
        nextActivePath &&
        state.currentShellPath === normalizedPath
      ) {
        window.history.pushState(
          window.history.state,
          "",
          buildStaticRouteUrl(nextActivePath),
        );
      }

      return {
        currentShellPath: nextActivePath ?? state.currentShellPath,
        openShellTabs: nextTabs,
      };
    });

    return nextActivePath;
  },
}));
