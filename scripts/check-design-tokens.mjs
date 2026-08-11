import { readFileSync, readdirSync } from "node:fs";
import { extname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const workspace = resolve(fileURLToPath(new URL("..", import.meta.url)));
const sourceDirectory = join(workspace, "src");
const tokenFile = join(sourceDirectory, "design-tokens.css");
const violations = [];

function visit(directory) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      visit(path);
    } else if (extname(entry.name) === ".css" && path !== tokenFile) {
      inspectCss(path);
    } else if ([".ts", ".tsx"].includes(extname(entry.name))) {
      inspectTypeScript(path);
    }
  }
}

function inspectCss(path) {
  const lines = readFileSync(path, "utf8").split(/\r?\n/u);
  const checks = [
    { label: "literal color", pattern: /#[0-9a-f]{3,8}\b|\brgba?\(/iu },
    { label: "literal font size", pattern: /font-size\s*:\s*-?(?:\d*\.)?\d+(?:px|rem|em|pt)\b/iu },
    { label: "literal font weight", pattern: /font-weight\s*:\s*\d+\b/iu },
    { label: "literal font shorthand", pattern: /\bfont\s*:\s*(?!inherit\b)[^;]*(?:\d+(?:px|rem|em|pt)|\b(?:Segoe UI|sans-serif)\b)/iu },
    { label: "literal spacing", pattern: /(?:gap|row-gap|column-gap|padding(?:-(?:top|right|bottom|left))?|margin(?:-(?:top|right|bottom|left))?)\s*:[^;}]*\b\d+px\b/iu },
    { label: "literal corner radius", pattern: /border-radius\s*:\s*\d+px\b/iu },
    { label: "literal elevation", pattern: /box-shadow\s*:(?!\s*var\()/iu },
  ];

  lines.forEach((line, index) => {
    for (const check of checks) {
      if (check.pattern.test(line)) {
        violations.push(`${relative(workspace, path)}:${index + 1}: ${check.label}: ${line.trim()}`);
      }
    }
  });
}

function inspectTypeScript(path) {
  const lines = readFileSync(path, "utf8").split(/\r?\n/u);
  const checks = [
    { label: "inline literal color", pattern: /(?:color|backgroundColor|borderColor|outlineColor|fill|stroke)\s*:\s*["'`](?:#[0-9a-f]{3,8}\b|rgba?\()/iu },
    { label: "inline literal font size", pattern: /fontSize\s*:\s*(?:["'`]?-?(?:\d*\.)?\d+(?:px|rem|em|pt)?["'`]?)/iu },
    { label: "inline literal font weight", pattern: /fontWeight\s*:\s*(?:["'`]?\d+["'`]?)/iu },
  ];

  lines.forEach((line, index) => {
    for (const check of checks) {
      if (check.pattern.test(line)) {
        violations.push(`${relative(workspace, path)}:${index + 1}: ${check.label}: ${line.trim()}`);
      }
    }
  });
}

visit(sourceDirectory);

if (violations.length) {
  console.error("Design token check failed. Move visual constants to src/design-tokens.css and use semantic variables:\n");
  console.error(violations.join("\n"));
  process.exit(1);
}

console.log("Design token check passed.");
