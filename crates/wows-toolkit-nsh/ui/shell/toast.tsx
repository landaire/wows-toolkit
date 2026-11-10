import { signal, Text, View } from "nsh";
import { colors, controlRadius, semantic } from "../theme";
import { t } from "../i18n";

/** How a toast reads: what happened, and whether it went well. */
export type ToastLevel = "ok" | "failed" | "info" | "warn";

type Toast = {
  id: number;
  level: ToastLevel;
  message: string;
  /** The state a stuck or progress toast reports, so a repeat replaces it. */
  key?: string;
  /** Whether it stays until dismissed or resolved. */
  sticky: boolean;
};

/** How long an ordinary toast stays before it goes by itself. */
const AUTOHIDE_MS = 5000;

const toasts = signal<Toast[]>([]);
let nextId = 1;

function push(level: ToastLevel, message: string, key?: string, sticky = false) {
  const id = nextId++;
  const kept = key ? toasts().filter((toast) => toast.key !== key) : toasts();
  toasts.set([...kept, { id, level, message, key, sticky }]);
  if (!sticky) setTimeout(() => dismiss(id), AUTOHIDE_MS);
}

function dismiss(id: number) {
  toasts.set(toasts().filter((toast) => toast.id !== id));
}

/** Something the reader asked for happened. */
export function ok(message: string) {
  push("ok", message);
}

/** Something the reader asked for did not happen. */
export function failed(message: string) {
  push("failed", message);
}

/** Something worth reading that nobody asked for. */
export function info(message: string) {
  push("info", message);
}

/** Something is wrong but nothing was lost. */
export function warn(message: string) {
  push("warn", message);
}

/**
 * A state the reader has to act on: it stays until dismissed or resolved,
 * and a repeat of the same `key` replaces it rather than stacking.
 */
export function stuck(key: string, message: string) {
  push("warn", message, key, true);
}

/** A message that stays while work runs, replaced as the work reports. */
export function progress(key: string, message: string) {
  push("info", message, key, true);
}

/** Takes a stuck or progress message down. */
export function resolved(key: string) {
  toasts.set(toasts().filter((toast) => toast.key !== key));
}

/** The messages on screen now, newest last. */
export function shownToasts(): readonly { level: ToastLevel; message: string }[] {
  return toasts();
}

function tone(level: ToastLevel): string {
  switch (level) {
    case "ok": return semantic.ok;
    case "failed": return semantic.error;
    case "warn": return semantic.warn;
    default: return semantic.notice;
  }
}

/** The stack of toasts in the window's lower right corner. */
export function ToastLayer() {
  return (
    <View style={{ position: "absolute", right: 16, bottom: 34, width: 380, gap: 8 }}>
      <Each of={toasts()}>
        {(toast) => (
          <View
            role="alert"
            accessibilityLabel={toast().message}
            style={{
              flexDirection: "row",
              alignItems: "flex-start",
              gap: 10,
              padding: 12,
              borderRadius: controlRadius(),
              borderWidth: 1,
              borderColor: colors.border,
              borderLeftWidth: 3,
              borderLeftColor: tone(toast().level),
              backgroundColor: colors.popover,
            }}
          >
            <Text style={{ flex: 1, fontSize: 13, lineHeight: 18, color: colors.popoverForeground }}>{toast().message}</Text>
            <View
              role="button"
              accessibilityLabel={t("ui.buttons.dismiss")}
              onPress={() => dismiss(toast().id)}
              hoverStyle={{ backgroundColor: colors.accent }}
              style={{ paddingHorizontal: 6, borderRadius: controlRadius() }}
            >
              <Text style={{ fontSize: 12, color: colors.mutedForeground }}>x</Text>
            </View>
          </View>
        )}
      </Each>
    </View>
  );
}
