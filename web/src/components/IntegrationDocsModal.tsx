import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Copy,
  Check,
  Sliders,
} from "lucide-react";
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

export function IntegrationDocsModal({
  isOpen,
  onClose,
  app,
  apiKeyPrefix,
}: IntegrationDocsModalProps) {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<LanguageTab>("rust");
  const [disableLogs, setDisableLogs] = useState(false);
  const [captureErrors, setCaptureErrors] = useState(true);
  const [copied, setCopied] = useState(false);

  const origin = typeof window !== "undefined" ? window.location.origin : "https://telemetry.yourdomain.com";
  const apiKeyPlaceholder = apiKeyPrefix ? `${apiKeyPrefix}********************` : "sonde_key_xxxxxxxxxxxxxxxxxxxxxxxx";

  const generateCode = (lang: LanguageTab) => {
    switch (lang) {
      case "rust":
        return `// Cargo.toml
// [dependencies]
// serde = { version = "1.0", features = ["derive"] }
// serde_json = "1.0"
// reqwest = { version = "0.12", features = ["json"] }
// tokio = { version = "1", features = ["full"] }

use reqwest::Client;
use serde_json::json;

pub struct SondeClient {
    client: Client,
    endpoint: String,
    api_key: String,
    disable_logs: bool,
}

impl SondeClient {
    pub fn new(api_key: &str) -> Self {
        Self {
            client: Client::new(),
            endpoint: "${origin}/api/v1/ingest".into(),
            api_key: api_key.into(),
            disable_logs: ${disableLogs},
        }
    }

    /// 上报事件 (包含匿名设备 ID、日活与版本)
    pub async fn track_event(&self, name: &str, device_id: &str, version: &str) -> Result<(), reqwest::Error> {
        let payload = json!({
            "items": [{
                "name": name,
                "anonymousId": device_id,
                "appVersion": version,
                "os": std::env::consts::OS,
                "timestamp": chrono::Utc::now().timestamp_millis(),
                "attributes": {
                    "env": "production"
                }
            }]
        });

        self.client
            .post(format!("{}/events", self.endpoint))
            .header("x-sonde-key", &self.api_key)
            .json(&payload)
            .send()
            .await?;
        Ok(())
    }
${captureErrors ? `
    /// 规范化上报崩溃与错误信号
    pub async fn report_error(&self, err_type: &str, message: &str, stack: Option<&str>, handled: bool) -> Result<(), reqwest::Error> {
        let payload = json!({
            "items": [{
                "name": err_type,
                "message": message,
                "stackTrace": stack,
                "severity": if handled { "error" } else { "fatal" },
                "handled": handled,
                "timestamp": chrono::Utc::now().timestamp_millis()
            }]
        });

        self.client
            .post(format!("{}/errors", self.endpoint))
            .header("x-sonde-key", &self.api_key)
            .json(&payload)
            .send()
            .await?;
        Ok(())
    }` : ""}${!disableLogs ? `
    /// 上报运行日志
    pub async fn log(&self, level: &str, msg: &str) -> Result<(), reqwest::Error> {
        if self.disable_logs { return Ok(()); }
        let payload = json!({
            "items": [{
                "level": level,
                "message": msg,
                "timestamp": chrono::Utc::now().timestamp_millis()
            }]
        });

        self.client
            .post(format!("{}/logs", self.endpoint))
            .header("x-sonde-key", &self.api_key)
            .json(&payload)
            .send()
            .await?;
        Ok(())
    }` : ""}
}

#[tokio::main]
async fn main() {
    let client = SondeClient::new("${apiKeyPlaceholder}");
    client.track_event("app_startup", "device-uuid-1234", "1.0.0").await.unwrap();
}`;

      case "typescript":
        return `// npm install axios
import axios from "axios";

export interface SondeConfig {
  apiKey: string;
  endpoint?: string;
  disableLogs?: boolean;
  autoCaptureErrors?: boolean;
}

export class SondeTelemetry {
  private apiKey: string;
  private endpoint: string;
  private disableLogs: boolean;

  constructor(config: SondeConfig) {
    this.apiKey = config.apiKey;
    this.endpoint = config.endpoint || "${origin}/api/v1/ingest";
    this.disableLogs = config.disableLogs ?? ${disableLogs};

    ${captureErrors ? `// 全局异常自动监听
    if (typeof window !== "undefined" && config.autoCaptureErrors !== false) {
      window.addEventListener("error", (event) => {
        this.reportError("UncaughtException", event.message, event.error?.stack, false);
      });
      window.addEventListener("unhandledrejection", (event) => {
        this.reportError("UnhandledRejection", String(event.reason), event.reason?.stack, false);
      });
    }` : ""}
  }

  /** 上报事件与活跃用户 */
  async track(name: string, properties: Record<string, any> = {}) {
    await axios.post(
      \`\${this.endpoint}/events\`,
      {
        items: [{
          name,
          anonymousId: properties.deviceId || "web-client",
          appVersion: properties.version || "1.0.0",
          os: navigator.userAgent,
          timestamp: Date.now(),
          attributes: properties,
        }],
      },
      { headers: { "x-sonde-key": this.apiKey } }
    );
  }
${captureErrors ? `
  /** 规范化错误与异常上报 */
  async reportError(name: string, message: string, stackTrace?: string, handled = true) {
    await axios.post(
      \`\${this.endpoint}/errors\`,
      {
        items: [{
          name,
          message,
          stackTrace,
          severity: handled ? "error" : "fatal",
          handled,
          timestamp: Date.now(),
        }],
      },
      { headers: { "x-sonde-key": this.apiKey } }
    );
  }` : ""}${!disableLogs ? `
  /** 上报业务日志 */
  async log(level: "info" | "warn" | "error", message: string) {
    if (this.disableLogs) return;
    await axios.post(
      \`\${this.endpoint}/logs\`,
      {
        items: [{ level, message, timestamp: Date.now() }],
      },
      { headers: { "x-sonde-key": this.apiKey } }
    );
  }` : ""}
}

// 初始化客户端
const telemetry = new SondeTelemetry({
  apiKey: "${apiKeyPlaceholder}",
  disableLogs: ${disableLogs},
});
telemetry.track("page_view", { path: "/dashboard" });`;

      case "go":
        return `package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
	"time"
)

type SondeClient struct {
	Endpoint    string
	APIKey      string
	DisableLogs bool
	HttpClient  *http.Client
}

func NewSondeClient(apiKey string) *SondeClient {
	return &SondeClient{
		Endpoint:    "${origin}/api/v1/ingest",
		APIKey:      apiKey,
		DisableLogs: ${disableLogs},
		HttpClient:  &http.Client{Timeout: 5 * time.Second},
	}
}

func (s *SondeClient) TrackEvent(name, anonymousId, version string) error {
	payload := map[string]interface{}{
		"items": []map[string]interface{}{
			{
				"name":        name,
				"anonymousId": anonymousId,
				"appVersion":  version,
				"timestamp":   time.Now().UnixMilli(),
			},
		},
	}
	return s.send("events", payload)
}
${captureErrors ? `
func (s *SondeClient) ReportError(name, msg, stack string, handled bool) error {
	severity := "error"
	if !handled {
		severity = "fatal"
	}
	payload := map[string]interface{}{
		"items": []map[string]interface{}{
			{
				"name":       name,
				"message":    msg,
				"stackTrace": stack,
				"severity":   severity,
				"handled":    handled,
				"timestamp":  time.Now().UnixMilli(),
			},
		},
	}
	return s.send("errors", payload)
}` : ""}${!disableLogs ? `
func (s *SondeClient) Log(level, message string) error {
	if s.DisableLogs {
		return nil
	}
	payload := map[string]interface{}{
		"items": []map[string]interface{}{
			{
				"level":     level,
				"message":   message,
				"timestamp": time.Now().UnixMilli(),
			},
		},
	}
	return s.send("logs", payload)
}` : ""}
func (s *SondeClient) send(path string, data interface{}) error {
	body, _ := json.Marshal(data)
	req, _ := http.NewRequest("POST", fmt.Sprintf("%s/%s", s.Endpoint, path), bytes.NewBuffer(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("x-sonde-key", s.APIKey)
	resp, err := s.HttpClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	return nil
}

func main() {
	client := NewSondeClient("${apiKeyPlaceholder}")
	_ = client.TrackEvent("service_start", "srv-node-01", "v1.2.0")
}`;

      case "python":
        return `import requests
import time
import sys
import traceback

class SondeClient:
    def __init__(self, api_key: str, endpoint: str = "${origin}/api/v1/ingest", disable_logs: bool = ${disableLogs ? "True" : "False"}):
        self.api_key = api_key
        self.endpoint = endpoint
        self.disable_logs = disable_logs
        self.headers = {"x-sonde-key": self.api_key, "Content-Type": "application/json"}
        ${captureErrors ? `# 安装未处理崩溃拦截钩子
        sys.excepthook = self._handle_uncaught_exception` : ""}

    def track(self, name: str, anonymous_id: str = None, version: str = "1.0.0", attributes: dict = None):
        payload = {
            "items": [{
                "name": name,
                "anonymousId": anonymous_id,
                "appVersion": version,
                "timestamp": int(time.time() * 1000),
                "attributes": attributes or {}
            }]
        }
        requests.post(f"{self.endpoint}/events", json=payload, headers=self.headers, timeout=5)
${captureErrors ? `
    def report_error(self, name: str, message: str, stack_trace: str = None, handled: bool = True):
        payload = {
            "items": [{
                "name": name,
                "message": message,
                "stackTrace": stack_trace,
                "severity": "error" if handled else "fatal",
                "handled": handled,
                "timestamp": int(time.time() * 1000)
            }]
        }
        requests.post(f"{self.endpoint}/errors", json=payload, headers=self.headers, timeout=5)

    def _handle_uncaught_exception(self, exc_type, exc_value, exc_traceback):
        tb_str = "".join(traceback.format_exception(exc_type, exc_value, exc_traceback))
        self.report_error(exc_type.__name__, str(exc_value), tb_str, handled=False)
        sys.__excepthook__(exc_type, exc_value, exc_traceback)` : ""}${!disableLogs ? `
    def log(self, level: str, message: str):
        if self.disable_logs:
            return
        payload = {
            "items": [{
                "level": level,
                "message": message,
                "timestamp": int(time.time() * 1000)
            }]
        }
        requests.post(f"{self.endpoint}/logs", json=payload, headers=self.headers, timeout=5)` : ""}

# 初始化
sonde = SondeClient("${apiKeyPlaceholder}")
sonde.track("startup", anonymous_id="usr-py-99", version="2.0.1")`;

      case "csharp":
        return `using System;
using System.Net.Http;
using System.Text;
using System.Text.Json;
using System.Threading.Tasks;

public class SondeClient
{
    private readonly HttpClient _http = new HttpClient();
    private readonly string _endpoint = "${origin}/api/v1/ingest";
    private readonly string _apiKey;
    public bool DisableLogs { get; set; } = ${disableLogs ? "true" : "false"};

    public SondeClient(string apiKey)
    {
        _apiKey = apiKey;
        _http.DefaultRequestHeaders.Add("x-sonde-key", _apiKey);
        ${captureErrors ? `
        AppDomain.CurrentDomain.UnhandledException += (s, e) => {
            if (e.ExceptionObject is Exception ex)
                ReportErrorAsync(ex.GetType().Name, ex.Message, ex.StackTrace, false).Wait();
        };`: ""}
    }

    public async Task TrackEventAsync(string name, string deviceId, string version)
    {
        var payload = new {
            items = new[] {
                new {
                    name = name,
                    anonymousId = deviceId,
                    appVersion = version,
                    timestamp = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds()
                }
            }
        };
        await _http.PostAsync(\`\${_endpoint}/events\`, new StringContent(JsonSerializer.Serialize(payload), Encoding.UTF8, "application/json"));
    }
${captureErrors ? `
    public async Task ReportErrorAsync(string name, string message, string stackTrace = null, bool handled = true)
    {
        var payload = new {
            items = new[] {
                new {
                    name = name,
                    message = message,
                    stackTrace = stackTrace,
                    severity = handled ? "error" : "fatal",
                    handled = handled,
                    timestamp = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds()
                }
            }
        };
        await _http.PostAsync(\`\${_endpoint}/errors\`, new StringContent(JsonSerializer.Serialize(payload), Encoding.UTF8, "application/json"));
    }` : ""}${!disableLogs ? `
    public async Task LogAsync(string level, string message)
    {
        if (DisableLogs) return;
        var payload = new {
            items = new[] {
                new {
                    level = level,
                    message = message,
                    timestamp = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds()
                }
            }
        };
        await _http.PostAsync(\`\${_endpoint}/logs\`, new StringContent(JsonSerializer.Serialize(payload), Encoding.UTF8, "application/json"));
    }` : ""}
}`;

      case "cpp":
        return `// C++ Standard Library & Libcurl example
#include <iostream>
#include <string>
#include <curl/curl.h>

void sonde_track_event(const std::string& api_key, const std::string& name, const std::string& device_id) {
    CURL* curl = curl_easy_init();
    if (curl) {
        std::string json_data = "{\\"items\\":[{\\"name\\":\\"" + name + "\\",\\"anonymousId\\":\\"" + device_id + "\\"}]}";
        struct curl_slist* headers = NULL;
        headers = curl_slist_append(headers, "Content-Type: application/json");
        std::string auth = "x-sonde-key: " + api_key;
        headers = curl_slist_append(headers, auth.c_str());

        curl_easy_setopt(curl, CURLOPT_URL, "${origin}/api/v1/ingest/events");
        curl_easy_setopt(curl, CURLOPT_HTTPHEADER, headers);
        curl_easy_setopt(curl, CURLOPT_POSTFIELDS, json_data.c_str());

        curl_easy_perform(curl);
        curl_slist_free_all(headers);
        curl_easy_cleanup(curl);
    }
}

int main() {
    sonde_track_event("${apiKeyPlaceholder}", "app_launch", "device_c_001");
    return 0;
}`;

      case "curl":
        return `# 1. 上报事件与日活 (Events)
curl -X POST "${origin}/api/v1/ingest/events" \\
  -H "Content-Type: application/json" \\
  -H "x-sonde-key: ${apiKeyPlaceholder}" \\
  -d '{
    "items": [
      {
        "name": "login_success",
        "anonymousId": "user_device_9921",
        "appVersion": "1.4.0",
        "os": "Windows 11 Build 26200",
        "attributes": { "role": "admin" }
      }
    ]
  }'

# 2. 规范化错误与崩溃信号上报 (Errors)
curl -X POST "${origin}/api/v1/ingest/errors" \\
  -H "Content-Type: application/json" \\
  -H "x-sonde-key: ${apiKeyPlaceholder}" \\
  -d '{
    "items": [
      {
        "name": "NullReferenceException",
        "message": "Object reference not set to an instance of an object",
        "stackTrace": "at App.MainModule.Execute() in MainModule.cs:line 42",
        "severity": "fatal",
        "handled": false
      }
    ]
  }'
${!disableLogs ? `
# 3. 业务日志上传 (Logs)
curl -X POST "${origin}/api/v1/ingest/logs" \\
  -H "Content-Type: application/json" \\
  -H "x-sonde-key: ${apiKeyPlaceholder}" \\
  -d '{
    "items": [
      {
        "level": "info",
        "message": "Data pipeline initialized successfully"
      }
    ]
  }'` : ""}`;
    }
  };

  const codeSnippet = generateCode(activeTab);

  const handleCopy = () => {
    navigator.clipboard.writeText(codeSnippet);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <Modal isOpen={isOpen} onClose={onClose} title="Sonde SDK & 接入指南" size="xl">
      <div className="space-y-5">
        {/* App Info Banner */}
        <div className="flex items-center justify-between p-4 rounded-2xl bg-[var(--input-bg)] border border-[var(--border-soft)]">
          <div className="flex items-center gap-3">
            <div className="h-10 w-10 rounded-xl bg-[var(--signal-subtle)] border border-[var(--signal)]/30 text-[var(--signal)] flex items-center justify-center font-bold">
              {app.name.slice(0, 2).toUpperCase()}
            </div>
            <div>
              <h4 className="text-sm font-bold text-[var(--text)] m-0">{app.name}</h4>
              <p className="text-xs font-mono text-[var(--muted)] m-0 mt-0.5">
                Slug: <span className="text-[var(--signal)] font-semibold">{app.slug}</span> · Endpoint: <span className="text-[var(--text)] font-semibold">{origin}/api/v1/ingest</span>
              </p>
            </div>
          </div>
        </div>

        {/* Dynamic Feature Config Toggles */}
        <div className="p-3.5 rounded-2xl bg-[var(--panel-strong)] border border-[var(--border-soft)] flex flex-wrap items-center justify-between gap-4 text-xs">
          <div className="flex items-center gap-2 font-bold text-[var(--text)]">
            <Sliders size={15} className="text-[var(--signal)]" />
            <span>实时代码配置生成器</span>
          </div>

          <div className="flex items-center gap-4 flex-wrap">
            <label className="inline-flex items-center gap-1.5 cursor-pointer select-none text-[var(--text)] font-medium">
              <input
                type="checkbox"
                checked={captureErrors}
                onChange={(e) => setCaptureErrors(e.target.checked)}
                className="rounded text-[var(--signal)] focus:ring-0"
              />
              <span>捕获未处理崩溃与异常</span>
            </label>

            <label className="inline-flex items-center gap-1.5 cursor-pointer select-none text-[var(--text)] font-medium">
              <input
                type="checkbox"
                checked={disableLogs}
                onChange={(e) => setDisableLogs(e.target.checked)}
                className="rounded text-[var(--signal)] focus:ring-0"
              />
              <span className={disableLogs ? "text-[var(--amber)] font-bold" : ""}>
                关闭日志上传 (disable_logs)
              </span>
            </label>
          </div>
        </div>

        {/* Language Tabs */}
        <div className="flex items-center gap-1.5 overflow-x-auto pb-1 scrollbar-none border-b border-[var(--border-soft)]">
          <button
            type="button"
            className={`px-3 py-1.5 rounded-xl text-xs font-bold transition-all cursor-pointer inline-flex items-center gap-1.5 ${
              activeTab === "rust"
                ? "bg-[var(--signal)] text-white shadow-xs"
                : "text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--input-bg)]"
            }`}
            onClick={() => setActiveTab("rust")}
          >
            <span>🦀 Rust</span>
          </button>

          <button
            type="button"
            className={`px-3 py-1.5 rounded-xl text-xs font-bold transition-all cursor-pointer inline-flex items-center gap-1.5 ${
              activeTab === "typescript"
                ? "bg-[var(--signal)] text-white shadow-xs"
                : "text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--input-bg)]"
            }`}
            onClick={() => setActiveTab("typescript")}
          >
            <span>🌐 TypeScript / JS</span>
          </button>

          <button
            type="button"
            className={`px-3 py-1.5 rounded-xl text-xs font-bold transition-all cursor-pointer inline-flex items-center gap-1.5 ${
              activeTab === "go"
                ? "bg-[var(--signal)] text-white shadow-xs"
                : "text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--input-bg)]"
            }`}
            onClick={() => setActiveTab("go")}
          >
            <span>🐹 Go</span>
          </button>

          <button
            type="button"
            className={`px-3 py-1.5 rounded-xl text-xs font-bold transition-all cursor-pointer inline-flex items-center gap-1.5 ${
              activeTab === "python"
                ? "bg-[var(--signal)] text-white shadow-xs"
                : "text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--input-bg)]"
            }`}
            onClick={() => setActiveTab("python")}
          >
            <span>🐍 Python</span>
          </button>

          <button
            type="button"
            className={`px-3 py-1.5 rounded-xl text-xs font-bold transition-all cursor-pointer inline-flex items-center gap-1.5 ${
              activeTab === "csharp"
                ? "bg-[var(--signal)] text-white shadow-xs"
                : "text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--input-bg)]"
            }`}
            onClick={() => setActiveTab("csharp")}
          >
            <span>🔷 C# / .NET</span>
          </button>

          <button
            type="button"
            className={`px-3 py-1.5 rounded-xl text-xs font-bold transition-all cursor-pointer inline-flex items-center gap-1.5 ${
              activeTab === "cpp"
                ? "bg-[var(--signal)] text-white shadow-xs"
                : "text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--input-bg)]"
            }`}
            onClick={() => setActiveTab("cpp")}
          >
            <span>⚡ C++</span>
          </button>

          <button
            type="button"
            className={`px-3 py-1.5 rounded-xl text-xs font-bold transition-all cursor-pointer inline-flex items-center gap-1.5 ${
              activeTab === "curl"
                ? "bg-[var(--signal)] text-white shadow-xs"
                : "text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--input-bg)]"
            }`}
            onClick={() => setActiveTab("curl")}
          >
            <span>🐚 cURL / REST</span>
          </button>
        </div>

        {/* Code Snippet Box */}
        <div className="relative rounded-2xl bg-[#0d1117] border border-[var(--border)] overflow-hidden shadow-2xl">
          <div className="flex items-center justify-between px-4 py-2 bg-[#161b22] border-b border-[#30363d] text-xs">
            <span className="font-mono text-gray-400 font-semibold uppercase">{activeTab} SDK Integration</span>
            <button
              type="button"
              onClick={handleCopy}
              className="inline-flex items-center gap-1.5 px-3 py-1 rounded-lg bg-[var(--signal)] text-white font-bold hover:brightness-110 active:scale-95 transition-all cursor-pointer shadow-xs"
            >
              {copied ? <Check size={13} /> : <Copy size={13} />}
              <span>{copied ? "已复制到剪贴板" : "复制代码"}</span>
            </button>
          </div>
          <pre className="p-4 text-xs font-mono text-gray-200 overflow-x-auto max-h-[380px] leading-relaxed select-text">
            <code>{codeSnippet}</code>
          </pre>
        </div>
      </div>
    </Modal>
  );
}
