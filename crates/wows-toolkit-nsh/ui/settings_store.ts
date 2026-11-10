import { signal } from "nsh";
import { GameData, Settings, type AppSettings, type ReplayPreferences, type SettingKey } from "wows-toolkit/native";
import * as toast from "./shell/toast";

/** The settings the shell read at startup, kept in step with every edit. `null` until the read lands. */
export const appSettings = signal<AppSettings | null>(null);
/** Why the startup read failed, if it did. */
export const settingsLoadError = signal<string | null>(null);
/** Whether the startup read is still running. */
export const settingsLoading = signal(true);

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Reads the shared settings. Resolves to them, or `null` after reporting why not. */
export async function loadAppSettings(): Promise<AppSettings | null> {
  settingsLoading.set(true);
  settingsLoadError.set(null);
  try {
    const loaded = await Settings.load();
    appSettings.set(loaded);
    return loaded;
  } catch (error) {
    settingsLoadError.set(errorText(error));
    return null;
  } finally {
    settingsLoading.set(false);
  }
}

/** The settings the loaded game data was read under. */
const GAME_DATA_KEYS: readonly SettingKey[] = ["wowsDir", "locale", "autoDumpGameData", "gameDataCacheDir", "proxyUrl"];

/** The value type of each setting `editSetting` writes. */
type Editable = { [K in SettingKey]: K extends keyof AppSettings ? NonNullable<AppSettings[K]> : never };

/**
 * Applies one edit and writes it to the shared database at once, as the
 * other front ends do; there is no save step. A write that fails is reported
 * and the shown value goes back to what is stored.
 */
export async function editSetting<K extends SettingKey>(key: K, value: Editable[K]): Promise<boolean> {
  const current = appSettings();
  if (!current) return false;
  const before = (current as Record<string, unknown>)[key];
  appSettings.set({ ...current, [key]: value });
  try {
    await Settings.save(key, value);
    // The game data follows these, so it is bound to them again.
    if (GAME_DATA_KEYS.includes(key)) void GameData.reload();
    return true;
  } catch (error) {
    const now = appSettings();
    if (now) appSettings.set({ ...now, [key]: before });
    toast.failed(errorText(error));
    return false;
  }
}

/** Applies an edit to the Settings tab's part of the replay row and writes it. */
export async function editReplayPreferences(patch: Partial<ReplayPreferences>): Promise<boolean> {
  const current = appSettings();
  if (!current) return false;
  const before = current.replay;
  const replay = { ...before, ...patch };
  appSettings.set({ ...current, replay });
  try {
    await Settings.saveReplay(replay);
    return true;
  } catch (error) {
    const now = appSettings();
    if (now) appSettings.set({ ...now, replay: before });
    toast.failed(errorText(error));
    return false;
  }
}
