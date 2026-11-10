import { mount, View } from "nsh";
import { ExportDialog } from "../../ui/armor/export";
import { armor } from "../../ui/armor/state";
import { fakeArmor, fakeExportDialog } from "./fake_state";

armor.set(fakeArmor([], fakeExportDialog("armor")));

function Export() {
  return (
    <View style={{ flex: 1 }}>
      <View testID="fixture-export-mesh" role="button" onPress={() => armor.set(fakeArmor([], fakeExportDialog("mesh")))} style={{ height: 20 }}/>
      <ExportDialog/>
    </View>
  );
}

mount(Export);
