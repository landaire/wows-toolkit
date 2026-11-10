import { signal, Text, View } from "nsh";
import { Dialog } from "nsh/base";
import { Shell, type AppSettings } from "wows-toolkit/native";
import { Button, centeredDialog, Surface } from "../components";
import { applyLocale, t } from "../i18n";
import { editSetting } from "../settings_store";
import { colors } from "../theme";

/** What a fresh run still has to ask, in the order every front end asks it. */
export type Question =
  | { kind: "buildConsent" }
  | { kind: "replayMigration" }
  | { kind: "language"; detected: string };

/**
 * The question a run has to ask, if any: what may be shared comes before the
 * offer to share more, and the language question stands on its own. An
 * English machine needs no warning about machine translation, so it is
 * recorded as asked instead (`"recordLanguage"`).
 */
export function pendingQuestion(settings: AppSettings, detected: string): Question | "recordLanguage" | null {
  if (!settings.buildConsentShown) return { kind: "buildConsent" };
  if (!settings.replayConsentShown) return { kind: "replayMigration" };
  if (!settings.languageSelectionShown) return detected === "en" ? "recordLanguage" : { kind: "language", detected };
  return null;
}

const question = signal<Question | null>(null);

/** Puts the pending question, once, before anything is shared under a default. */
export function askFirstRun(settings: AppSettings, detected: string) {
  const pending = pendingQuestion(settings, detected);
  if (pending === "recordLanguage") {
    void editSetting("languageSelectionShown", true);
    return;
  }
  question.set(pending);
}

/** Changes the language the interface is read in, and remembers it. */
export async function chooseLanguage(code: string) {
  applyLocale(code);
  await editSetting("locale", code);
}

function adoptSharing(mode: string) {
  void editSetting("dataSharing", mode);
}

function BuildConsent() {
  // Answering the first question answers the second: the reader has been told
  // what sharing replays means and chosen.
  const answer = (mode: string) => {
    adoptSharing(mode);
    void editSetting("buildConsentShown", true);
    void editSetting("replayConsentShown", true);
    question.set(null);
  };
  return (
    <View style={{ gap: 14 }}>
      <Text style={{ fontSize: 17, fontWeight: "600", color: colors.popoverForeground }}>{t("ui.windows.build_consent")}</Text>
      <Text style={{ fontSize: 13, lineHeight: 19, color: colors.popoverForeground }}>{t("ui.dialogs.build_consent_message")}</Text>
      <View style={{ flexDirection: "row", flexWrap: "wrap", justifyContent: "flex-end", gap: 8 }}>
        <Button label={t("ui.buttons.send_replays")} onPress={() => answer("replays")}/>
        <Button label={t("ui.buttons.send_build_data")} onPress={() => answer("build_data")}/>
        <Button label={t("ui.buttons.send_nothing")} onPress={() => answer("off")}/>
      </View>
    </View>
  );
}

function ReplayMigration() {
  const done = () => {
    void editSetting("replayConsentShown", true);
    question.set(null);
  };
  return (
    <View style={{ gap: 14 }}>
      <Text style={{ fontSize: 17, fontWeight: "600", color: colors.popoverForeground }}>{t("ui.windows.replay_migration")}</Text>
      <Text style={{ fontSize: 13, lineHeight: 19, color: colors.popoverForeground }}>{t("ui.dialogs.replay_migration_message")}</Text>
      <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
        <Button label={t("ui.buttons.switch_to_replays")} variant="primary" onPress={() => { adoptSharing("replays"); done(); }}/>
        {/* The setting stands; only the asking is recorded. */}
        <Button label={t("ui.buttons.keep_current")} onPress={done}/>
      </View>
    </View>
  );
}

function LanguageQuestion(props: { detected: string }) {
  const native = () => Shell.languages().find((language) => language.code === props.detected)?.nativeName ?? "English";
  // The catalog's own label reads in the detected language once it is loaded;
  // in English it names the language instead.
  const keepLabel = () => {
    const offered = t("dialog.continue_in_language");
    return offered === "Continue in English" ? `Continue in ${native()}` : offered;
  };
  const answer = (code: string) => {
    void chooseLanguage(code);
    void editSetting("languageSelectionShown", true);
    question.set(null);
  };
  return (
    <View style={{ gap: 14 }}>
      <Text style={{ fontSize: 17, fontWeight: "600", color: colors.popoverForeground }}>{t("dialog.select_language")}</Text>
      <Text style={{ fontSize: 13, lineHeight: 19, color: colors.popoverForeground }}>{t("dialog.machine_translation_warning")}</Text>
      <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
        <Button label={t("dialog.continue_in_english")} onPress={() => answer("en")}/>
        <Button label={keepLabel()} variant="primary" onPress={() => answer(props.detected)}/>
      </View>
    </View>
  );
}

/** The first-run question, mounted once over the whole window. It has no close button: it is answered. */
export function FirstRunDialog() {
  const current = () => question();
  const detected = () => {
    const asked = question();
    return asked?.kind === "language" ? asked.detected : "en";
  };
  return (
    <Dialog style={centeredDialog} open={current() !== null} onOpenChange={() => undefined}>
      <Surface style={{ width: "100%", maxWidth: 560, padding: 20, gap: 14, backgroundColor: colors.popover }}>
        <If when={current()?.kind === "buildConsent"}>
          <BuildConsent/>
        </If>
        <If when={current()?.kind === "replayMigration"}>
          <ReplayMigration/>
        </If>
        <If when={current()?.kind === "language"}>
          <LanguageQuestion detected={detected()}/>
        </If>
      </Surface>
    </Dialog>
  );
}
