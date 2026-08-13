import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const CARGO_ABOUT_VERSION = "0.9.1";
const LICENSE_CHECKER_VERSION = "5.0.1";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const checkOnly = process.argv.includes("--check");

const outputPaths = {
  inventory: resolve(root, "THIRD_PARTY_LICENSES.md"),
  notices: resolve(root, "THIRD_PARTY_NOTICES.txt"),
  sources: resolve(root, "THIRD_PARTY_SOURCES.md"),
  sbom: resolve(root, "bom.cyclonedx.json"),
};

function run(command, args, options = {}) {
  return execFileSync(command, args, {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    ...options,
  }).trim();
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function assertToolVersions() {
  const cargoAbout = run("cargo", ["about", "--version"]);
  if (!cargoAbout.includes(`cargo-about ${CARGO_ABOUT_VERSION}`)) {
    throw new Error(
      `cargo-about ${CARGO_ABOUT_VERSION} is required (found: ${cargoAbout}). ` +
        `Install it with: cargo install cargo-about --locked --version ${CARGO_ABOUT_VERSION} --features cli`,
    );
  }

  const checkerPackage = JSON.parse(
    readFileSync(resolve(root, "node_modules/license-checker-rseidelsohn/package.json"), "utf8"),
  );
  if (checkerPackage.version !== LICENSE_CHECKER_VERSION) {
    throw new Error(
      `license-checker-rseidelsohn ${LICENSE_CHECKER_VERSION} is required (found: ${checkerPackage.version}).`,
    );
  }
}

function loadCargoMetadata() {
  return JSON.parse(
    run("cargo", [
      "metadata",
      "--format-version",
      "1",
      "--locked",
      "--filter-platform",
      "x86_64-pc-windows-msvc",
      "--manifest-path",
      resolve(root, "src-tauri/Cargo.toml"),
    ]),
  );
}

function loadNpmLicenses() {
  const bin = resolve(
    root,
    "node_modules/license-checker-rseidelsohn/bin/license-checker-rseidelsohn.js",
  );
  const result = JSON.parse(
    run(process.execPath, [bin, "--production", "--json", "--relativeLicensePath"]),
  );
  return Object.entries(result)
    .filter(([key]) => !key.startsWith("nota@"))
    .map(([key, value]) => ({ key, ...value }))
    .sort((a, b) => a.key.localeCompare(b.key));
}

function assertOfficialNpmRegistry() {
  const lock = JSON.parse(readFileSync(resolve(root, "package-lock.json"), "utf8"));
  const invalid = Object.entries(lock.packages ?? {})
    .filter(([, item]) => item.resolved)
    .filter(([, item]) => {
      const url = new URL(item.resolved);
      return url.protocol !== "https:" || url.hostname !== "registry.npmjs.org";
    })
    .map(([path, item]) => `${path}: ${item.resolved}`);
  if (invalid.length > 0) {
    throw new Error(
      `package-lock.json contains non-official npm download URLs:\n${invalid.join("\n")}`,
    );
  }
}

function resolvedCargoPackages(cargo) {
  const ids = new Set(cargo.resolve?.nodes.map((node) => node.id) ?? []);
  return cargo.packages.filter((item) => ids.has(item.id));
}

const allowedNpmLicenses = new Set([
  "0BSD",
  "Apache-2.0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "CC0-1.0",
  "ISC",
  "MIT",
  "MIT-0",
  "MPL-2.0",
  "Unlicense",
  "Unicode-3.0",
  "Zlib",
]);

function assertNpmPolicy(packages) {
  const prohibited = /\b(?:AGPL|SSPL|BUSL|GPL|LGPL)\b/i;
  for (const item of packages) {
    const expression = String(item.licenses ?? "UNKNOWN");
    if (prohibited.test(expression)) {
      throw new Error(`Prohibited npm license for ${item.key}: ${expression}`);
    }
    const identifiers = expression.match(/[A-Za-z0-9.-]+/g) ?? [];
    const unknown = identifiers.filter(
      (token) => !["AND", "OR", "WITH"].includes(token) && !allowedNpmLicenses.has(token),
    );
    if (identifiers.length === 0 || unknown.length > 0) {
      throw new Error(`Unreviewed npm license for ${item.key}: ${expression}`);
    }
    if (!item.licenseFile) {
      throw new Error(`No license file was found for production npm package ${item.key}.`);
    }
  }
}

function verifyManualComponents(cargo) {
  const manual = JSON.parse(
    readFileSync(resolve(root, "scripts/licenses/manual-components.json"), "utf8"),
  );
  return manual.map((item) => {
    const parent = cargo.packages.find(
      (pkg) => pkg.name === item.parentCrate && pkg.version === item.parentVersion,
    );
    if (!parent) {
      throw new Error(`Manual component parent not found: ${item.parentCrate} ${item.parentVersion}`);
    }
    const licenseFile = resolve(dirname(parent.manifest_path), item.licensePath);
    const text = readFileSync(licenseFile, "utf8").replaceAll("\r\n", "\n").trimEnd();
    const actual = sha256(Buffer.from(text + "\n", "utf8"));
    const rawActual = sha256(readFileSync(licenseFile));
    if (rawActual !== item.licenseSha256 && actual !== item.licenseSha256) {
      throw new Error(
        `Manual license checksum changed for ${item.name}: expected ${item.licenseSha256}, found ${rawActual}`,
      );
    }
    return { ...item, text };
  });
}

function generateRustNotices(tempDirectory) {
  const output = resolve(tempDirectory, "rust-notices.txt");
  run(
    "cargo",
    [
      "about",
      "generate",
      "--config",
      resolve(root, "src-tauri/about.toml"),
      "--output-file",
      output,
      resolve(root, "scripts/licenses/about.hbs"),
    ],
    { cwd: resolve(root, "src-tauri") },
  );
  return readFileSync(output, "utf8").replaceAll("\r\n", "\n").trimEnd();
}

function generateRustNoticeFiles(cargoPackages) {
  const groups = new Map();
  for (const item of cargoPackages.filter((pkg) => pkg.source)) {
    const packageDirectory = dirname(item.manifest_path);
    let entries;
    try {
      entries = readdirSync(packageDirectory, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      if (!entry.isFile() || !/^(NOTICE|COPYRIGHT)(\..*)?$/i.test(entry.name)) {
        continue;
      }
      const text = readFileSync(resolve(packageDirectory, entry.name), "utf8")
        .replaceAll("\r\n", "\n")
        .trimEnd();
      const key = sha256(text);
      const group = groups.get(key) ?? { text, packages: [] };
      group.packages.push(`${item.name}@${item.version} (${entry.name})`);
      groups.set(key, group);
    }
  }
  const sections = [...groups.values()]
    .sort((a, b) => a.packages[0].localeCompare(b.packages[0]))
    .map(
      (group) =>
        `${"-".repeat(79)}\nUsed by:\n${group.packages.map((name) => `- ${name}`).join("\n")}\n\n${group.text}`,
    );
  return sections.length
    ? `ADDITIONAL RUST UPSTREAM NOTICE FILES\n${"=".repeat(37)}\n\n${sections.join("\n\n")}`
    : "";
}

function generateNpmNotices(packages) {
  const groups = new Map();
  for (const item of packages) {
    const path = resolve(root, item.licenseFile);
    const text = readFileSync(path, "utf8").replaceAll("\r\n", "\n").trimEnd();
    const key = sha256(text);
    const group = groups.get(key) ?? { text, packages: [] };
    group.packages.push(`${item.key} (${item.licenses})`);
    groups.set(key, group);
  }

  const sections = [...groups.values()]
    .sort((a, b) => a.packages[0].localeCompare(b.packages[0]))
    .map(
      (group) =>
        `${"-".repeat(79)}\nUsed by:\n${group.packages.map((name) => `- ${name}`).join("\n")}\n\n${group.text}`,
    );
  return `NOTA NPM THIRD-PARTY NOTICES\n${"=".repeat(29)}\n\n${sections.join("\n\n")}`;
}

function inventoryMarkdown(cargoPackages, npmPackages, manual) {
  const rustRows = cargoPackages
    .filter((item) => item.source)
    .sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`))
    .map((item) => `| ${item.name} | ${item.version} | ${item.license ?? "UNDECLARED"} |`)
    .join("\n");
  const npmRows = npmPackages
    .map((item) => {
      const split = item.key.lastIndexOf("@");
      return `| ${item.key.slice(0, split)} | ${item.key.slice(split + 1)} | ${item.licenses} |`;
    })
    .join("\n");
  const manualRows = manual
    .map((item) => `| ${item.name} | ${item.version} | ${item.license} | ${item.parentCrate} ${item.parentVersion} |`)
    .join("\n");

  return `# Third-party dependency inventory

Generated from the locked Rust and production npm dependency graphs. The
redistributable license texts are in \`THIRD_PARTY_NOTICES.txt\`; source-offer
details for reciprocal licenses are in \`THIRD_PARTY_SOURCES.md\`.

## Rust crates

| Package | Version | SPDX / declaration |
|---|---:|---|
${rustRows}

## Production npm packages

| Package | Version | SPDX / declaration |
|---|---:|---|
${npmRows}

## Manually audited embedded components

| Component | Version | License | Included by |
|---|---:|---|---|
${manualRows}
`;
}

function sourceMarkdown(cargoPackages, npmPackages, manual) {
  const mplCargo = cargoPackages
    .filter((item) => item.source && item.license?.includes("MPL-2.0"))
    .sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`));
  const mplNpm = npmPackages.filter((item) => String(item.licenses).includes("MPL-2.0"));
  const cargoRows = mplCargo
    .map(
      (item) =>
        `| ${item.name} | ${item.version} | ${item.repository ?? `https://crates.io/crates/${item.name}/${item.version}`} |`,
    )
    .join("\n");
  const npmRows = mplNpm
    .map((item) => `| ${item.key} | ${item.repository ?? "See the npm package metadata"} |`)
    .join("\n");
  const manualRows = manual.map((item) => `| ${item.name} | ${item.source} | ${item.reason} |`).join("\n");

  return `# Third-party source availability

Nota's own source is MIT licensed. Some unmodified dependencies use reciprocal
licenses. The release workflow publishes an exact source archive for the
MPL-2.0 Rust crates shipped in the Windows binary. That archive corresponds to
the versions in \`src-tauri/Cargo.lock\` and is named
\`Nota-<version>-mpl-sources.zip\`.

## MPL-2.0 Rust crates included in the Windows release

| Crate | Version | Upstream source |
|---|---:|---|
${cargoRows || "| None | - | - |"}

## MPL-2.0 production npm packages

| Package | Upstream source |
|---|---|
${npmRows || "| None | - |"}

## Manually audited embedded native code

| Component | Exact source | Why it is tracked manually |
|---|---|---|
${manualRows}

The source archive is provided for license compliance and debugging; Nota does
not claim ownership of third-party code. If a dependency version or vendored
native snapshot changes, regenerate and review all compliance artifacts.
`;
}

function component(name, version, license, purl, externalReference) {
  const value = {
    type: "library",
    "bom-ref": purl,
    name,
    version,
    licenses: [{ expression: license ?? "NOASSERTION" }],
    purl,
  };
  if (externalReference) {
    value.externalReferences = [{ type: "vcs", url: externalReference }];
  }
  return value;
}

function sbomJson(cargoPackages, npmPackages, manual) {
  const components = [
    ...cargoPackages
      .filter((item) => item.source)
      .map((item) =>
        component(
          item.name,
          item.version,
          item.license,
          `pkg:cargo/${encodeURIComponent(item.name)}@${item.version}`,
          item.repository,
        ),
      ),
    ...npmPackages.map((item) => {
      const split = item.key.lastIndexOf("@");
      const name = item.key.slice(0, split);
      const version = item.key.slice(split + 1);
      return component(
        name,
        version,
        item.licenses,
        `pkg:npm/${name.replace("/", "%2F")}@${version}`,
        item.repository,
      );
    }),
    ...manual.map((item) =>
      component(
        item.name,
        item.version,
        item.license,
        `pkg:generic/${item.name}@${encodeURIComponent(item.version)}`,
        item.source,
      ),
    ),
  ].sort((a, b) => a["bom-ref"].localeCompare(b["bom-ref"]));

  const lockDigest = sha256(
    readFileSync(resolve(root, "src-tauri/Cargo.lock")) + readFileSync(resolve(root, "package-lock.json")),
  );
  const uuid = `${lockDigest.slice(0, 8)}-${lockDigest.slice(8, 12)}-4${lockDigest.slice(13, 16)}-a${lockDigest.slice(17, 20)}-${lockDigest.slice(20, 32)}`;
  return `${JSON.stringify(
    {
      bomFormat: "CycloneDX",
      specVersion: "1.6",
      serialNumber: `urn:uuid:${uuid}`,
      version: 1,
      metadata: {
        component: {
          type: "application",
          "bom-ref": "pkg:github/kwp-lab/nota",
          name: "Nota",
          version: JSON.parse(readFileSync(resolve(root, "package.json"), "utf8")).version,
          licenses: [{ license: { id: "MIT" } }],
        },
        tools: { components: [
          { type: "application", name: "cargo-about", version: CARGO_ABOUT_VERSION },
          { type: "application", name: "license-checker-rseidelsohn", version: LICENSE_CHECKER_VERSION },
        ] },
      },
      components,
    },
    null,
    2,
  )}\n`;
}

function writeOrCheck(path, value) {
  const normalized = value.replaceAll("\r\n", "\n").trimEnd() + "\n";
  if (checkOnly) {
    const current = readFileSync(path, "utf8").replaceAll("\r\n", "\n");
    if (current !== normalized) {
      throw new Error(`${path} is stale. Run npm run licenses:generate.`);
    }
    return;
  }
  writeFileSync(path, normalized, "utf8");
}

assertToolVersions();
assertOfficialNpmRegistry();
const cargo = loadCargoMetadata();
const cargoPackages = resolvedCargoPackages(cargo);
const npmPackages = loadNpmLicenses();
assertNpmPolicy(npmPackages);
const manual = verifyManualComponents({ packages: cargoPackages });
const tempDirectory = mkdtempSync(resolve(tmpdir(), "nota-licenses-"));

try {
  const rustNotices = generateRustNotices(tempDirectory);
  const rustNoticeFiles = generateRustNoticeFiles(cargoPackages);
  const npmNotices = generateNpmNotices(npmPackages);
  const manualNotices = manual
    .map(
      (item) =>
        `${"-".repeat(79)}\n${item.name} ${item.version} (${item.license})\nSource: ${item.source}\nIncluded by: ${item.parentCrate} ${item.parentVersion}\n\n${item.text}`,
    )
    .join("\n\n");

  writeOrCheck(outputPaths.inventory, inventoryMarkdown(cargoPackages, npmPackages, manual));
  writeOrCheck(
    outputPaths.notices,
    `${rustNotices}\n\n${rustNoticeFiles}\n\n${npmNotices}\n\nNOTA MANUALLY AUDITED EMBEDDED COMPONENTS\n${"=".repeat(43)}\n\n${manualNotices}`,
  );
  writeOrCheck(outputPaths.sources, sourceMarkdown(cargoPackages, npmPackages, manual));
  writeOrCheck(outputPaths.sbom, sbomJson(cargoPackages, npmPackages, manual));
} finally {
  rmSync(tempDirectory, { recursive: true, force: true });
}

console.log(checkOnly ? "License artifacts are current." : "License artifacts generated.");
