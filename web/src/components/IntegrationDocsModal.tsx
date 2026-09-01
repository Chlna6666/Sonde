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
                API Key 只用于换取短期设备 Token。设备身份来自 Token，活动时间、会话边界、在线状态和统计周期由 Sonde 使用服务端接收时间维护；客户端只报告当前事实与业务遥测。
              </p>
              <p className="m-0 mt-2 break-all font-mono text-[10px] text-[var(--faint)]">
                {origin}/api/v1/ingest · slug={app.slug}
              </p>
            </div>
          </div>
        </div>

        <div className="grid gap-3 md:grid-cols-4">
          <Step number="1" title="Bootstrap" text="API Key + 稳定 pseudonymous deviceId → /ingest/token" />
          <Step number="2" title="Device heartbeat" text="用短期 Token 报告 appVersion / OS / language / architecture 等当前事实。" />
          <Step number="3" title="Canonical request" text="签名请求头使用当前时间、nonce、method、path 和原始 body；它不属于 telemetry payload。" />
          <Step number="4" title="Business telemetry" text="events / metrics / logs / errors 只提交业务数据，身份、时间和 session 由平台注入。" />
        </div>

        <div className="rounded-2xl border border-[var(--amber)]/30 bg-[var(--amber-subtle)] p-3.5 text-xs leading-relaxed text-[var(--text)]">
          <div className="flex items-center gap-2 font-bold">
            <KeyRound size={15} className="text-[var(--amber)]" />
            长期 API Key 和平台字段都不能写进 telemetry
          </div>
          <p className="m-0 mt-1.5 text-[var(--muted)]">
            API Key 只能调用 /token。live telemetry item 不接受
            <code className="mx-1 rounded bg-[var(--input-bg)] px-1.5 py-0.5">timestamp</code>
            <code className="mx-1 rounded bg-[var(--input-bg)] px-1.5 py-0.5">sessionId</code>
            <code className="mx-1 rounded bg-[var(--input-bg)] px-1.5 py-0.5">anonymousId</code>；这些字段由平台维护。请求头
            <code className="mx-1 rounded bg-[var(--input-bg)] px-1.5 py-0.5">x-sonde-timestamp</code>
            仅用于 HMAC 防重放，不参与在线时间或 DAU/WAU/MAU 统计。
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
/api/v1/ingest/heartbeat\n
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
${comment} 1) Bootstrap only: exchange API Key for a device-bound short-lived token.
POST ${origin}/api/v1/ingest/token
Authorization: Bearer ${apiKey}
Content-Type: application/json

{"deviceId":"stable-pseudonymous-device-id"}

${comment} Response: token, signingKey, expiresAt, signatureVersion.
${comment} The request-signing timestamp below is NOT telemetry event time.

${comment} 2) Report current device/application facts. Sonde owns lastSeen/session/time statistics.
POST ${origin}/api/v1/ingest/heartbeat
Authorization: Bearer <sndt_device_token>
x-sonde-timestamp: <request-signing-unix-ms>
x-sonde-nonce: <random-nonce>
x-sonde-signature: <signature-for-heartbeat-body>
Content-Type: application/json

{"appVersion":"1.0.0","os":"Windows 11 24H2","systemLanguage":"zh-CN","architecture":"x86_64"}

${comment} 3) Business telemetry does not carry timestamp/sessionId/anonymousId.
POST ${origin}/api/v1/ingest/events
Authorization: Bearer <sndt_device_token>
x-sonde-timestamp: <request-signing-unix-ms>
x-sonde-nonce: <random-nonce>
x-sonde-signature: <signature-for-event-body>
Content-Type: application/json

{"items":[{"name":"app_startup","attributes":{"channel":"stable"}}]}

${comment} API Key never goes to heartbeat/events/metrics/logs/errors.`;
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

async fn send_signed(
    client: &Client,
    auth: &TokenResponse,
    path: &str,
    body: Vec<u8>,
) -> reqwest::Result<()> {
    let request_timestamp = chrono::Utc::now().timestamp_millis();
    let nonce = uuid::Uuid::now_v7().to_string();
    let signature = sign(&auth.signing_key, request_timestamp, &nonce, path, &body);
    client.post(format!("${origin}{}", path))
        .bearer_auth(&auth.token)
        .header("content-type", "application/json")
        .header("x-sonde-timestamp", request_timestamp.to_string())
        .header("x-sonde-nonce", nonce)
        .header("x-sonde-signature", signature)
        .body(body)
        .send().await?.error_for_status()?;
    Ok(())
}

async fn report(client: &Client, auth: &TokenResponse) -> reqwest::Result<()> {
    let heartbeat = serde_json::to_vec(&serde_json::json!({
        "appVersion": "1.0.0",
        "os": "Windows 11 24H2",
        "systemLanguage": "zh-CN",
        "architecture": std::env::consts::ARCH
    })).unwrap();
    send_signed(client, auth, "/api/v1/ingest/heartbeat", heartbeat).await?;

    // No timestamp, sessionId or anonymousId in live telemetry items.
    let event = serde_json::to_vec(&serde_json::json!({
        "items": [{ "name": "app_startup" }]
    })).unwrap();
    send_signed(client, auth, "/api/v1/ingest/events", event).await
}`;
}

function typescriptSnippet(origin: string, apiKey: string): string {
  return `const endpoint = "${origin}/api/v1/ingest";
const apiKey = "${apiKey}"; // bootstrap only
const deviceId = "stable-pseudonymous-device-id";
const encoder = new TextEncoder();

const tokenResponse = await fetch(endpoint + "/token", {
  method: "POST",
  headers: { authorization: "Bearer " + apiKey, "content-type": "application/json" },
  body: JSON.stringify({ deviceId }),
});
const auth = await tokenResponse.json();

function hex(bytes: ArrayBuffer) {
  return [...new Uint8Array(bytes)].map((value) => value.toString(16).padStart(2, "0")).join("");
}

async function sendSigned(path: string, payload: unknown) {
  const requestTimestamp = Date.now(); // HMAC replay protection only.
  const nonce = crypto.randomUUID();
  const rawBody = JSON.stringify(payload);
  const canonicalPath = "/api/v1/ingest" + path;
  const bodyHash = hex(await crypto.subtle.digest("SHA-256", encoder.encode(rawBody)));
  const canonical =
    "sonde-hmac-sha256-v2\\n" + requestTimestamp + "\\n" + nonce +
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
      "x-sonde-timestamp": String(requestTimestamp),
      "x-sonde-nonce": nonce,
      "x-sonde-signature": signature,
    },
    body: rawBody,
  });
}

await sendSigned("/heartbeat", {
  appVersion: "1.0.0",
  os: "Windows 11 24H2",
  systemLanguage: navigator.language,
  architecture: "x86_64",
});
await sendSigned("/events", { items: [{ name: "app_startup" }] });`;
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

def send_signed(path, payload):
    request_timestamp = int(time.time() * 1000)  # signing/replay protection only
    nonce = str(uuid.uuid4())
    raw_body = json.dumps(payload, separators=(",", ":")).encode()
    canonical_path = "/api/v1/ingest" + path
    body_hash = hashlib.sha256(raw_body).hexdigest()
    canonical = (
        "sonde-hmac-sha256-v2\\n" + str(request_timestamp) + "\\n" + nonce
        + "\\nPOST\\n" + canonical_path + "\\n" + body_hash
    ).encode()
    signature = hmac.new(auth["signingKey"].encode(), canonical, hashlib.sha256).hexdigest()
    requests.post(
        ENDPOINT + path,
        data=raw_body,
        headers={
            "Authorization": "Bearer " + auth["token"],
            "Content-Type": "application/json",
            "x-sonde-timestamp": str(request_timestamp),
            "x-sonde-nonce": nonce,
            "x-sonde-signature": signature,
        },
        timeout=5,
    ).raise_for_status()

send_signed("/heartbeat", {
    "appVersion": "1.0.0",
    "os": "Windows 11 24H2",
    "systemLanguage": "zh-CN",
    "architecture": "x86_64",
})
send_signed("/events", {"items": [{"name": "app_startup"}]})`;
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
# Sign the EXACT raw body with the request timestamp / nonce / method / canonical path.
# x-sonde-timestamp is for HMAC replay protection only; it is not telemetry time.
#
# heartbeat.json example:
# {"appVersion":"1.0.0","os":"Windows 11 24H2","systemLanguage":"zh-CN","architecture":"x86_64"}
#
# event.json example (no timestamp/sessionId/anonymousId):
# {"items":[{"name":"app_startup"}]}

curl -X POST "$BASE/api/v1/ingest/heartbeat" \\
  -H 'Authorization: Bearer <sndt_device_token>' \\
  -H 'Content-Type: application/json' \\
  -H 'x-sonde-timestamp: <request-signing-timestamp>' \\
  -H 'x-sonde-nonce: <nonce>' \\
  -H 'x-sonde-signature: <signature>' \\
  --data-binary @heartbeat.json`;
}
