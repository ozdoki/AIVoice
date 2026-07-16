import assert from "node:assert/strict";
import test from "node:test";
import { shouldClearFloatingBarTerminalTimer } from "../src/floatingBarTimerPolicy.ts";

test("settings-driven listener re-registration keeps the terminal hide timer", () => {
  assert.equal(shouldClearFloatingBarTerminalTimer("listener_reregister"), false);
});

test("new sessions and component unmount clear the terminal hide timer", () => {
  assert.equal(shouldClearFloatingBarTerminalTimer("new_session_event"), true);
  assert.equal(shouldClearFloatingBarTerminalTimer("component_unmount"), true);
});
