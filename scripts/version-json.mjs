import fs from "node:fs";

const [command, path, version] = process.argv.slice(2);

if (command === "read-lock") {
  const lock = JSON.parse(fs.readFileSync(path, "utf8"));
  process.stdout.write(
    JSON.stringify({
      documentVersion: lock.version,
      rootPackageVersion: lock.packages?.[""]?.version,
    }),
  );
} else if (command === "set") {
  if (!version) {
    throw new Error("The set command requires a version.");
  }
  const source = fs.readFileSync(path, "utf8");
  const versionPattern = /("version"\s*:\s*")[^"]+(")/;
  if (!versionPattern.test(source)) {
    throw new Error(`Could not find a version field in ${path}.`);
  }
  fs.writeFileSync(
    path,
    source.replace(versionPattern, `$1${version}$2`),
    "utf8",
  );
} else if (command === "set-lock") {
  if (!version) {
    throw new Error("The set-lock command requires a version.");
  }
  const source = fs.readFileSync(path, "utf8");
  const documentVersionPattern = /("version"\s*:\s*")[^"]+(")/;
  const rootPackageVersionPattern =
    /("packages"\s*:\s*\{\s*""\s*:\s*\{[\s\S]*?"version"\s*:\s*")[^"]+(")/;
  if (
    !documentVersionPattern.test(source) ||
    !rootPackageVersionPattern.test(source)
  ) {
    throw new Error(`Could not find both lockfile version fields in ${path}.`);
  }
  const updated = source
    .replace(documentVersionPattern, `$1${version}$2`)
    .replace(rootPackageVersionPattern, `$1${version}$2`);
  fs.writeFileSync(path, updated, "utf8");
} else if (command === "set-readme") {
  if (!version) {
    throw new Error("The set-readme command requires a version.");
  }
  const source = fs.readFileSync(path, "utf8");
  const badgePattern = /(badge\/version-).*?(-56615D\?style)/;
  if (!badgePattern.test(source)) {
    throw new Error(`Could not find the version badge in ${path}.`);
  }
  const updated = source
    .replace(badgePattern, `$1${version}$2`)
    .replace(/(alt="Version: )[^"]+(")/, `$1${version}$2`)
    .replace(/(alt="版本：)[^"]+(")/, `$1${version}$2`);
  fs.writeFileSync(path, updated, "utf8");
} else {
  throw new Error(`Unknown version JSON command: ${command ?? "(missing)"}`);
}
