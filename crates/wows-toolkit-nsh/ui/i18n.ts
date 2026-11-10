import { signal } from "nsh";
import { Shell } from "wows-toolkit/native";

// The process's language, not a fixed one: text a module reads before the
// first applyLocale is in it, and a change from it must be seen as a change.
const locale = signal(Shell.locale());

/** The language the interface is read in now (reactive). */
export function currentLocale(): string {
  return locale();
}

/** Reads every catalog string in `code` from now on, and re-renders what read one. */
export function applyLocale(code: string) {
  Shell.setLocale(code);
  locale.set(code);
}

/** Re-renders what read a catalog string in `code`, which another window already applied to the process. */
export function followLocale(code: string) {
  locale.set(code);
}

/**
 * The catalog text for `key` in the current language, `%{name}` placeholders
 * filled from `args`. Reading it inside a render re-renders on a language change.
 */
export function t(key: string, args?: Record<string, string | number>): string {
  locale();
  if (!args) return Shell.translate(key);
  const text: Record<string, string> = {};
  for (const name of Object.keys(args)) text[name] = String(args[name]);
  return Shell.translate(key, text);
}
