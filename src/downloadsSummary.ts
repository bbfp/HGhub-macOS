import type { DownloadStatus } from "./types";

export type DownloadFilter = "all" | "downloading" | "queued" | "paused" | "failed";

export type DownloadSummaryTask = Pick<{ drama_id: number; status: DownloadStatus }, "drama_id" | "status">;

export type DownloadOverview = {
  dramas: number;
  unfinished_dramas: number;
  unfinished_tasks: number;
  downloading: number;
  queued: number;
  paused: number;
  failed: number;
  completed: number;
};

export function isTaskDeletable(status: DownloadStatus): boolean {
  return status === "failed" || status === "cancelled";
}

export function unfinishedTaskCount(tasks: DownloadSummaryTask[]): number {
  return tasks.filter((task) => task.status !== "completed").length;
}

export function matchesDownloadFilter(status: DownloadStatus, filter: DownloadFilter): boolean {
  if (filter === "all") return true;
  if (filter === "failed") return status === "failed" || status === "cancelled";
  return status === filter;
}

export function summarizeDownloads(tasks: DownloadSummaryTask[]): DownloadOverview {
  const dramas = new Set(tasks.map((task) => task.drama_id));
  const unfinishedDramas = new Set(
    tasks.filter((task) => task.status !== "completed").map((task) => task.drama_id),
  );
  return {
    dramas: dramas.size,
    unfinished_dramas: unfinishedDramas.size,
    unfinished_tasks: unfinishedTaskCount(tasks),
    downloading: tasks.filter((task) => task.status === "downloading").length,
    queued: tasks.filter((task) => task.status === "queued").length,
    paused: tasks.filter((task) => task.status === "paused").length,
    failed: tasks.filter((task) => task.status === "failed" || task.status === "cancelled").length,
    completed: tasks.filter((task) => task.status === "completed").length,
  };
}
