export type Platform = "android" | "windows" | "macos";

export type StartupTab = "home" | "profile";

export function detectPlatform(userAgent: string): Platform {
  if (/Android/i.test(userAgent)) return "android";
  if (/Macintosh|Mac OS X/i.test(userAgent)) return "macos";
  return "windows";
}

/** Wry maps custom schemes to HTTP only on Windows/Android. */
export function mediaBase(platform: Platform): string {
  return platform === "macos" ? "hg://localhost" : "http://hg.localhost";
}

export function defaultStartupTab(platform: Platform): StartupTab {
  return platform === "macos" ? "profile" : "home";
}
