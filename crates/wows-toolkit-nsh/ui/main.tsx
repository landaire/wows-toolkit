import { requestedWindow } from "./shell/windows";

// The bundle every window runs. The main window's shell is imported only in
// the main window: importing it starts its startup work, which a board or
// renderer window must not repeat.
const request = requestedWindow();
if (request) {
  void import("./shell/window_root").then((root) => root.mountWindow(request));
} else {
  void import("./App");
}
