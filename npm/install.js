"use strict";

const https = require("https");
const http = require("http");
const fs = require("fs");
const path = require("path");
const { execFileSync } = require("child_process");
const os = require("os");

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const PACKAGE = require("./package.json");
const VERSION = PACKAGE.version;
const REPO = "George-RD/mag";
const BASE_URL = `https://github.com/${REPO}/releases/download/v${VERSION}`;

const PLATFORM_MAP = {
  darwin: {
    x64: "x86_64-apple-darwin",
    arm64: "aarch64-apple-darwin",
  },
  linux: {
    x64: "x86_64-unknown-linux-gnu",
    arm64: "aarch64-unknown-linux-gnu",
  },
  win32: {
    x64: "x86_64-pc-windows-msvc",
  },
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Resolve the Rust target triple for the current platform/arch.
 */
function resolveTarget() {
  const platform = process.platform;
  const arch = process.arch;

  const targets = PLATFORM_MAP[platform];
  if (!targets) {
    throw new Error(
      `Unsupported platform: ${platform}. ` +
        `Supported: ${Object.keys(PLATFORM_MAP).join(", ")}`
    );
  }

  const triple = targets[arch];
  if (!triple) {
    throw new Error(
      `Unsupported architecture: ${arch} on ${platform}. ` +
        `Supported: ${Object.keys(targets).join(", ")}`
    );
  }

  return triple;
}

/**
 * Follow redirects and download a URL to a local file.
 * Returns a Promise that resolves when the file is fully written.
 */
function download(url, dest, maxRedirects) {
  if (maxRedirects === undefined) maxRedirects = 5;

  return new Promise(function (resolve, reject) {
    if (maxRedirects < 0) {
      return reject(new Error("Too many redirects"));
    }

    var proto = url.startsWith("https") ? https : http;

    proto
      .get(url, function (res) {
        // Handle redirects (GitHub releases redirect to S3)
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          res.resume(); // drain response
          return download(res.headers.location, dest, maxRedirects - 1).then(
            resolve,
            reject
          );
        }

        if (res.statusCode !== 200) {
          res.resume();
          return reject(
            new Error(
              "Download failed: HTTP " + res.statusCode + " for " + url
            )
          );
        }

        var file = fs.createWriteStream(dest);
        res.pipe(file);
        file.on("finish", function () {
          file.close(resolve);
        });
        file.on("error", function (err) {
          fs.unlink(dest, function () {}); // clean up
          reject(err);
        });
      })
      .on("error", function (err) {
        reject(err);
      });
  });
}

/**
 * Extract a .tar.gz archive into `destDir` using the system tar command.
 */
function extractTarGz(archivePath, destDir) {
  fs.mkdirSync(destDir, { recursive: true });
  execFileSync("tar", ["xzf", archivePath, "-C", destDir], {
    stdio: "inherit",
  });
}

/**
 * Extract a .zip archive into `destDir`.
 * Uses PowerShell on Windows, unzip elsewhere.
 */
function extractZip(archivePath, destDir) {
  fs.mkdirSync(destDir, { recursive: true });

  if (process.platform === "win32") {
    execFileSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-Command",
        "Expand-Archive",
        "-Path",
        archivePath,
        "-DestinationPath",
        destDir,
        "-Force",
      ],
      { stdio: "inherit" }
    );
  } else {
    execFileSync("unzip", ["-o", archivePath, "-d", destDir], {
      stdio: "inherit",
    });
  }
}

// ---------------------------------------------------------------------------
// Main install logic
// ---------------------------------------------------------------------------

async function main() {
  var binDir = path.join(__dirname, "bin");

  // Respect MAG_BINARY_PATH for local development
  var localBinary = process.env.MAG_BINARY_PATH;
  if (localBinary) {
    console.log("MAG_BINARY_PATH set — linking local binary: " + localBinary);

    if (!fs.existsSync(localBinary)) {
      throw new Error("MAG_BINARY_PATH points to missing file: " + localBinary);
    }

    fs.mkdirSync(binDir, { recursive: true });

    var dest = path.join(binDir, process.platform === "win32" ? "mag.exe" : "mag");

    // Remove existing file/link before creating a new one
    try {
      fs.unlinkSync(dest);
    } catch (_) {
      // ignore if doesn't exist
    }

    fs.symlinkSync(path.resolve(localBinary), dest);
    console.log("Linked " + dest + " -> " + localBinary);
    return;
  }

  var target = resolveTarget();
  var isWindows = process.platform === "win32";
  var ext = isWindows ? ".zip" : ".tar.gz";
  var archiveName = "mag-" + target + ext;
  var archiveUrl = BASE_URL + "/" + archiveName;

  var tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "mag-install-"));
  var archivePath = path.join(tmpDir, archiveName);

  try {
    console.log("Downloading mag v" + VERSION + " for " + target + "...");
    console.log("  " + archiveUrl);

    await download(archiveUrl, archivePath);

    console.log("Extracting...");
    fs.mkdirSync(binDir, { recursive: true });

    if (isWindows) {
      extractZip(archivePath, binDir);
    } else {
      extractTarGz(archivePath, binDir);
    }

    // The archive may contain the binary directly or inside a directory.
    // Find the mag binary and move it to bin/ if needed.
    var binaryName = isWindows ? "mag.exe" : "mag";
    var expectedPath = path.join(binDir, binaryName);

    if (!fs.existsSync(expectedPath)) {
      // Check if extracted into a subdirectory
      var entries = fs.readdirSync(binDir);
      for (var i = 0; i < entries.length; i++) {
        var candidateDir = path.join(binDir, entries[i]);
        var candidateBin = path.join(candidateDir, binaryName);
        if (
          fs.statSync(candidateDir).isDirectory() &&
          fs.existsSync(candidateBin)
        ) {
          fs.renameSync(candidateBin, expectedPath);
          // Clean up the extracted subdirectory
          fs.rmSync(candidateDir, { recursive: true, force: true });
          break;
        }
      }
    }

    if (!fs.existsSync(expectedPath)) {
      throw new Error(
        "Binary not found after extraction. Expected: " + expectedPath
      );
    }

    // Make executable on Unix
    if (!isWindows) {
      fs.chmodSync(expectedPath, 0o755);
    }

    console.log("Installed mag to " + expectedPath);
  } finally {
    // Clean up temp files
    try {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    } catch (_) {
      // best effort
    }
  }
}

main().catch(function (err) {
  console.error("Error installing mag binary:");
  console.error(err.message || err);
  console.error("");
  console.error("You can install manually:");
  console.error("  1. Download from https://github.com/" + REPO + "/releases");
  console.error("  2. Place the binary in " + path.join(__dirname, "bin", "mag"));
  console.error("  3. Or set MAG_BINARY_PATH=/path/to/mag before install");
  process.exit(1);
});
