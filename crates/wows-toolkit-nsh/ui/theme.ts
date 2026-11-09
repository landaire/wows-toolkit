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
};

export const controlRadius = () => token("radius");
