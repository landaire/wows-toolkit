import { signal, Text, View } from "nsh";
import { Spinner } from "../components";
import { t } from "../i18n";
import { Icon } from "../icons";
import { colors, semantic } from "../theme";

/** One running job on the status strip: what it is, read in the current language, and how far it has got. */
export type StatusJob = { label: () => string; done?: number; total?: number };

const jobs = signal<Record<string, StatusJob>>({});

/** Whether the session shows debug detail. Seeded from the setting, flipped by Ctrl+Shift+D, never written back. */
export const debugMode = signal(false);

/** Names a running job on the status strip, replacing the one of the same `key`. */
export function setStatusJob(key: string, job: StatusJob) {
  jobs.set({ ...jobs(), [key]: job });
}

/** Takes a finished job off the status strip. */
export function clearStatusJob(key: string) {
  const { [key]: _finished, ...rest } = jobs();
  jobs.set(rest);
}

/** What is running, along the bottom of the window; absent while nothing is. */
export function StatusBar() {
  const shown = () => Object.entries(jobs());
  return (
    <If when={shown().length > 0}>
      <View
        accessibilityLabel={t("ui.app.status_running_jobs")}
        style={{ minHeight: 24, flexDirection: "row", alignItems: "center", gap: 14, paddingHorizontal: 8, borderTopWidth: 1, borderTopColor: colors.border, backgroundColor: colors.surface }}
      >
        <Each of={shown()}>
          {(entry) => {
            const job = () => entry()[1];
            const counted = () => {
              const { done, total } = job();
              return done !== undefined && total !== undefined && total > 0 ? ` ${t("ui.app.status_of", { done, total })}` : "";
            };
            return (
              <View testID={`status-job-${entry()[0]}`} style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
                <Spinner size={12} label={job().label()}/>
                <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>{`${job().label()}${counted()}`}</Text>
              </View>
            );
          }}
        </Each>
      </View>
    </If>
  );
}

/** The warning strip shown while debug mode is on, as the other front ends draw it. */
export function DebugNotice() {
  return (
    <If when={debugMode()}>
      <View accessibilityLabel={t("ui.app.debug_build")} style={{ minHeight: 24, flexDirection: "row", alignItems: "center", justifyContent: "center", gap: 6 }}>
        <Icon name="triangle-alert" size={14} color={semantic.warn}/>
        <Text style={{ fontSize: 12.5, fontWeight: "700", color: semantic.warn }}>{t("ui.app.debug_build")}</Text>
        <Icon name="triangle-alert" size={14} color={semantic.warn}/>
      </View>
    </If>
  );
}
