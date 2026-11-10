import { token } from "nsh";

export const colors = {
  get background() { return token("background"); },
  get foreground() { return token("foreground"); },
  get card() { return token("card"); },
  get cardForeground() { return token("card-foreground"); },
  get popover() { return token("popover"); },
  get popoverForeground() { return token("popover-foreground"); },
  get primary() { return token("primary"); },
  get primaryForeground() { return token("primary-foreground"); },
  get secondary() { return token("secondary"); },
  get secondaryForeground() { return token("secondary-foreground"); },
  get muted() { return token("muted"); },
  get mutedForeground() { return token("muted-foreground"); },
  get accent() { return token("accent"); },
  get accentForeground() { return token("accent-foreground"); },
  get destructive() { return token("destructive"); },
  get border() { return token("border"); },
  get input() { return token("input"); },
  get ring() { return token("ring"); },
  /** Behind the panels: the window chrome, the tab strip, the status bar. */
  get surface() { return token("surface"); },
  /** A rule that has to be seen, such as the one under the tab strip. */
  get borderBright() { return token("border-bright"); },
  /** What a selected row is filled with. */
  get selection() { return token("selection"); },
  get textBright() { return token("text-bright"); },
  get textFaint() { return token("text-faint"); },
};

/** What a color means rather than what it looks like, in the palette shown. */
export const semantic = {
  get win() { return token("win"); },
  get loss() { return token("loss"); },
  get draw() { return token("draw"); },
  get warn() { return token("warn"); },
  get error() { return token("error"); },
  get ok() { return token("ok"); },
  get notice() { return token("notice"); },
};

export const controlRadius = () => token("radius");
