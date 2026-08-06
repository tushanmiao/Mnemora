import fs from "node:fs";

const [reportPath, budgetsPath] = process.argv.slice(2);
if (!reportPath || !budgetsPath) {
  console.error("Usage: node scripts/memory/compare-memory-report.mjs <report.json> <budgets.json>");
  process.exit(2);
}

const report = JSON.parse(fs.readFileSync(reportPath, "utf8"));
const budgets = JSON.parse(fs.readFileSync(budgetsPath, "utf8"));
const samples = Array.isArray(report.samples) ? report.samples : [];
const release = budgets.release ?? {};
const byScene = new Map();
for (const sample of samples) {
  const privateBytes = sample?.process?.totalPrivateBytes;
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
