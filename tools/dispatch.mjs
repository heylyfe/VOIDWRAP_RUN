#!/usr/bin/env node
// Ticket dispatcher: sends one ticket to one subordinate model and writes the reply.
//
//   node tools/dispatch.mjs --target <slot> --ticket tickets/T-XXX.md [--via plan|opencode|api]
//                           [--image shot.png ...] [--timeout 900]
//
// Slots and default routes (see ROUTES below):
//   deepseek         plan      Vibe / Mistral plan endpoint  (unlimited)
//   glm              plan      Vibe / Mistral plan endpoint  (unlimited)
//   inkling-design   opencode  Zen, max reasoning
//   inkling-vision   opencode  Zen, max reasoning, accepts --image
//   nemotron-vision  RETIRED   (Zen free quota is metered; see VERIFICATION_LOG routing incident)
//   utility          RETIRED   (chores go to glm / deepseek)
//
// Subordinates run in an empty sandbox directory with every tool disabled: they see only
// the ticket text (and attached images). Keys are never read or logged here — Vibe reads
// its own keyring; the api route reads keys from the environment and never prints them.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync, copyFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const SYSTEM_PROMPT = join(root, "tools/subordinate-system.md");

export const ROUTES = {
  deepseek: { default: "plan", plan: "deepseek-v4", api: { url: "https://api.deepseek.com/chat/completions", model: "deepseek-chat", keyEnv: "DEEPSEEK_API_KEY" } },
  glm: { default: "plan", plan: "zai-glm-5-3-flash", api: { url: "https://open.bigmodel.cn/api/paas/v4/chat/completions", model: "glm-5.3-flash", keyEnv: "ZHIPU_API_KEY" } },
  "inkling-design": { default: "opencode", opencode: process.env.VW_INKLING_MODEL ?? "opencode/inkling-2-small-free#max" },
  "inkling-vision": { default: "opencode", opencode: process.env.VW_INKLING_MODEL ?? "opencode/inkling-2-small-free#max", vision: true },
  "nemotron-vision": { retired: "vision second opinions are skipped; OVERSEER reviews screenshots directly" },
  utility: { retired: "chores go to --target glm (docs) or --target deepseek (fixtures, seed data)" },
};

/** Never inlined into a ticket, whatever the ticket asks: secrets and local-only data. */
const INCLUDE_DENY = [/(^|\/)\.env/i, /(^|\/)shots\//, /key/i, /secret/i, /token/i, /\.pem$/i];

/**
 * Replace each line `@include <repo-relative path>` with that file's contents in a fenced block.
 * Lets OVERSEER reference current code instead of pasting it. Only files inside the repository
 * that are tracked-style source (no secrets, no screenshots) can be included.
 */
export function expandIncludes(text, depth = 0) {
  return text.replace(/^@include[ \t]+(\S+)[ \t]*$/gm, (_, rel) => {
    const norm = rel.replace(/\\/g, "/");
    if (norm.startsWith("/") || /^[a-zA-Z]:/.test(norm) || norm.split("/").includes("..")) throw new Error(`@include: unsafe path ${rel}`);
    if (INCLUDE_DENY.some((re) => re.test(norm))) throw new Error(`@include: refused ${rel}`);
    const abs = join(root, norm);
    if (!existsSync(abs)) throw new Error(`@include: not found ${rel}`);
    const ext = norm.split(".").pop() ?? "";
    // Another ticket (tickets/*.md) is inlined as text with ITS includes expanded too, so a follow-up ticket
    // that includes the original one still carries all of the original's source files.
    if (norm.startsWith("tickets/") && ext === "md") {
      if (depth >= 3) throw new Error(`@include: tickets nested too deeply at ${rel}`);
      return expandIncludes(readFileSync(abs, "utf8"), depth + 1);
    }
    return `\`${norm}\` (current contents):\n~~~~${ext}\n${readFileSync(abs, "utf8").replace(/\s+$/, "")}\n~~~~`;
  });
}

function parseArgs(argv) {
  const args = { images: [], timeout: 900 };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const next = () => {
      if (i + 1 >= argv.length) throw new Error(`${a} needs a value`);
      return argv[++i];
    };
    if (a === "--target") args.target = next();
    else if (a === "--ticket") args.ticket = next();
    else if (a === "--via") args.via = next();
    else if (a === "--image") args.images.push(next());
    else if (a === "--timeout") args.timeout = Number(next());
    else if (a === "--help" || a === "-h") args.help = true;
    else throw new Error(`unknown argument ${a}`);
  }
  return args;
}

/** Run a process with stdin closed (opencode blocks forever on prompts otherwise) and a hard timeout. */
function run(cmd, argv, { cwd, timeoutMs, env }) {
  return new Promise((resolvePromise) => {
    const child = spawn(cmd, argv, { cwd, env: { ...process.env, ...env }, stdio: ["ignore", "pipe", "pipe"], windowsHide: true });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d) => (stdout += d));
    child.stderr.on("data", (d) => (stderr += d));
    const timer = setTimeout(() => {
      child.kill();
      stderr += `\n[dispatch] killed after ${timeoutMs / 1000}s timeout`;
    }, timeoutMs);
    child.on("error", (e) => {
      clearTimeout(timer);
      resolvePromise({ code: -1, stdout, stderr: `${stderr}\n[dispatch] spawn failed: ${e.message}` });
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      resolvePromise({ code: code ?? -1, stdout, stderr });
    });
  });
}

const stripAnsi = (s) => s.replace(/\x1b\[[0-9;?]*[A-Za-z]/g, "").replace(/\r/g, "");

function opencodeBin() {
  if (process.env.VW_OPENCODE_BIN) return process.env.VW_OPENCODE_BIN;
  if (process.platform === "win32") {
    const exe = join(process.env.APPDATA ?? "", "npm/node_modules/@opencode/cli/bin/opencode.exe");
    if (existsSync(exe)) return exe;
  }
  return "opencode";
}

async function viaPlan(alias, ticketPath, sandbox, timeoutSec) {
  const exportDir = join(sandbox, ".export");
  const res = await run(
    process.env.VW_VIBE_BIN ?? "vibe",
    [
      "--prompt-file", ticketPath,
      "--system-prompt-file", SYSTEM_PROMPT,
      "--model", alias,
      "--workdir", sandbox,
      // enabled-tools alone leaves the code-mode wrapper visible; the regex disable removes every tool.
      "--enabled-tools", "none",
      "--disabled-tools", "re:.*",
      "--max-turns", "4",
      "--time-limit", String(timeoutSec),
      "--output-dir", exportDir,
    ],
    { cwd: sandbox, timeoutMs: (timeoutSec + 30) * 1000 },
  );
  let meta = {};
  try {
    const exp = JSON.parse(readFileSync(join(exportDir, "export.json"), "utf8"));
    meta = { usage: exp.usage, stop_reason: exp.stop_reason, error: exp.error ?? undefined };
  } catch {
    /* export is best-effort */
  }
  return { ...res, text: stripAnsi(res.stdout).trim(), meta };
}

async function viaOpencode(model, ticketPath, images, sandbox, timeoutSec) {
  // Deny every tool: the subordinate gets the ticket and images, nothing else.
  writeFileSync(join(sandbox, "opencode.json"), JSON.stringify({ permission: { "*": "deny" } }));
  const local = join(sandbox, "TICKET.md");
  copyFileSync(ticketPath, local);
  // Prefix with the attachment index: different screenshots often share a basename (e.g. 01-spawn.png).
  const files = [local, ...images.map((img, i) => {
    const dst = join(sandbox, `image-${String(i + 1).padStart(2, "0")}-${basename(img)}`);
    copyFileSync(img, dst);
    return dst;
  })];
  const sys = readFileSync(SYSTEM_PROMPT, "utf8");
  const msg = `${sys}\n\nThe ticket is attached as TICKET.md${images.length ? ` and ${images.length} screenshot(s) are attached` : ""}. Answer it now in plain text. You have no tools.`;
  const res = await run(opencodeBin(), ["run", "-m", model, ...files.flatMap((f) => ["-f", f]), msg], {
    cwd: sandbox,
    timeoutMs: timeoutSec * 1000,
  });
  const text = stripAnsi(res.stdout)
    .split("\n")
    .filter((l) => !/^> \S+ · /.test(l.trim()))
    .join("\n")
    .trim();
  const limited = /free usage limit/i.test(text + res.stderr);
  return { ...res, text, meta: limited ? { error: "zen free usage limit reached" } : {} };
}

async function viaApi(cfg, ticketPath, timeoutSec) {
  const key = process.env[cfg.keyEnv];
  if (!key) return { code: 2, text: "", stderr: `${cfg.keyEnv} is not set`, meta: {} };
  const body = {
    model: cfg.model,
    messages: [
      { role: "system", content: readFileSync(SYSTEM_PROMPT, "utf8") },
      { role: "user", content: readFileSync(ticketPath, "utf8") },
    ],
  };
  try {
    const r = await fetch(cfg.url, {
      method: "POST",
      headers: { "content-type": "application/json", authorization: `Bearer ${key}` },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(timeoutSec * 1000),
    });
    const json = await r.json().catch(() => ({}));
    if (!r.ok) return { code: 1, text: "", stderr: `HTTP ${r.status}: ${JSON.stringify(json).slice(0, 500)}`, meta: {} };
    return { code: 0, text: json.choices?.[0]?.message?.content?.trim() ?? "", stderr: "", meta: { usage: json.usage } };
  } catch (e) {
    return { code: 1, text: "", stderr: String(e), meta: {} };
  }
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help || !args.target || !args.ticket) {
    console.log(readFileSync(fileURLToPath(import.meta.url), "utf8").split("\n").slice(1, 17).map((l) => l.replace(/^\/\/ ?/, "")).join("\n"));
    process.exit(args.help ? 0 : 2);
  }
  const route = ROUTES[args.target];
  if (!route) throw new Error(`unknown target ${args.target}`);
  if (route.retired) throw new Error(`target ${args.target} is retired: ${route.retired}`);
  const via = args.via ?? route.default;
  if (!route[via]) throw new Error(`target ${args.target} has no '${via}' route`);
  if (args.images.length && !route.vision) throw new Error(`target ${args.target} is text-only; --image not allowed`);
  const sourceTicket = resolve(args.ticket);
  if (!existsSync(sourceTicket)) throw new Error(`ticket not found: ${sourceTicket}`);
  for (const img of args.images) if (!existsSync(img)) throw new Error(`image not found: ${img}`);

  const outPath = sourceTicket.replace(/\.md$/, "") + ".response.md";
  // Expand `@include path` lines into the repository file's contents (see expandIncludes).
  const ticketPath = join(mkdtempSync(join(tmpdir(), "vw-ticket-")), "TICKET.md");
  writeFileSync(ticketPath, expandIncludes(readFileSync(sourceTicket, "utf8")));
  const maxAttempts = 3;
  let result;
  let attempt = 0;
  const started = Date.now();
  while (attempt < maxAttempts) {
    attempt++;
    const sandbox = mkdtempSync(join(tmpdir(), "vw-dispatch-"));
    try {
      if (via === "plan") result = await viaPlan(route.plan, ticketPath, sandbox, args.timeout);
      else if (via === "opencode") result = await viaOpencode(route.opencode, ticketPath, args.images, sandbox, args.timeout);
      else result = await viaApi(route.api, ticketPath, args.timeout);
    } finally {
      rmSync(sandbox, { recursive: true, force: true });
    }
    if (result.code === 0 && result.text && !result.meta.error) break;
    if (result.meta.error?.includes("free usage limit")) break; // retrying cannot help
    console.error(`[dispatch] attempt ${attempt} failed (exit ${result.code}): ${(result.meta.error ?? result.stderr).toString().slice(-300)}`);
    if (attempt < maxAttempts) await new Promise((r) => setTimeout(r, 5000 * attempt));
  }
  const ok = result.code === 0 && !!result.text && !result.meta.error;
  const model = via === "api" ? route.api.model : route[via];
  const header = [
    `<!-- dispatch: target=${args.target} via=${via} model=${model} ok=${ok} attempts=${attempt}`,
    `     seconds=${Math.round((Date.now() - started) / 1000)} images=${args.images.length}${result.meta.usage ? ` usage=${JSON.stringify(result.meta.usage)}` : ""} -->`,
    "",
  ].join("\n");
  mkdirSync(dirname(outPath), { recursive: true });
  writeFileSync(outPath, header + (ok ? result.text : `DISPATCH FAILED\n\n${result.meta.error ?? ""}\n${result.stderr.slice(-2000)}`) + "\n");
  console.log(`[dispatch] ${ok ? "ok" : "FAILED"} → ${outPath}`);
  process.exit(ok ? 0 : 1);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((e) => {
    console.error(`[dispatch] ${e.message}`);
    process.exit(2);
  });
}
