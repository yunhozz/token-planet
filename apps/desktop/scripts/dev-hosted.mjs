import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseEnv } from "node:util";

const desktopDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const envPath = resolve(desktopDir, ".env.local");
const npmCli = process.env.npm_execpath;

function fail(message) {
  console.error(`Hosted Supabase launcher: ${message}`);
  process.exit(1);
}

if (!npmCli) {
  fail("Run this script with `npm run dev:hosted`.");
}

let hostedSettings;
try {
  hostedSettings = parseEnv(readFileSync(envPath, "utf8"));
} catch {
  fail("Could not read .env.local. Copy .env.example to .env.local and set the hosted values.");
}

const hostedUrl = hostedSettings.TOKEN_PLANET_SUPABASE_URL?.trim();
if (!hostedUrl) {
  fail(".env.local must set TOKEN_PLANET_SUPABASE_URL.");
}

let parsedUrl;
try {
  parsedUrl = new URL(hostedUrl);
} catch {
  fail("TOKEN_PLANET_SUPABASE_URL must be a valid HTTPS URL.");
}

if (parsedUrl.protocol !== "https:") {
  fail("TOKEN_PLANET_SUPABASE_URL must use HTTPS.");
}

const publishableKey = hostedSettings.TOKEN_PLANET_SUPABASE_PUBLISHABLE_KEY?.trim();
if (!publishableKey) {
  fail(".env.local must set TOKEN_PLANET_SUPABASE_PUBLISHABLE_KEY.");
}

const tauri = spawn(
  process.execPath,
  [npmCli, "run", "tauri", "--", "dev"],
  {
    cwd: desktopDir,
    env: {
      ...process.env,
      TOKEN_PLANET_SUPABASE_URL: parsedUrl.toString().replace(/\/$/, ""),
      TOKEN_PLANET_SUPABASE_PUBLISHABLE_KEY: publishableKey,
    },
    stdio: "inherit",
  },
);

tauri.on("error", () => {
  console.error("Hosted Supabase launcher: Could not start Tauri.");
  process.exitCode = 1;
});

tauri.on("exit", (code) => {
  process.exitCode = code ?? 1;
});
