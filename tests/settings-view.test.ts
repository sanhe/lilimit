import assert from "node:assert/strict";
import test from "node:test";

import { renderSettingsHeader } from "../src/settings-view.ts";

test("settings header owns its close control and shows the application version", () => {
  const header = renderSettingsHeader("0.1.3");

  assert.match(header, /data-tauri-drag-region/);
  assert.match(header, /class="icon-button settings-close-button close-settings-button"/);
  assert.match(header, /aria-label="Close settings"/);
  assert.match(header, /Version 0\.1\.3/);
});

test("settings version is escaped before rendering", () => {
  const header = renderSettingsHeader('<script>alert("x")</script>');

  assert.doesNotMatch(header, /<script>/);
  assert.match(header, /&lt;script&gt;alert\(&quot;x&quot;\)&lt;\/script&gt;/);
});
