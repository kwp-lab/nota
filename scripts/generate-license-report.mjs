import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const cargo = JSON.parse(
  execFileSync("cargo", ["metadata", "--format-version", "1", "--locked"], {
    cwd: resolve(root, "src-tauri"),
    encoding: "utf8",
    maxBuffer: 32 * 1024 * 1024,
  }),
);
const npmLock = JSON.parse(readFileSync(resolve(root, "package-lock.json"), "utf8"));

const rust = cargo.packages
  .filter((item) => item.source)
  .map((item) => ({
    name: item.name,
    version: item.version,
    license: item.license ?? "未声明",
  }))
  .sort((a, b) => a.name.localeCompare(b.name));

const npm = Object.entries(npmLock.packages ?? {})
  .filter(([path]) => path.startsWith("node_modules/"))
  .map(([path, item]) => ({
    name: item.name ?? path.slice("node_modules/".length),
    version: item.version ?? "",
    license: item.license ?? "未声明",
  }))
  .sort((a, b) => a.name.localeCompare(b.name));

const rows = (items) =>
  items.map((item) => `| ${item.name.replaceAll("|", "\\|")} | ${item.version} | ${item.license.replaceAll("|", "\\|")} |`).join("\n");

const output = `# 第三方依赖许可证清单

本文件由 \`scripts/generate-license-report.mjs\` 根据锁文件和 Cargo 元数据生成。它是依赖清单，不替代各依赖包内附带的完整许可证文本。

## Rust

| 包 | 版本 | SPDX / 声明 |
|---|---:|---|
${rows(rust)}

## npm

| 包 | 版本 | SPDX / 声明 |
|---|---:|---|
${rows(npm)}
`;

writeFileSync(resolve(root, "THIRD_PARTY_LICENSES.md"), output, "utf8");
