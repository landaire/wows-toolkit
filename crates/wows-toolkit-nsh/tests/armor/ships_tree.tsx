import { mount, View } from "nsh";
import { ShipsSidebar } from "../../ui/armor/sidebar";
import { armor, catalog, type CatalogShip } from "../../ui/armor/state";
import { fakeArmor } from "./fake_state";

const ship = (paramIndex: string, tier: number, roman: string, name: string): CatalogShip => ({ paramIndex, name, tier, roman, label: `${roman} ${name}` });

// Set on a press, so the tree is made in the environment the test applied first.
function loadCatalog() {
  catalog.set({
    nations: [
      {
        nation: "usa",
        label: "U.S.A.",
        classes: [
          { species: "Destroyer", label: "Destroyers", ships: [ship("PASD001", 2, "II", "Sampson"), ship("PASD002", 3, "III", "Wickes")] },
          { species: "Cruiser", label: "Cruisers", ships: [ship("PASC001", 1, "I", "Erie")] },
        ],
      },
      {
        nation: "japan",
        label: "Japan",
        classes: [{ species: "Battleship", label: "Battleships", ships: [ship("PJSB001", 3, "III", "Kawachi")] }],
      },
    ],
  });
}

// The service reports Kawachi open in the active pane, as a palette pick or a replay does.
function showKawachi() {
  const state = fakeArmor([], null);
  state.panes[0].paramIndex = "PJSB001";
  armor.set(state);
}

function Ships() {
  return (
    <View style={{ width: 240, height: 600 }}>
      <View testID="fixture-load-catalog" role="button" onPress={loadCatalog} style={{ height: 20 }}/>
      <View testID="fixture-show-kawachi" role="button" onPress={showKawachi} style={{ height: 20 }}/>
      <ShipsSidebar/>
    </View>
  );
}

mount(Ships);
