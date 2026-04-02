import { spawn } from "node:child_process";

const args = process.argv.slice(2);
const env = { ...process.env };

const shouldEnableCiForHeadlessDmg =
  process.platform === "darwin" &&
  args[0] === "build" &&
  !process.stdout.isTTY &&
  env.CI === undefined;

if (shouldEnableCiForHeadlessDmg) {
  env.CI = "true";
  console.error(
    "info: enabling CI=true for headless macOS DMG bundling to skip Finder AppleScript.",
  );
}

const command = process.platform === "win32" ? "tauri.cmd" : "tauri";
const child = spawn(command, args, {
  env,
  stdio: "inherit",
});

child.on("error", (error) => {
  console.error(error);
  process.exit(1);
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }

  process.exit(code ?? 1);
});
