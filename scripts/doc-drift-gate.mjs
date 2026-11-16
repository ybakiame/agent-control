// Issue #1 spike: minimal consumer of a doc-drift report, simulating an index pipeline's gate.
// Usage: node scripts/doc-drift-gate.mjs <report.json> [--threshold 0.2]
// Emits a machine-readable gating plan: each claim anchor becomes a chunk; stale/broken claims
// exclude their chunk, anchor drift downweights it, and a doc above the rot threshold is
// downweighted wholesale. No human interpretation needed.

import { readFileSync } from "node:fs";

const args = process.argv.slice(2);
const reportPath = args[0];
if (!reportPath) {
  console.error("usage: node scripts/doc-drift-gate.mjs <report.json> [--threshold 0.2]");
  process.exit(2);
}
const thresholdIdx = args.indexOf("--threshold");
const threshold = thresholdIdx >= 0 ? Number(args[thresholdIdx + 1]) : 0.2;

const report = JSON.parse(readFileSync(reportPath, "utf8"));
if (report.schema !== "doc-drift-report/1") {
  console.error(`unsupported report schema: ${report.schema}`);
  process.exit(2);
}

const plan = { source: reportPath, evaluated_at: report.evaluated_at, documents: [] };
for (const doc of report.documents) {
  const docAction = doc.rot_score > threshold ? "downweight" : "keep";
  const chunks = doc.claims.map((claim) => {
    const reasons = [];
    let action = "keep";
    if (claim.status === "stale" || claim.status === "broken") {
      action = "exclude";
      reasons.push(`${claim.id}:${claim.status}`);
    } else if (claim.anchor_ok === false) {
      action = "downweight";
      reasons.push(`${claim.id}:anchor_drift`);
    }
    if (docAction === "downweight" && action === "keep") {
      action = "downweight";
      reasons.push(`doc_rot:${doc.rot_score.toFixed(3)}`);
    }
    return { lines: claim.anchor ?? null, action, reasons };
  });
  plan.documents.push({
    doc: doc.doc,
    doc_exists: doc.doc_exists,
    rot_score: doc.rot_score,
    action: docAction,
    chunks,
  });
}

console.log(JSON.stringify(plan, null, 2));
const excluded = plan.documents.flatMap((d) => d.chunks).filter((c) => c.action === "exclude").length;
const downweighted = plan.documents.flatMap((d) => d.chunks).filter((c) => c.action === "downweight").length;
console.error(`gate: ${plan.documents.length} docs, ${excluded} chunks excluded, ${downweighted} downweighted`);