import { mount, View } from "nsh";
import { OptionsRail } from "../../ui/armor/options";
import { armor } from "../../ui/armor/state";
import { fakeArmor, partialZone } from "./fake_state";

armor.set(fakeArmor([partialZone], null));

function Rail() {
  return (
    <View style={{ height: 900, flexDirection: "row" }}>
      <OptionsRail/>
    </View>
  );
}

mount(Rail);
