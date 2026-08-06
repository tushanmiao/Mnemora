import fs from "node:fs";

const [reportPath, budgetsPath] = process.argv.slice(2);
if (!reportPath || !budgetsPath) {
  console.error("Usage: node scripts/memory/evaluate-isolation.mjs <report.json> <budgets.json>");
  process.exit(2);
}

const report = JSON.parse(fs.readFileSync(reportPath, "utf8"));
const budgets = JSON.parse(fs.readFileSync(budgetsPath, "utf8"));
const samples = Array.isArray(report.samples) ? report.samples : [];
const baseline = samples.find((sample) => sample?.scene === "idle")?.process?.processes ?? [];
const residual = samples.filter((sample) => sample?.scene === "postHeavyResidual");
const threshold = budgets.release?.postHeavyResidual;
if (!baseline.length || !residual.length || typeof threshold !== "number") {
  console.error("Need idle and postHeavyResidual process samples plus a postHeavyResidual budget.");
  process.exit(2);
}

const baselineByRole = new Map();
for (const process of baseline) {
  const value = process?.privateBytes;
  if (typeof value !== "number") continue;
  baselineByRole.set(process.role, (baselineByRole.get(process.role) ?? 0) + value);
}

let peakDelta = 0;
for (const sample of residual) {
  const currentByRole = new Map();
  for (const process of sample.process?.processes ?? []) {
    const value = process?.privateBytes;
    if (typeof value !== "number") continue;
    currentByRole.set(process.role, (currentByRole.get(process.role) ?? 0) + value);
  }
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
