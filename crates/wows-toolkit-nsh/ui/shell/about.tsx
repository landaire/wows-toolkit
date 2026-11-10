import { signal, Text, View } from "nsh";
import { Dialog } from "nsh/base";
import { Shell } from "wows-toolkit/native";
import { Button, centeredDialog, Surface } from "../components";
import { t } from "../i18n";
import { colors } from "../theme";

/** What the PR figures are credited to, which the About window links. */
const PR_INFO_URL = "https://wows-numbers.com/personal/rating";
export const PROJECT_URL = "https://github.com/landaire/wows-toolkit";

const open = signal(false);

export function openAbout() {
  open.set(true);
}

/** Who made this, what it is built on, and where to look next; this build's version first, which a bug report needs. */
export function AboutDialog(props: { version: string }) {
  const lines = () => [
    `WoWs Toolkit v${props.version}`,
    t("ui.labels.made_by"),
    t("ui.labels.credits"),
    `${t("ui.labels.pr_credits")} ${PR_INFO_URL}`,
    PROJECT_URL,
  ];
  return (
    <Dialog style={centeredDialog} open={open()} onOpenChange={({ open: shown }) => open.set(shown)}>
      <Surface style={{ width: "100%", maxWidth: 520, padding: 20, gap: 12, backgroundColor: colors.popover }}>
        <Text style={{ fontSize: 17, fontWeight: "600", color: colors.popoverForeground }}>{t("ui.windows.about")}</Text>
        <View selectable style={{ gap: 10 }}>
          <Each of={lines()}>
            {(line) => <Text style={{ fontSize: 13, lineHeight: 19, color: colors.popoverForeground }}>{line()}</Text>}
          </Each>
        </View>
        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
          <Button label={t("ui.buttons.cancel")} onPress={() => open.set(false)}/>
          <Button
            label={t("ui.buttons.view_github")}
            variant="primary"
            onPress={() => {
              open.set(false);
              void Shell.openUrl(PROJECT_URL);
            }}
          />
        </View>
      </Surface>
    </Dialog>
  );
}
