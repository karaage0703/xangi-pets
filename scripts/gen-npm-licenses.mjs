import fs from 'node:fs';
import path from 'node:path';

const rootDir = path.resolve(import.meta.dirname, '..');
const outputPath = path.join(rootDir, 'src-tauri', 'THIRD_PARTY_NPM_LICENSES.html');
const rootPackage = JSON.parse(fs.readFileSync(path.join(rootDir, 'package.json'), 'utf8'));
const packages = new Map();

function findPackageDir(name, fromDir) {
  let current = fromDir;
  while (true) {
    const candidate = path.join(current, 'node_modules', name);
    if (fs.existsSync(path.join(candidate, 'package.json'))) return candidate;
    const parent = path.dirname(current);
    if (parent === current) break;
    current = parent;
  }
  throw new Error(`Installed package not found: ${name}`);
}

function collectPackage(name, fromDir) {
  const packageDir = findPackageDir(name, fromDir);
  const manifest = JSON.parse(fs.readFileSync(path.join(packageDir, 'package.json'), 'utf8'));
  const key = `${manifest.name}@${manifest.version}`;
  if (packages.has(key)) return;

  const licenseFiles = fs.readdirSync(packageDir)
    .filter((file) => /^(licen[cs]e|copying)/i.test(file))
    .sort();
  if (licenseFiles.length === 0) throw new Error(`License file not found: ${key}`);

  packages.set(key, {
    name: manifest.name,
    version: manifest.version,
    licenses: licenseFiles.map((file) => ({
      file,
      text: fs.readFileSync(path.join(packageDir, file), 'utf8').trim(),
    })),
  });

  for (const dependency of Object.keys(manifest.dependencies ?? {}).sort()) {
    collectPackage(dependency, packageDir);
  }
}

for (const dependency of Object.keys(rootPackage.dependencies ?? {}).sort()) {
  collectPackage(dependency, rootDir);
}

const escapeHtml = (value) => value
  .replaceAll('&', '&amp;')
  .replaceAll('<', '&lt;')
  .replaceAll('>', '&gt;');

const sections = [...packages.values()]
  .sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`))
  .map((entry) => {
    const licenses = entry.licenses
      .map(({ file, text }) => `<h3>${escapeHtml(file)}</h3>\n<pre>${escapeHtml(text)}</pre>`)
      .join('\n');
    return `<section>\n<h2>${escapeHtml(entry.name)} ${escapeHtml(entry.version)}</h2>\n${licenses}\n</section>`;
  })
  .join('\n');

const html = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Third-party npm licenses</title>
<style>body{font-family:system-ui,sans-serif;max-width:960px;margin:2rem auto;padding:0 1rem}pre{white-space:pre-wrap;border:1px solid #ddd;padding:1rem}</style>
</head>
<body>
<h1>Third-party npm licenses</h1>
${sections}
</body>
</html>
`;

fs.writeFileSync(outputPath, html);
console.log(`Generated ${path.relative(rootDir, outputPath)} for ${packages.size} packages`);
