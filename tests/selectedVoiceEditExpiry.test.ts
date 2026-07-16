import assert from "node:assert/strict";
import test from "node:test";
import { shouldFinalizeSelectedVoiceEditExpiry } from "../src/selectedVoiceEditExpiry.ts";

test("expiry waits for a mutation and finalizes once it becomes idle", () => {
  assert.equal(shouldFinalizeSelectedVoiceEditExpiry(true, "replace", false, false), false);
  assert.equal(shouldFinalizeSelectedVoiceEditExpiry(true, null, false, false), true);
});

test("completed or already-finalizing expiry cannot cancel or close twice", () => {
  assert.equal(shouldFinalizeSelectedVoiceEditExpiry(true, null, true, false), false);
  assert.equal(shouldFinalizeSelectedVoiceEditExpiry(true, null, false, true), false);
  assert.equal(shouldFinalizeSelectedVoiceEditExpiry(false, null, false, false), false);
});
