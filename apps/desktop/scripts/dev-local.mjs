import { spawn, spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const desktopDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repositoryRoot = resolve(desktopDir, "../..");
const npmCli = process.env.npm_execpath;

function fail(message) {
  console.error(`Local Supabase launcher: ${message}`);
  process.exit(1);
}

if (!npmCli) {
  fail("Run this script with `npm run dev:local`.");
}

const statusResult = spawnSync(
  process.execPath,
  [npmCli, "exec", "--yes", "--package=supabase@2.118.0", "--", "supabase", "status", "-o", "json"],
  { cwd: repositoryRoot, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] },
);

if (statusResult.error || statusResult.status !== 0) {
  fail("Could not read local Supabase status. Start local Supabase first.");
}

let localStatus;
try {
  localStatus = JSON.parse(statusResult.stdout);
} catch {
  fail("Local Supabase status was not valid JSON.");
}

let apiUrl;
try {
  apiUrl = new URL(localStatus.API_URL);
} catch {
  fail("Local Supabase did not provide a valid API_URL.");
}

if (
  apiUrl.protocol !== "http:" ||
  !["localhost", "127.0.0.1", "[::1]"].includes(apiUrl.hostname) ||
  apiUrl.username ||
  apiUrl.password
) {
  fail("API_URL must use HTTP on localhost or a loopback address.");
}

const publishableKey = localStatus.PUBLISHABLE_KEY;
if (typeof publishableKey !== "string" || !publishableKey.trim()) {
  fail("Local Supabase did not provide a publishable key.");
}

const tauri = spawn(
  process.execPath,
  [npmCli, "run", "tauri", "--", "dev"],
  {
    cwd: desktopDir,
    env: {
      ...process.env,
      TOKEN_PLANET_SUPABASE_URL: apiUrl.toString().replace(/\/$/, ""),
      TOKEN_PLANET_SUPABASE_PUBLISHABLE_KEY: publishableKey.trim(),
    },
    stdio: "inherit",
  },
);

tauri.on("error", () => {
  console.error("Local Supabase launcher: Could not start Tauri.");
  process.exitCode = 1;
});

tauri.on("exit", (code) => {
  process.exitCode = code ?? 1;
});
