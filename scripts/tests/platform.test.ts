import assert from "node:assert/strict";
import test from "node:test";
import { detectPlatform, mediaBase } from "../../src/platform.ts";

test("macOS uses its registered custom scheme for media", () => {
  const platform = detectPlatform("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15");
  assert.equal(platform, "macos");
  assert.equal(mediaBase(platform), "hg://localhost");
});

test("Windows and Android keep their existing media origins", () => {
  assert.equal(mediaBase(detectPlatform("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")), "http://hg.localhost");
  assert.equal(mediaBase(detectPlatform("Mozilla/5.0 (Linux; Android 15)")), "http://hg.localhost");
});
