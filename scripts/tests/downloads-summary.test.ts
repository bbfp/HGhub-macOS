import assert from "node:assert/strict";
import test from "node:test";
import { isTaskDeletable, matchesDownloadFilter, summarizeDownloads, unfinishedTaskCount } from "../../src/downloadsSummary.ts";

const task = (drama_id: number, status: "queued" | "downloading" | "paused" | "failed" | "cancelled" | "completed") => ({ drama_id, status });

test("summarizeDownloads separates unfinished states and counts dramas", () => {
  assert.deepEqual(summarizeDownloads([
    task(1, "downloading"), task(1, "queued"), task(2, "paused"),
    task(2, "failed"), task(3, "cancelled"), task(4, "completed"),
  ]), {
    dramas: 4,
    unfinished_dramas: 3,
    unfinished_tasks: 5,
    downloading: 1,
    queued: 1,
    paused: 1,
    failed: 2,
    completed: 1,
  });
});

test("only failed and cancelled tasks are deletable", () => {
  assert.equal(isTaskDeletable("failed"), true);
  assert.equal(isTaskDeletable("cancelled"), true);
  assert.equal(isTaskDeletable("queued"), false);
  assert.equal(isTaskDeletable("downloading"), false);
  assert.equal(isTaskDeletable("paused"), false);
  assert.equal(isTaskDeletable("completed"), false);
});

test("unfinished task count includes retryable records but excludes completed episodes", () => {
  assert.equal(unfinishedTaskCount([
    task(1, "downloading"), task(1, "queued"), task(2, "paused"),
    task(2, "failed"), task(3, "cancelled"), task(4, "completed"),
  ]), 5);
});

test("failed filter groups failed and cancelled tasks together", () => {
  assert.equal(matchesDownloadFilter("failed", "failed"), true);
  assert.equal(matchesDownloadFilter("cancelled", "failed"), true);
  assert.equal(matchesDownloadFilter("completed", "failed"), false);
  assert.equal(matchesDownloadFilter("queued", "queued"), true);
  assert.equal(matchesDownloadFilter("downloading", "queued"), false);
});
