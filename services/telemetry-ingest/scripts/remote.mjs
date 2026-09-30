// Deploy-time checks against the live account, run by the deploy workflow with
// its Cloudflare credentials:
//   node scripts/remote.mjs account   the account holds this config's D1 database
//   node scripts/remote.mjs state     the kill switch, as Markdown for the job summary
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { parse } from "jsonc-parser";

const config = parse(readFileSync(new URL("../wrangler.jsonc", import.meta.url), "utf8"));
const db = config.d1_databases[0];
const wrangler = (...args) =>
  JSON.parse(
    execFileSync("pnpm", ["exec", "wrangler", ...args], { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] }),
  );

const mode = process.argv[2];
if (mode === "account") {
  const found = wrangler("d1", "list", "--json").find((d) => d.uuid === db.database_id);
  if (!found || found.name !== db.database_name) {
    console.error(
      `The account behind CLOUDFLARE_ACCOUNT_ID has no D1 database ${db.database_name} (${db.database_id}). ` +
        "Check the secret points at the account wrangler.jsonc was written for.",
    );
    process.exit(1);
  }
  console.log(`account holds ${db.database_name} (${db.database_id})`);
} else if (mode === "state") {
  const [result] = wrangler(
    "d1",
    "execute",
    db.database_name,
    "--remote",
    "--json",
    "--command",
    "SELECT value FROM control WHERE key = 'ingest_enabled'",
  );
  const enabled = result?.results?.[0]?.value === "1";
  console.log(enabled ? "Ingestion is **enabled**." : "⚠️ Ingestion is **disabled** (kill switch): clients get 503.");
} else {
  console.error("usage: remote.mjs account|state");
  process.exit(2);
}
