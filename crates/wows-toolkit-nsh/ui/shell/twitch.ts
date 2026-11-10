import { signal } from "nsh";
import { Twitch, type TwitchStatus } from "wows-toolkit/native";
import { t } from "../i18n";
import * as toast from "./toast";

const STUCK_TWITCH_TOKEN = "twitch-token";

/** What Twitch made of the stored credential. `null` while it is being asked. */
export const twitchStatus = signal<TwitchStatus | null>({ state: "unset" });

/** Restarts the chat poll under the stored credential and channel, and reports a refused credential until it is fixed. */
export async function restartTwitch() {
  twitchStatus.set(null);
  try {
    const status = await Twitch.restart();
    twitchStatus.set(status);
    if (status.state === "refused") {
      toast.stuck(STUCK_TWITCH_TOKEN, t("ui.messages.twitch_token_invalid"));
    } else {
      toast.resolved(STUCK_TWITCH_TOKEN);
    }
  } catch (error) {
    twitchStatus.set({ state: "unset" });
    toast.failed(error instanceof Error ? error.message : String(error));
  }
}
