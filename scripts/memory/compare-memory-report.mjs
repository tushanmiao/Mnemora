import fs from "node:fs";

const [reportPath, budgetsPath] = process.argv.slice(2);
if (!reportPath || !budgetsPath) {
  console.error("Usage: node scripts/memory/compare-memory-report.mjs <report.json> <budgets.json>");
  process.exit(2);
}

const readJson = (path) => JSON.parse(fs.readFileSync(path, "utf8").replace(/^\uFEFF/, ""));
const report = readJson(reportPath);
const budgets = readJson(budgetsPath);
const samples = Array.isArray(report.samples) ? report.samples : [];
const release = budgets.release ?? {};
const residualScene = "postHeavyResidual";
const idleSamples = samples.filter((sample) => sample?.scene === "idle");
const baselineByRole = peakByRole(idleSamples);
const byScene = new Map();
for (const sample of samples) {
  const privateBytes = sample?.scene === residualScene && baselineByRole.size
    ? positiveRoleDelta(sample, baselineByRole)
    : sample?.process?.totalPrivateBytes;
  if (typeof privateBytes !== "number") continue;
  const current = byScene.get(sample.scene) ?? { peak: 0, count: 0 };
  current.peak = Math.max(current.peak, privateBytes);
  current.count += 1;
  byScene.set(sample.scene, current);
}

let failed = false;
console.log("scene\tsamples\tpeakPrivateBytes\tbudgetBytes\tresult");
for (const [scene, value] of byScene) {
  const budget = release[scene];
  const pass = typeof budget !== "number" || value.peak <= budget;
  if (!pass) failed = true;
  console.log(`${scene}\t${value.count}\t${value.peak}\t${budget ?? "n/a"}\t${pass ? "PASS" : "FAIL"}`);
}
if (byScene.size === 0) {
  console.error("The report contains no process samples.");
  process.exit(2);
}
process.exit(failed ? 1 : 0);

function peakByRole(sceneSamples) {
  const peaks = new Map();
  for (const sample of sceneSamples) {
    const current = sumByRole(sample?.process?.processes ?? []);
    for (const [role, value] of current) {
      peaks.set(role, Math.max(peaks.get(role) ?? 0, value));
    }
  }
  return peaks;
}

function positiveRoleDelta(sample, baseline) {
  let delta = 0;
  for (const [role, current] of sumByRole(sample?.process?.processes ?? [])) {
    delta += Math.max(0, current - (baseline.get(role) ?? 0));
  }
  return delta;
}

function sumByRole(processes) {
  const values = new Map();
  for (const process of processes) {
    if (typeof process?.privateBytes !== "number") continue;
    values.set(process.role, (values.get(process.role) ?? 0) + process.privateBytes);
  }
  return values;
}
