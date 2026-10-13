#!/usr/bin/env node
/**
 * Downloads the local whisper.cpp model that powers Nova's wake-word detection
 * and offline speech-to-text.
 *
 * Default model: ggml-tiny.en.bin (~75 MB) from the whisper.cpp repository.
 *
 * Overridable via environment variables:
 *   NOVA_MODELS_DIR  - directory to store models in (default: <repo>/models)
 *   NOVA_MODEL_NAME  - file name to download (default: ggml-tiny.en.bin)
 *   NOVA_MODEL_URL   - full URL to download from
 *
 * Usage: npm run models
 */
import { createWriteStream, existsSync, mkdirSync, statSync, unlinkSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import https from "node:https";

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, "..");
const modelsDir = process.env.NOVA_MODELS_DIR
  ? resolve(process.env.NOVA_MODELS_DIR)
  : join(repoRoot, "models");
const modelName = process.env.NOVA_MODEL_NAME || "ggml-tiny.en.bin";
const url =
  process.env.NOVA_MODEL_URL ||
  `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/${modelName}`;
const target = join(modelsDir, modelName);

// Sanity floor: whisper models are at least tens of MB. Guards against
// saving an error page as if it were a model.
const MIN_BYTES = 20 * 1024 * 1024;

const log = (...args) => console.log("[nova:models]", ...args);

async function main() {
  mkdirSync(modelsDir, { recursive: true });

  if (existsSync(target) && statSync(target).size >= MIN_BYTES) {
    log(`model already present: ${target}`);
    log("nothing to do.");
    return;
  }

  log(`downloading ${url}`);
  log(`-> ${target}`);

  try {
    await download(url, target, 0);
  } catch (err) {
    cleanup();
    log(`download failed: ${err.message}`);
    log("");
    log("You can also download the model manually and place it at:");
    log(`  ${target}`);
    log(`Source: ${url}`);
    process.exitCode = 1;
    return;
  }

  const size = statSync(target).size;
  if (size < MIN_BYTES) {
    cleanup();
    log(`downloaded file is unexpectedly small (${size} bytes); treating as failure.`);
    process.exitCode = 1;
    return;
  }

  log(`done: ${(size / 1048576).toFixed(1)} MB`);
}

function cleanup() {
  try {
    if (existsSync(target)) unlinkSync(target);
  } catch {
    /* ignore */
  }
}

function download(u, dest, redirects) {
  return new Promise((resolvePromise, reject) => {
    const req = https.get(
      u,
      { headers: { "user-agent": "nova-model-fetcher" } },
      (res) => {
        if (
          [301, 302, 303, 307, 308].includes(res.statusCode) &&
          res.headers.location &&
          redirects < 5
        ) {
          res.resume();
          resolvePromise(
            download(new URL(res.headers.location, u).toString(), dest, redirects + 1)
          );
          return;
        }
        if (res.statusCode !== 200) {
          res.resume();
          reject(new Error(`HTTP ${res.statusCode}`));
          return;
        }
        const total = parseInt(res.headers["content-length"] || "0", 10);
        let received = 0;
        let lastTick = Date.now();
        const out = createWriteStream(dest);
        res.on("data", (chunk) => {
          received += chunk.length;
          if (Date.now() - lastTick > 500) {
            lastTick = Date.now();
            const line = total
              ? `\r  ${(received / 1048576).toFixed(1)} / ${(total / 1048576).toFixed(1)} MB`
              : `\r  ${(received / 1048576).toFixed(1)} MB`;
            process.stdout.write(line);
          }
        });
        res.on("error", reject);
        out.on("error", reject);
        out.on("finish", () => {
          out.close(() => {
            process.stdout.write("\n");
            resolvePromise();
          });
        });
        res.pipe(out);
      }
    );
    req.on("error", reject);
  });
}

main();
