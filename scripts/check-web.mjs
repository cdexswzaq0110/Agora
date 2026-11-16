#!/usr/bin/env node
// Static check for the no-build frontend (stdlib only):
//   1. every web/js/*.js parses as an ES module, web/*.js as a classic script;
//   2. every named import `import { a, b as c } from './x.js'` exists in x.js's exports;
//   3. every web/js module is in the service worker's offline SHELL list.
// Why not `node --check web/js/x.js`: on Node 24 it exited 0 for a module with an unterminated
// string literal — see docs/lessons/0008-node-check-misses-esm-syntax-errors.md.
import { spawnSync } from 'node:child_process';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const web = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'web');
const jsDir = join(web, 'js');
const modules = readdirSync(jsDir).filter((f) => f.endsWith('.js')).sort();
const classic = readdirSync(web).filter((f) => f.endsWith('.js')).sort();
const problems = [];

function parse(file, type) {
  const r = spawnSync(process.execPath, ['--check', `--input-type=${type}`], { input: readFileSync(file, 'utf8'), encoding: 'utf8' });
  if (r.status !== 0) problems.push(`${file}: does not parse as ${type}\n${(r.stderr || '').split('\n').slice(0, 5).join('\n')}`);
}

function exportsOf(src) {
  const names = new Set();
  for (const m of src.matchAll(/export\s+(?:async\s+)?function\s*\*?\s*([A-Za-z_$][\w$]*)/g)) names.add(m[1]);
  for (const m of src.matchAll(/export\s+(?:const|let|var|class)\s+([A-Za-z_$][\w$]*)/g)) names.add(m[1]);
  for (const m of src.matchAll(/export\s*\{([^}]*)\}/g)) {
    for (const part of m[1].split(',')) {
      const name = part.trim().split(/\s+as\s+/).pop().trim();
      if (name) names.add(name);
    }
  }
  return names;
}

for (const f of modules) parse(join(jsDir, f), 'module');
for (const f of classic) parse(join(web, f), 'commonjs');

const exportMap = new Map(modules.map((f) => [f, exportsOf(readFileSync(join(jsDir, f), 'utf8'))]));
for (const f of modules) {
  const src = readFileSync(join(jsDir, f), 'utf8');
  for (const m of src.matchAll(/import\s*\{([^}]*)\}\s*from\s*['"]\.\/([\w.-]+)['"]/g)) {
    const target = m[2];
    const have = exportMap.get(target);
    if (!have) { problems.push(`${f}: imports from missing module ./${target}`); continue; }
    for (const part of m[1].split(',')) {
      const name = part.trim().split(/\s+as\s+/)[0].trim();
      if (name && !have.has(name)) problems.push(`${f}: '${name}' is not exported by ${target}`);
    }
  }
}

const sw = readFileSync(join(web, 'sw.js'), 'utf8');
for (const f of modules) if (!sw.includes(`'/js/${f}'`)) problems.push(`sw.js: /js/${f} missing from SHELL (offline start would fail)`);

if (problems.length) {
  console.error(problems.join('\n\n'));
  console.error(`\n${problems.length} problem(s).`);
  process.exit(1);
}
console.log(`ok: ${modules.length} modules + ${classic.length} classic scripts parse; all named imports resolve; sw.js caches every module.`);
