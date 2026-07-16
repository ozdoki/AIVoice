import assert from "node:assert/strict";
import test from "node:test";
import { runFloatingBarTerminalPresentation } from "../src/floatingBarTerminalController.ts";

function deferred() {
  let release!: () => void;
  const promise = new Promise<void>((resolve) => { release = resolve; });
  return { promise, release };
}

test("recording event during terminal resize prevents stale hide scheduling", async () => {
  let epoch = 1;
  let scheduled = 0;
  const resize = deferred();
  const run = runFloatingBarTerminalPresentation({
    epoch: 1,
    isCurrent: (captured) => captured === epoch,
    resize: () => resize.promise,
    show: async () => undefined,
    scheduleHide: () => { scheduled += 1; },
  });
  epoch = 2;
  resize.release();
  assert.equal(await run, false);
  assert.equal(scheduled, 0);
});

test("unmount during terminal show prevents stale hide scheduling", async () => {
  let disposed = false;
  let scheduled = 0;
  const show = deferred();
  const run = runFloatingBarTerminalPresentation({
    epoch: 1,
    isCurrent: () => !disposed,
    resize: async () => undefined,
    show: () => show.promise,
    scheduleHide: () => { scheduled += 1; },
  });
  await Promise.resolve();
  disposed = true;
  show.release();
  assert.equal(await run, false);
  assert.equal(scheduled, 0);
});
