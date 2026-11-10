import { mount, View } from "nsh";
import { Button } from "../../ui/components";
import { clearStatusJob, setStatusJob, StatusBar } from "../../ui/shell/status";

function StatusStrip() {
  return (
    <View style={{ flex: 1 }}>
      <Button label="Start" onPress={() => setStatusJob("sample", { label: () => "Sampling", done: 1, total: 3 })}/>
      <Button label="Finish" onPress={() => clearStatusJob("sample")}/>
      <StatusBar/>
    </View>
  );
}

mount(StatusStrip);
