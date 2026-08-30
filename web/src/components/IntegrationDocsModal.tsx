import { useMemo, useState } from "react";
import { Check, Copy, KeyRound, ShieldCheck } from "lucide-react";
import { Modal } from "./Modal";

export type IntegrationDocsModalProps = {
  isOpen: boolean;
  onClose: () => void;
  app: {
    name: string;
    slug: string;
  };
  apiKeyPrefix?: string;
};

type LanguageTab = "rust" | "typescript" | "go" | "python" | "csharp" | "cpp" | "curl";

const tabs: Array<{ id: LanguageTab; label: string }> = [
  { id: "rust", label: "Rust" },
  { id: "typescript", label: "TypeScript / JS" },
  { id: "go", label: "Go" },
  { id: "python", label: "Python" },
  { id: "csharp", label: "C# / .NET" },
  { id: "cpp", label: "C++" },
  { id: "curl", label: "cURL / shell" },
];

export function IntegrationDocsModal({
  isOpen,
  onClose,
  app,
  apiKeyPrefix,
}: IntegrationDocsModalProps) {
  const [activeTab, setActiveTab] = useState<LanguageTab>("rust");
  const [copied, setCopied] = useState(false);
  const origin = typeof window !== "undefined" ? window.location.origin : "https://telemetry.yourdomain.com";
  const apiKeyPlaceholder = apiKeyPrefix
    ? `${apiKeyPrefix}********************`
    : "sonde_key_xxxxxxxxxxxxxxxxxxxxxxxx";
  const codeSnippet = useMemo(
    () => buildSnippet(activeTab, origin, apiKeyPlaceholder),
    [activeTab, origin, apiKeyPlaceholder],
  );

  async function handleCopy() {
    await navigator.clipboard.writeText(codeSnippet);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1800);
  }

  return (
    <Modal isOpen={isOpen} onClose={onClose} title="Sonde 安全接入指南" size="xl">
      <div className="space-y-5">
        <div className="rounded-2xl border border-[var(--border-soft)] bg-[var(--input-bg)] p-4">
          <div className="flex items-start gap-3">
            <div className="mt-0.5 flex h-10 w-10 flex-shrink-0 items-center justify-center rounded-xl border border-[var(--signal)]/30 bg-[var(--signal-subtle)] text-[var(--signal)]">
              <ShieldCheck size={18} />
            </div>
            <div className="min-w-0">
              <h4 className="m-0 text-sm font-bold text-[var(--text)]">{app.name}</h4>
              <p className="m-0 mt-1 text-xs leading-relaxed text-[var(--muted)]">
                API Key 只用于换取短期设备 Token。正式 telemetry 必须使用 device-bound Token、timestamp、nonce 和 HMAC-SHA256 签名；平台从可信 telemetry 自行维护设备状态与历史。
              </p>
              <p className="m-0 mt-2 break-all font-mono text-[10px] text-[var(--faint)]">
                {origin}/api/v1/ingest · slug={app.slug}
              </p>
            </div>
          </div>
        </div>

        <div className="grid gap-3 md:grid-cols-4">
          <Step number="1" title="Bootstrap" text="API Key + 当前稳定 pseudonymous deviceId → /ingest/token" />
          <Step number="2" title="Short-lived token" text="保存 token、signingKey、expiresAt；到期后重新交换。" />
          <Step number="3" title="Canonical request" text="对原始 body 做 SHA-256，并绑定 timestamp / nonce / method / path。" />
          <Step number="4" title="Signed ingest" text="Authorization: Bearer sndt_… + HMAC headers → events / metrics / logs / errors" />
        </div>

        <div className="rounded-2xl border border-[var(--amber)]/30 bg-[var(--amber-subtle)] p-3.5 text-xs leading-relaxed text-[var(--text)]">
          <div className="flex items-center gap-2 font-bold">
            <KeyRound size={15} className="text-[var(--amber)]" />
            长期 API Key 不允许直接写入 telemetry
          </div>
          <p className="m-0 mt-1.5 text-[var(--muted)]">
            将 API Key 直接发送到 /events、/metrics、/logs 或 /errors 会返回
            <code className="mx-1 rounded bg-[var(--input-bg)] px-1.5 py-0.5">ingest_token_required</code>。
            客户端也不应把 Session、版本或 OS 历史放进 Token 请求；这些状态由平台从 telemetry 推导。
          </p>
        </div>

        <div className="overflow-x-auto border-b border-[var(--border-soft)] pb-1">
          <div className="flex min-w-max gap-1.5">
            {tabs.map((tab) => (
              <button
                key={tab.id}
                type="button"
                onClick={() => setActiveTab(tab.id)}
                className={`rounded-xl px-3 py-1.5 text-xs font-bold transition-colors ${
                  activeTab === tab.id
                    ? "bg-[var(--signal)] text-white"
                    : "text-[var(--muted)] hover:bg-[var(--input-bg)] hover:text-[var(--text)]"
                }`}
              >
                {tab.label}
              </button>
            ))}
          </div>
        </div>

        <div className="relative overflow-hidden rounded-2xl border border-[var(--border)] bg-[#0d1117] shadow-2xl">
          <div className="flex items-center justify-between border-b border-[#30363d] bg-[#161b22] px-4 py-2 text-xs">
            <span className="font-mono font-semibold uppercase text-gray-400">
              {activeTab} · sonde-hmac-sha256-v2
            </span>
            <button
              type="button"
              onClick={() => void handleCopy()}
              className="inline-flex items-center gap-1.5 rounded-lg bg-[var(--signal)] px-3 py-1 font-bold text-white transition-all hover:brightness-110 active:scale-95"
            >
              {copied ? <Check size={13} /> : <Copy size={13} />}
              {copied ? "已复制" : "复制代码"}
            </button>
          </div>
          <pre className="max-h-[480px] overflow-auto p-4 font-mono text-xs leading-relaxed text-gray-200">
            <code>{codeSnippet}</code>
          </pre>
        </div>

        <div className="rounded-2xl border border-[var(--border-soft)] bg-[var(--panel-strong)] p-4 text-xs leading-relaxed text-[var(--muted)]">
          <strong className="text-[var(--text)]">Canonical request</strong>
          <pre className="m-0 mt-2 overflow-x-auto rounded-xl bg-[var(--input-bg)] p-3 font-mono text-[11px] text-[var(--text)]">{`sonde-hmac-sha256-v2\n
timestampMillis\n
nonce\n
HTTP_METHOD_UPPERCASE\n
/api/v1/ingest/events\n
hex(sha256(rawBodyBytes))`}</pre>
          <p className="m-0 mt-2">
            签名为 <code>hex(HMAC-SHA256(signingKey, canonicalBytes))</code>。必须签名与实际发送完全相同的原始 body bytes；不要签名后再次格式化 JSON。
          </p>
        </div>
      </div>
    </Modal>
  );
}

function Step({ number, title, text }: { number: string; title: string; text: string }) {
  return (
    <div className="rounded-2xl border border-[var(--border-soft)] bg-[var(--panel)] p-3.5">
      <div className="mb-2 flex h-6 w-6 items-center justify-center rounded-lg bg-[var(--signal-subtle)] text-[10px] font-extrabold text-[var(--signal)]">
        {number}
      </div>
      <div className="text-xs font-bold text-[var(--text)]">{title}</div>
      <p className="m-0 mt-1 text-[11px] leading-relaxed text-[var(--muted)]">{text}</p>
    </div>
  );
}

function buildSnippet(lang: LanguageTab, origin: string, apiKey: string): string {
  switch (lang) {
    case "rust":
      return rustSnippet(origin, apiKey);
    case "typescript":
      return typescriptSnippet(origin, apiKey);
    case "python":
      return pythonSnippet(origin, apiKey);
    case "go":
      return protocolSnippet("//", origin, apiKey, "Go: crypto/hmac + crypto/sha256 + encoding/hex");
    case "csharp":
      return protocolSnippet("//", origin, apiKey, "C#: HMACSHA256 + SHA256.HashData + Convert.ToHexString(...).ToLowerInvariant()");
    case "cpp":
      return protocolSnippet("//", origin, apiKey, "C++: OpenSSL HMAC(EVP_sha256) + SHA256 + libcurl");
    case "curl":
      return shellSnippet(origin, apiKey);
  }
}

function protocolSnippet(comment: string, origin: string, apiKey: string, cryptoHint: string): string {
  return `${comment} ${cryptoHint}
${comment} 1) Bootstrap only: exchange API Key for a device token.
POST ${origin}/api/v1/ingest/token
Authorization: Bearer ${apiKey}
Content-Type: application/json

{"deviceId":"stable-pseudonymous-device-id"}

${comment} Response fields used by the SDK:
${comment} token, signingKey, expiresAt, signatureVersion

${comment} 2) Serialize telemetry exactly once to rawBodyBytes.
${comment} 3) timestamp = current Unix epoch milliseconds.
${comment} 4) nonce = cryptographically random 16+ byte printable identifier.
${comment} 5) bodyHash = lowercaseHex(SHA256(rawBodyBytes)).
${comment} 6) canonical =
${comment}    "sonde-hmac-sha256-v2\\n" +
${comment}    timestamp + "\\n" + nonce + "\\n" +
${comment}    "POST\\n/api/v1/ingest/events\\n" + bodyHash
${comment} 7) signature = lowercaseHex(HMAC-SHA256(signingKey, UTF8(canonical))).

POST ${origin}/api/v1/ingest/events
Authorization: Bearer <sndt_device_token>
x-sonde-timestamp: <timestamp>
x-sonde-nonce: <nonce>
x-sonde-signature: <signature>
Content-Type: application/json

{"items":[{"name":"app_startup","appVersion":"1.0.0","os":"windows","timestamp":<timestamp>}]}

${comment} Do not send the bootstrap API Key to telemetry endpoints.`;
}

function rustSnippet(origin: string, apiKey: string): string {
  return `// Cargo.toml: reqwest, serde, serde_json, sha2, hmac, hex, uuid, chrono
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenResponse {
    token: String,
    signing_key: String,
    expires_at: i64,
}

async fn exchange(client: &Client, device_id: &str) -> reqwest::Result<TokenResponse> {
    client.post("${origin}/api/v1/ingest/token")
        .bearer_auth("${apiKey}")
        .json(&serde_json::json!({ "deviceId": device_id }))
        .send().await?.error_for_status()?.json().await
}

fn sign(signing_key: &str, timestamp: i64, nonce: &str, path: &str, body: &[u8]) -> String {
    let body_hash = hex::encode(Sha256::digest(body));
    let canonical = format!(
        "sonde-hmac-sha256-v2\\n{}\\n{}\\nPOST\\n{}\\n{}",
        timestamp, nonce, path, body_hash,
    );
    let mut mac = HmacSha256::new_from_slice(signing_key.as_bytes()).unwrap();
    mac.update(canonical.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

async fn send_event(client: &Client, auth: &TokenResponse) -> reqwest::Result<()> {
    let timestamp = chrono::Utc::now().timestamp_millis();
    let nonce = uuid::Uuid::now_v7().to_string();
    let path = "/api/v1/ingest/events";
    let body = serde_json::to_vec(&serde_json::json!({
        "items": [{
            "name": "app_startup",
            "appVersion": "1.0.0",
            "os": std::env::consts::OS,
            "timestamp": timestamp
        }]
    })).unwrap();
    let signature = sign(&auth.signing_key, timestamp, &nonce, path, &body);

    client.post(format!("${origin}{}", path))
        .bearer_auth(&auth.token)
        .header("content-type", "application/json")
        .header("x-sonde-timestamp", timestamp.to_string())
        .header("x-sonde-nonce", nonce)
        .header("x-sonde-signature", signature)
        .body(body)
        .send().await?.error_for_status()?;
    Ok(())
}`;
}

function typescriptSnippet(origin: string, apiKey: string): string {
  return `const endpoint = "${origin}/api/v1/ingest";
const apiKey = "${apiKey}"; // bootstrap only
const deviceId = "stable-pseudonymous-device-id";
const encoder = new TextEncoder();

const tokenResponse = await fetch(endpoint + "/token", {
  method: "POST",
  headers: {
    authorization: "Bearer " + apiKey,
    "content-type": "application/json",
  },
  body: JSON.stringify({ deviceId }),
});
const auth = await tokenResponse.json();

function hex(bytes: ArrayBuffer) {
  return [...new Uint8Array(bytes)]
    .map((value) => value.toString(16).padStart(2, "0"))
    .join("");
}

async function sendEvent() {
  const timestamp = Date.now();
  const nonce = crypto.randomUUID();
  const path = "/events";
  const canonicalPath = "/api/v1/ingest/events";
  const rawBody = JSON.stringify({
    items: [{ name: "app_startup", appVersion: "1.0.0", timestamp }],
  });
  const bodyHash = hex(await crypto.subtle.digest("SHA-256", encoder.encode(rawBody)));
  const canonical =
    "sonde-hmac-sha256-v2\\n" + timestamp + "\\n" + nonce +
    "\\nPOST\\n" + canonicalPath + "\\n" + bodyHash;
  const key = await crypto.subtle.importKey(
    "raw", encoder.encode(auth.signingKey), { name: "HMAC", hash: "SHA-256" }, false, ["sign"],
  );
  const signature = hex(await crypto.subtle.sign("HMAC", key, encoder.encode(canonical)));

  await fetch(endpoint + path, {
    method: "POST",
    headers: {
      authorization: "Bearer " + auth.token,
      "content-type": "application/json",
      "x-sonde-timestamp": String(timestamp),
      "x-sonde-nonce": nonce,
      "x-sonde-signature": signature,
    },
    body: rawBody,
  });
}`;
}

function pythonSnippet(origin: string, apiKey: string): string {
  return `import hashlib
import hmac
import json
import time
import uuid
import requests

ENDPOINT = "${origin}/api/v1/ingest"
API_KEY = "${apiKey}"  # bootstrap only
DEVICE_ID = "stable-pseudonymous-device-id"

auth = requests.post(
    ENDPOINT + "/token",
    headers={"Authorization": "Bearer " + API_KEY},
    json={"deviceId": DEVICE_ID},
    timeout=5,
).json()

timestamp = int(time.time() * 1000)
nonce = str(uuid.uuid4())
path = "/api/v1/ingest/events"
raw_body = json.dumps({
    "items": [{"name": "app_startup", "appVersion": "1.0.0", "timestamp": timestamp}]
}, separators=(",", ":")).encode()
body_hash = hashlib.sha256(raw_body).hexdigest()
canonical = (
    "sonde-hmac-sha256-v2\\n"
    + str(timestamp) + "\\n" + nonce + "\\nPOST\\n" + path + "\\n" + body_hash
).encode()
signature = hmac.new(auth["signingKey"].encode(), canonical, hashlib.sha256).hexdigest()

requests.post(
    "${origin}" + path,
    data=raw_body,
    headers={
        "Authorization": "Bearer " + auth["token"],
        "Content-Type": "application/json",
        "x-sonde-timestamp": str(timestamp),
        "x-sonde-nonce": nonce,
        "x-sonde-signature": signature,
    },
    timeout=5,
).raise_for_status()`;
}

function shellSnippet(origin: string, apiKey: string): string {
  return `# API Key is bootstrap-only.
API_KEY='${apiKey}'
DEVICE_ID='stable-pseudonymous-device-id'
BASE='${origin}'

curl -sS -X POST "$BASE/api/v1/ingest/token" \\
  -H "Authorization: Bearer $API_KEY" \\
  -H 'Content-Type: application/json' \\
  --data "{\\"deviceId\\":\\"$DEVICE_ID\\"}"

# Read token + signingKey from the JSON response.
# For each telemetry request:
#   timestamp = Unix milliseconds
#   nonce = cryptographically random unique value
#   body_hash = lowercase hex SHA-256 of the EXACT raw body bytes
#   canonical = sonde-hmac-sha256-v2 + newline + timestamp + newline + nonce
#               + newline + POST + newline + /api/v1/ingest/events
#               + newline + body_hash
#   signature = lowercase hex HMAC-SHA256(signingKey, canonical)
#
# Then send:
curl -X POST "$BASE/api/v1/ingest/events" \\
  -H 'Authorization: Bearer <sndt_device_token>' \\
  -H 'Content-Type: application/json' \\
  -H 'x-sonde-timestamp: <timestamp>' \\
  -H 'x-sonde-nonce: <nonce>' \\
  -H 'x-sonde-signature: <signature>' \\
  --data-binary @telemetry.json`;
}
