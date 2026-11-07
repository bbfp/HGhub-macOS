import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const action = process.argv[2] || "build";
const env = {
  ...process.env,
  RUSTUP_HOME: path.join(root, ".local/rustup"),
  CARGO_HOME: path.join(root, ".local/cargo"),
  RUSTUP_TOOLCHAIN: "1.90.0-aarch64-apple-darwin",
  PATH: `${path.join(root, ".local/rustup/toolchains/1.90.0-aarch64-apple-darwin/bin")}:${process.env.PATH}`,
};
if (action === "cargo" && process.argv[3] === "test") {
  // Keep loopback fixture traffic local even when macOS has a system proxy.
  // This changes only the test process; external hosts retain their routing.
  env.NO_PROXY = [process.env.NO_PROXY || process.env.no_proxy, "127.0.0.1", "localhost", "::1"].filter(Boolean).join(",");
  env.no_proxy = env.NO_PROXY;
}
const args = action === "cargo" ? [process.argv[3], "--manifest-path", "src-tauri/Cargo.toml", ...process.argv.slice(4)] : action === "dev" ? ["tauri", "dev"] : ["tauri", "build", "--target", "aarch64-apple-darwin", ...process.argv.slice(3)];
const child = spawn(action === "cargo" ? "cargo" : "pnpm", args, { cwd: root, env, stdio: "inherit" });
child.on("exit", (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  else process.exitCode = code ?? 1;
});
child.on("error", (error) => { console.error(error.message); process.exitCode = 1; });
