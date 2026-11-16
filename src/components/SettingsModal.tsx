import type { ArchivePolicy, ArchiveStats, ThemePreference } from "../types";
import type { ProviderConfig } from "./ProviderSelector";
import { type SettingsCategory, SettingsStudio } from "./SettingsStudio";

type TabKey = "general" | "models" | "mcp" | "data";

const TAB_TO_CATEGORY: Record<TabKey, SettingsCategory> = {
  general: "appearance",
  models: "providers",
  mcp: "mcp",
  data: "storage",
};

interface SettingsModalProps {
  open: boolean;
  onClose: () => void;
  providers: ProviderConfig[];
  onProvidersChange: (providers: ProviderConfig[]) => void;
  onRefreshAvailability: (forceRefresh?: boolean) => Promise<void>;
  isRefreshingModels: boolean;
  initialTab?: TabKey;
  themePreference: ThemePreference;
  onThemePreferenceChange: (theme: ThemePreference) => void;
  archivePolicy?: ArchivePolicy | null;
  workspaceArchivePolicy?: ArchivePolicy | null;
  archiveStats?: ArchiveStats | null;
  activeWorkspaceName?: string;
  onArchivePolicyChange?: (
    workspaceId: string | null,
    retentionDays: number,
    autoArchiveHours: number
  ) => Promise<void>;
  onClearWorkspaceArchivePolicy?: () => Promise<void>;
  onRunArchiveMaintenance?: () => Promise<void>;
  archivePolicySaving?: boolean;
}

export function SettingsModal({
  open,
  onClose,
  providers,
  onProvidersChange,
  onRefreshAvailability,
  isRefreshingModels,
  initialTab = "models",
  themePreference,
  onThemePreferenceChange,
  archivePolicy,
  workspaceArchivePolicy,
  archiveStats,
  activeWorkspaceName,
  onArchivePolicyChange,
  onClearWorkspaceArchivePolicy,
  onRunArchiveMaintenance,
  archivePolicySaving = false,
}: SettingsModalProps) {
  if (!open) return null;

  return (
    <SettingsStudio
      onClose={onClose}
      initialCategory={TAB_TO_CATEGORY[initialTab]}
      providers={providers}
      onProvidersChange={onProvidersChange}
      onRefreshAvailability={onRefreshAvailability}
      isRefreshingModels={isRefreshingModels}
      themePreference={themePreference}
      onThemePreferenceChange={onThemePreferenceChange}
      archivePolicy={archivePolicy}
      workspaceArchivePolicy={workspaceArchivePolicy}
      archiveStats={archiveStats}
      activeWorkspaceName={activeWorkspaceName}
      onArchivePolicyChange={onArchivePolicyChange}
      onClearWorkspaceArchivePolicy={onClearWorkspaceArchivePolicy}
      onRunArchiveMaintenance={onRunArchiveMaintenance}
      archivePolicySaving={archivePolicySaving}
    />
  );
}
