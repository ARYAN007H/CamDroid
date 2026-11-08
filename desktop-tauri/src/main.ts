import "@fontsource-variable/inter";
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/components.css";
import "./styles/connect.css";
import "./styles/studio.css";

import { errorMessage } from "./core/format";
import { loadPlatform } from "./core/platform";
import { startApp } from "./features/app";

const root = document.getElementById("app");
if (!root) throw new Error("#app is missing from index.html");

loadPlatform()
  .then((platform) => startApp(root, platform).catch((error: unknown) => showFatal(root, error, platform.window.show)))
  .catch((error: unknown) => showFatal(root, error));

/** If start-up fails the window must still appear, with a readable reason. */
function showFatal(host: HTMLElement, error: unknown, show?: () => Promise<void>): void {
  host.innerHTML = "";
  const message = document.createElement("pre");
  message.style.cssText = "padding:32px;white-space:pre-wrap;color:#ffb4ae;font:13px ui-monospace,monospace;user-select:text";
  message.textContent = `CamDroid could not start.\n\n${errorMessage(error)}`;
  host.append(message);
  void show?.();
}
