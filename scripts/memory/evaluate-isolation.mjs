import fs from "node:fs";

const [reportPath, budgetsPath] = process.argv.slice(2);
if (!reportPath || !budgetsPath) {
  console.error("Usage: node scripts/memory/evaluate-isolation.mjs <report.json> <budgets.json>");
  process.exit(2);
}

const readJson = (path) => JSON.parse(fs.readFileSync(path, "utf8").replace(/^\uFEFF/, ""));
const report = readJson(reportPath);
const budgets = readJson(budgetsPath);
const samples = Array.isArray(report.samples) ? report.samples : [];
const baseline = samples.filter((sample) => sample?.scene === "idle");
const residual = samples.filter((sample) => sample?.scene === "postHeavyResidual");
const threshold = budgets.release?.postHeavyResidual;
if (!baseline.length || !residual.length || typeof threshold !== "number") {
  console.error("Need idle and postHeavyResidual process samples plus a postHeavyResidual budget.");
  process.exit(2);
}

const baselineByRole = new Map();
for (const sample of baseline) {
  const currentByRole = sumByRole(sample?.process?.processes ?? []);
  for (const [role, value] of currentByRole) {
    baselineByRole.set(role, Math.max(baselineByRole.get(role) ?? 0, value));
  }
}

let peakDelta = 0;
for (const sample of residual) {
  const currentByRole = sumByRole(sample.process?.processes ?? []);
  let delta = 0;
  for (const [role, current] of currentByRole) {
    delta += Math.max(0, current - (baselineByRole.get(role) ?? 0));
  }
  peakDelta = Math.max(peakDelta, delta);
}

const pass = peakDelta <= threshold;
console.log(`postHeavyResidualDeltaBytes\t${peakDelta}`);
console.log(`budgetBytes\t${threshold}`);
console.log(`result\t${pass ? "PASS" : "REVIEW"}`);
process.exit(pass ? 0 : 1);

function sumByRole(processes) {
  const values = new Map();
  for (const process of processes) {
    if (typeof process?.privateBytes !== "number") continue;
    values.set(process.role, (values.get(process.role) ?? 0) + process.privateBytes);
  }
  return values;
}
