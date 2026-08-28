import { Construction, DatabaseZap, Search, Settings, ShieldCheck } from "lucide-react";

const content = {
  explorer: [Search, "Telemetry explorer", "Filter events, metric points and structured logs from one workspace."],
  migration: [DatabaseZap, "Migration center", "Import a validated Cloudflare D1 export into an application environment."],
  access: [ShieldCheck, "Members & access", "Compose custom permissions and bind them globally or per application."],
  settings: [Settings, "System settings", "Manage locale, retention, notification channels and runtime health."]
} as const;

export function PlaceholderPage({ kind }: { kind: keyof typeof content }) {
  const [Icon, title, description] = content[kind];
  return <div className="page enter-page"><header className="page-header"><div><span className="eyebrow">SONDE / {kind.toUpperCase()}</span><h1>{title}</h1><p>{description}</p></div></header><section className="workbench"><Icon aria-hidden="true" /><div><span><Construction size={15} aria-hidden="true" /> Module scaffolded</span><h2>Interface contract ready</h2><p>This module is wired into navigation and the persisted schema. Its detailed workflow follows the same API and permission boundaries as the live modules.</p></div></section></div>;
}

