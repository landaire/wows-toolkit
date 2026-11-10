import { mount, View } from "nsh";
import { Icon } from "../../ui/icons";

function IconView() {
  return (
    <View style={{ flexDirection: "row", gap: 8, color: "#00ff00" }}>
      <Icon name="search" size={24} accessibilityLabel="Search"/>
      <Icon name="x" color="#ff0000" strokeWidth={1.5} accessibilityLabel="Close"/>
      <Icon name="twitch" color="#0000ff" accessibilityLabel="Twitch"/>
    </View>
  );
}

mount(IconView);
