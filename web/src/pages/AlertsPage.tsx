import { FormEvent, useEffect, useState, type InputHTMLAttributes } from "react";
import {
  BellRing,
  Plus,
  ShieldCheck,
  Radio,
  Send,
  Trash2,
  RefreshCw,
  CheckCircle2,
  AlertTriangle,
  Flame,
  Globe,
  MessageSquare,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { motion } from "motion/react";
import { CustomSelect } from "../components/CustomSelect";
import { api } from "../lib/api";

type Rule = {
  id: string;
  applicationId: string;
  name: string;
  sourceKind: string;
  query: {
    source: string;
    operator: string;
    threshold: number;
    windowMinutes: number;
    consecutiveHits: number;
    filters: { field: string; value: string }[];
  };
  windowMinutes: number;
  cooldownSeconds: number;
  lastState: string;
  enabled: boolean;
};

type NotificationChannel = {
  id: string;
  name: string;
  kind: string;
  config: { url?: string; [key: string]: unknown };
  enabled: boolean;
  createdAt: number;
};

type Delivery = {
  id: string;
  ruleId: string;
  ruleName?: string;
  channelId: string;
  channelName?: string;
  status: string;
  attempts: number;
  lastError?: string;
  createdAt: number;
};

type Application = { id: string; name: string };

type Tab = "rules" | "channels" | "deliveries";

export function AlertsPage() {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<Tab>("rules");
  const [rules, setRules] = useState<Rule[]>([]);
  const [channels, setChannels] = useState<NotificationChannel[]>([]);
  const [deliveries, setDeliveries] = useState<Delivery[]>([]);
  const [applications, setApplications] = useState<Application[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [successMsg, setSuccessMsg] = useState("");

  // Rule Form State
  const [creatingRule, setCreatingRule] = useState(false);
  const [ruleAppId, setRuleAppId] = useState("");
  const [ruleSourceKind, setRuleSourceKind] = useState("event_count");
  const [ruleOperator, setRuleOperator] = useState("greater_or_equal");
  const [ruleThreshold, setRuleThreshold] = useState("10");
  const [ruleWindow, setRuleWindow] = useState("5");
  const [ruleCooldown, setRuleCooldown] = useState("300");

  // Channel Form State
  const [creatingChannel, setCreatingChannel] = useState(false);
  const [channelKind, setChannelKind] = useState("webhook");
  const [testingChannelId, setTestingChannelId] = useState<string | null>(null);

  const loadAll = async () => {
    setLoading(true);
    setError("");
    try {
      const [rulesRes, channelsRes, deliveriesRes, appsRes] = await Promise.allSettled([
        api<Rule[]>("/api/v1/admin/alerts/rules"),
        api<NotificationChannel[]>("/api/v1/admin/alerts/channels"),
        api<Delivery[]>("/api/v1/admin/alerts/deliveries?limit=50"),
        api<Application[]>("/api/v1/admin/applications"),
      ]);

      if (rulesRes.status === "fulfilled") setRules(rulesRes.value);
      if (channelsRes.status === "fulfilled") setChannels(channelsRes.value);
      if (deliveriesRes.status === "fulfilled") setDeliveries(deliveriesRes.value);
      if (appsRes.status === "fulfilled") {
        setApplications(appsRes.value);
        setRuleAppId((current) => current || appsRes.value[0]?.id || "");
      }

      if (rulesRes.status === "rejected" && channelsRes.status === "rejected") {
        const cause = rulesRes.reason;
        setError(cause instanceof Error ? cause.message : String(cause));
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadAll();
  }, []);

  const createRule = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setError("");
    setSuccessMsg("");
    try {
      await api("/api/v1/admin/alerts/rules", {
        method: "POST",
        body: JSON.stringify({
          applicationId: ruleAppId,
          name: form.get("name"),
          cooldownSeconds: Number(ruleCooldown),
          expression: {
            source: ruleSourceKind,
            operator: ruleOperator,
            threshold: Number(ruleThreshold),
            windowMinutes: Number(ruleWindow),
            consecutiveHits: 1,
            filters: [],
          },
        }),
      });
      setCreatingRule(false);
      setSuccessMsg("告警规则创建成功！");
      await loadAll();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  };

  const deleteRule = async (id: string) => {
    if (!window.confirm(t("alerts.deleteRuleConfirm"))) return;
    try {
      await api(`/api/v1/admin/alerts/rules/${id}`, { method: "DELETE" });
      setSuccessMsg("规则已删除");
      await loadAll();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  };

  const createChannel = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setError("");
    setSuccessMsg("");
    try {
      const config: Record<string, any> = {};
      if (channelKind === "telegram") {
        config.botToken = form.get("botToken");
        config.chatId = form.get("chatId");
      } else {
        config.url = form.get("url");
      }

      await api("/api/v1/admin/alerts/channels", {
        method: "POST",
        body: JSON.stringify({
          name: form.get("name"),
          kind: channelKind,
          config,
          enabled: true,
        }),
      });
      setCreatingChannel(false);
      setSuccessMsg("通知渠道创建成功！");
      await loadAll();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  };

  const deleteChannel = async (id: string) => {
    if (!window.confirm(t("alerts.deleteChannelConfirm"))) return;
    try {
      await api(`/api/v1/admin/alerts/channels/${id}`, { method: "DELETE" });
      setSuccessMsg("通知渠道已删除");
      await loadAll();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  };

  const testChannel = async (id: string) => {
    setTestingChannelId(id);
    setError("");
    setSuccessMsg("");
    try {
      await api(`/api/v1/admin/alerts/channels/${id}/test`, { method: "POST" });
      setSuccessMsg(t("alerts.channelTestSuccess"));
      await loadAll();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setTestingChannelId(null);
    }
  };

  return (
    <div className="page enter-page">
      <header className="page-header">
        <div>
          <span className="eyebrow">{t("alerts.eyebrow")}</span>
          <h1>{t("alerts.title")}</h1>
          <p>{t("alerts.subtitle")}</p>
        </div>
        <div className="flex items-center gap-2">
          {activeTab === "rules" ? (
            <button className="primary-button compact" onClick={() => setCreatingRule(true)}>
              <Plus size={17} aria-hidden="true" />
              {t("alerts.new")}
            </button>
          ) : activeTab === "channels" ? (
            <button className="primary-button compact" onClick={() => setCreatingChannel(true)}>
              <Plus size={17} aria-hidden="true" />
              {t("alerts.newChannel")}
            </button>
          ) : null}
          <button
            type="button"
            onClick={() => void loadAll()}
            disabled={loading}
            className="p-2 rounded-xl border border-[var(--border-soft)] text-[var(--muted)] hover:text-[var(--text)] transition-colors cursor-pointer"
            title="Refresh"
          >
            <RefreshCw size={15} className={loading ? "animate-spin" : ""} />
          </button>
        </div>
      </header>

      {/* Tabs Header with Apple Sliding Pill */}
      <div className="segmented-control mb-6 self-start">
        <button
          type="button"
          onClick={() => setActiveTab("rules")}
          className={`segmented-control-item flex items-center gap-2 ${activeTab === "rules" ? "active" : ""}`}
        >
          {activeTab === "rules" ? (
            <motion.div
              layoutId="alerts-active-tab"
              transition={{ type: "spring", stiffness: 450, damping: 32 }}
              className="segmented-control-pill"
            />
          ) : null}
          <span className="relative z-10 flex items-center gap-2">
            <BellRing size={14} />
            <span>{t("alerts.tabRules")}</span>
            <span className="px-1.5 py-0.5 rounded-full text-[10px] bg-[var(--input-bg)] border border-[var(--border-soft)] font-mono">
              {rules.length}
            </span>
          </span>
        </button>

        <button
          type="button"
          onClick={() => setActiveTab("channels")}
          className={`segmented-control-item flex items-center gap-2 ${activeTab === "channels" ? "active" : ""}`}
        >
          {activeTab === "channels" ? (
            <motion.div
              layoutId="alerts-active-tab"
              transition={{ type: "spring", stiffness: 450, damping: 32 }}
              className="segmented-control-pill"
            />
          ) : null}
          <span className="relative z-10 flex items-center gap-2">
            <Radio size={14} />
            <span>{t("alerts.tabChannels")}</span>
            <span className="px-1.5 py-0.5 rounded-full text-[10px] bg-[var(--input-bg)] border border-[var(--border-soft)] font-mono">
              {channels.length}
            </span>
          </span>
        </button>

        <button
          type="button"
          onClick={() => setActiveTab("deliveries")}
          className={`segmented-control-item flex items-center gap-2 ${activeTab === "deliveries" ? "active" : ""}`}
        >
          {activeTab === "deliveries" ? (
            <motion.div
              layoutId="alerts-active-tab"
              transition={{ type: "spring", stiffness: 450, damping: 32 }}
              className="segmented-control-pill"
            />
          ) : null}
          <span className="relative z-10 flex items-center gap-2">
            <Send size={14} />
            <span>{t("alerts.tabDeliveries")}</span>
            <span className="px-1.5 py-0.5 rounded-full text-[10px] bg-[var(--input-bg)] border border-[var(--border-soft)] font-mono">
              {deliveries.length}
            </span>
          </span>
        </button>
      </div>

      {error ? <p className="form-error mb-4" role="alert">{error}</p> : null}
      {successMsg ? (
        <div className="flex items-center gap-2 p-3 mb-4 rounded-xl bg-[var(--signal)]/10 border border-[var(--signal)] text-[var(--signal)] text-xs font-medium">
          <CheckCircle2 size={16} />
          {successMsg}
        </div>
      ) : null}

      {/* ----------------- TAB 1: RULES ----------------- */}
      {activeTab === "rules" ? (
        <div>
          {creatingRule ? (
            <form className="rounded-2xl border border-[var(--border)] bg-[var(--panel)] p-6 mb-6 space-y-4 shadow-sm" onSubmit={createRule}>
              <h3 className="text-sm font-bold text-[var(--text)] m-0">{t("alerts.new")}</h3>
              <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 gap-4">
                <Field name="name" label={t("alerts.ruleName")} placeholder="e.g. Ingestion Spikes Spike" />
                <div className="field">
                  <span>{t("alerts.app")}</span>
                  <CustomSelect
                    label={t("alerts.app")}
                    value={ruleAppId}
                    options={[
                      { value: "", label: "—" },
                      ...applications.map((app) => ({ value: app.id, label: app.name })),
                    ]}
                    onChange={setRuleAppId}
                  />
                </div>
                <div className="field">
                  <span>{t("alerts.sourceKind")}</span>
                  <CustomSelect
                    label={t("alerts.sourceKind")}
                    value={ruleSourceKind}
                    options={[
                      { value: "event_count", label: t("alerts.sourceEventCount") },
                      { value: "metric_average", label: t("alerts.sourceMetricAvg") },
                      { value: "metric_sum", label: t("alerts.sourceMetricSum") },
                      { value: "log_count", label: t("alerts.sourceLogCount") },
                      { value: "missing_data", label: t("alerts.sourceMissingData") },
                      { value: "change_rate", label: t("alerts.sourceChangeRate") },
                    ]}
                    onChange={setRuleSourceKind}
                  />
                </div>
                <div className="field">
                  <span>{t("alerts.operator")}</span>
                  <CustomSelect
                    label={t("alerts.operator")}
                    value={ruleOperator}
                    options={[
                      { value: "greater_or_equal", label: "≥ (Greater or equal)" },
                      { value: "greater_than", label: "> (Greater than)" },
                      { value: "less_or_equal", label: "≤ (Less or equal)" },
                      { value: "less_than", label: "< (Less than)" },
                      { value: "equal", label: "= (Equal)" },
                    ]}
                    onChange={setRuleOperator}
                  />
                </div>
                <label className="field">
                  <span>{t("alerts.threshold")}</span>
                  <input
                    type="number"
                    step="any"
                    required
                    value={ruleThreshold}
                    onChange={(e) => setRuleThreshold(e.target.value)}
                    className="input-select"
                  />
                </label>
                <div className="field">
                  <span>{t("alerts.window")}</span>
                  <CustomSelect
                    label={t("alerts.window")}
                    value={ruleWindow}
                    options={[
                      { value: "1", label: "1 min" },
                      { value: "5", label: "5 min" },
                      { value: "15", label: "15 min" },
                      { value: "60", label: "60 min" },
                    ]}
                    onChange={setRuleWindow}
                  />
                </div>
                <div className="field">
                  <span>{t("alerts.cooldown")}</span>
                  <CustomSelect
                    label={t("alerts.cooldown")}
                    value={ruleCooldown}
                    options={[
                      { value: "60", label: "1 min" },
                      { value: "300", label: "5 min" },
                      { value: "900", label: "15 min" },
                      { value: "3600", label: "1 hr" },
                    ]}
                    onChange={setRuleCooldown}
                  />
                </div>
              </div>

              <div className="flex items-center gap-2 pt-2">
                <button className="primary-button compact" disabled={!ruleAppId}>
                  {t("common.create")}
                </button>
                <button type="button" className="secondary-button compact" onClick={() => setCreatingRule(false)}>
                  {t("common.cancel")}
                </button>
              </div>
            </form>
          ) : null}

          <section className="rule-list">
            {rules.length === 0 ? (
              <div className="empty-state">
                <ShieldCheck aria-hidden="true" />
                <h2>{t("alerts.noRules")}</h2>
                <p>{t("alerts.noRulesDesc")}</p>
              </div>
            ) : (
              rules.map((rule) => {
                const isFiring = rule.lastState === "firing";
                return (
                  <article key={rule.id} className="flex items-center justify-between p-4 rounded-xl border border-[var(--border-soft)] bg-[var(--panel)]">
                    <div className="flex items-center gap-3">
                      <div className={`p-2.5 rounded-xl border ${isFiring ? "bg-[var(--danger)]/15 border-[var(--danger)] text-[var(--danger)]" : "bg-[var(--panel-strong)] border-[var(--border-soft)] text-[var(--signal)]"}`}>
                        {isFiring ? <Flame size={18} className="animate-pulse" /> : <BellRing size={18} />}
                      </div>
                      <div>
                        <h2 className="text-sm font-bold text-[var(--text)] m-0">{rule.name}</h2>
                        <p className="text-xs text-[var(--muted)] m-0 mt-0.5 font-mono">
                          {rule.sourceKind.replaceAll("_", " ")} · {rule.windowMinutes}m window · {rule.cooldownSeconds}s cooldown
                        </p>
                      </div>
                    </div>

                    <div className="flex items-center gap-3">
                      <span className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-semibold ${isFiring ? "bg-[var(--danger)]/20 text-[var(--danger)] border border-[var(--danger)]/30" : "bg-[var(--signal)]/15 text-[var(--signal)] border border-[var(--signal)]/30"}`}>
                        <span className={`h-1.5 w-1.5 rounded-full ${isFiring ? "bg-[var(--danger)]" : "bg-[var(--signal)]"}`} />
                        {isFiring ? t("alerts.stateFiring") : t("alerts.stateHealthy")}
                      </span>

                      <button
                        type="button"
                        onClick={() => deleteRule(rule.id)}
                        className="p-1.5 rounded-lg text-[var(--muted)] hover:text-[var(--danger)] hover:bg-[var(--danger)]/10 transition-colors cursor-pointer"
                        title={t("alerts.deleteRule")}
                      >
                        <Trash2 size={15} />
                      </button>
                    </div>
                  </article>
                );
              })
            )}
          </section>
        </div>
      ) : null}

      {/* ----------------- TAB 2: CHANNELS ----------------- */}
      {activeTab === "channels" ? (
        <div>
          {creatingChannel ? (
            <form className="rounded-2xl border border-[var(--border)] bg-[var(--panel)] p-6 mb-6 space-y-4 shadow-sm" onSubmit={createChannel}>
              <h3 className="text-sm font-bold text-[var(--text)] m-0">{t("alerts.newChannel")}</h3>
              <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 gap-4">
                <Field name="name" label={t("alerts.channelName")} placeholder="e.g. SRE Slack Channel" />
                <div className="field">
                  <span>{t("alerts.channelKind")}</span>
                  <CustomSelect
                    label={t("alerts.channelKind")}
                    value={channelKind}
                    options={[
                      { value: "webhook", label: "Custom Webhook (POST JSON)" },
                      { value: "feishu", label: "飞书群机器人 (Feishu Webhook)" },
                      { value: "dingtalk", label: "钉钉机器人 (DingTalk Webhook)" },
                      { value: "wecom", label: "企业微信机器人 (WeCom Webhook)" },
                      { value: "slack", label: "Slack Webhook" },
                      { value: "discord", label: "Discord Webhook" },
                      { value: "telegram", label: "Telegram Bot" },
                      { value: "email", label: "Email Alert Relay Webhook" },
                    ]}
                    onChange={setChannelKind}
                  />
                </div>
                {channelKind === "telegram" ? (
                  <>
                    <Field name="botToken" label="Telegram Bot Token" placeholder="123456:ABC-DEF..." required />
                    <Field name="chatId" label="Chat ID" placeholder="-100123456789 or @channel" required />
                  </>
                ) : (
                  <Field
                    name="url"
                    label={t("alerts.channelUrl")}
                    placeholder={
                      channelKind === "feishu"
                        ? "https://open.feishu.cn/open-apis/bot/v2/hook/..."
                        : channelKind === "dingtalk"
                        ? "https://oapi.dingtalk.com/robot/send?access_token=..."
                        : channelKind === "wecom"
                        ? "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=..."
                        : "https://hooks.slack.com/services/..."
                    }
                    required
                  />
                )}
              </div>

              <div className="flex items-center gap-2 pt-2">
                <button className="primary-button compact">
                  {t("common.create")}
                </button>
                <button type="button" className="secondary-button compact" onClick={() => setCreatingChannel(false)}>
                  {t("common.cancel")}
                </button>
              </div>
            </form>
          ) : null}

          <section className="space-y-3">
            {channels.length === 0 ? (
              <div className="empty-state">
                <Radio aria-hidden="true" />
                <h2>{t("alerts.noChannels")}</h2>
              </div>
            ) : (
              channels.map((channel) => (
                <article key={channel.id} className="flex items-center justify-between p-4 rounded-xl border border-[var(--border-soft)] bg-[var(--panel)]">
                  <div className="flex items-center gap-3">
                    <div className="p-2.5 rounded-xl border bg-[var(--panel-strong)] border-[var(--border-soft)] text-[var(--signal)]">
                      {channel.kind === "slack" || channel.kind === "discord" ? (
                        <MessageSquare size={18} />
                      ) : (
                        <Globe size={18} />
                      )}
                    </div>
                    <div>
                      <h2 className="text-sm font-bold text-[var(--text)] m-0">{channel.name}</h2>
                      <p className="text-xs text-[var(--muted)] m-0 mt-0.5 font-mono break-all">
                        <span className="uppercase text-[10px] font-bold px-1.5 py-0.5 rounded bg-[var(--input-bg)] mr-1.5">
                          {channel.kind}
                        </span>
                        {channel.config.url}
                      </p>
                    </div>
                  </div>

                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      disabled={testingChannelId === channel.id}
                      onClick={() => testChannel(channel.id)}
                      className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-semibold rounded-xl border border-[var(--border-soft)] hover:bg-[var(--panel-strong)] text-[var(--text)] transition-colors cursor-pointer"
                    >
                      {testingChannelId === channel.id ? (
                        <RefreshCw size={13} className="animate-spin" />
                      ) : (
                        <Send size={13} />
                      )}
                      <span>{t("alerts.channelTest")}</span>
                    </button>

                    <button
                      type="button"
                      onClick={() => deleteChannel(channel.id)}
                      className="p-2 rounded-xl text-[var(--muted)] hover:text-[var(--danger)] hover:bg-[var(--danger)]/10 transition-colors cursor-pointer"
                      title="Delete channel"
                    >
                      <Trash2 size={15} />
                    </button>
                  </div>
                </article>
              ))
            )}
          </section>
        </div>
      ) : null}

      {/* ----------------- TAB 3: DELIVERIES ----------------- */}
      {activeTab === "deliveries" ? (
        <div className="rounded-2xl border border-[var(--border-soft)] bg-[var(--panel)] p-6 shadow-sm">
          {deliveries.length === 0 ? (
            <p className="text-xs text-[var(--muted)] text-center py-6">
              {t("alerts.noDeliveries")}
            </p>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full text-xs text-left border-collapse">
                <thead>
                  <tr className="border-b border-[var(--border-soft)] text-[var(--muted)]">
                    <th className="py-2.5 px-3 font-semibold">Rule</th>
                    <th className="py-2.5 px-3 font-semibold">Channel</th>
                    <th className="py-2.5 px-3 font-semibold">{t("alerts.deliveryStatus")}</th>
                    <th className="py-2.5 px-3 font-semibold">{t("alerts.deliveryAttempts")}</th>
                    <th className="py-2.5 px-3 font-semibold">Time</th>
                    <th className="py-2.5 px-3 font-semibold">{t("alerts.deliveryError")}</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-[var(--border-soft)]">
                  {deliveries.map((del) => {
                    const isOk = del.status === "delivered";
                    return (
                      <tr key={del.id} className="hover:bg-[var(--panel-strong)]/50 transition-colors">
                        <td className="py-2.5 px-3 font-medium text-[var(--text)]">
                          {del.ruleName || del.ruleId.slice(0, 8)}
                        </td>
                        <td className="py-2.5 px-3 text-[var(--text)] font-medium">
                          {del.channelName || del.channelId.slice(0, 8)}
                        </td>
                        <td className="py-2.5 px-3">
                          <span
                            className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-full font-semibold text-[10px] ${
                              isOk
                                ? "bg-[var(--signal)]/15 text-[var(--signal)]"
                                : "bg-[var(--danger)]/15 text-[var(--danger)]"
                            }`}
                          >
                            {isOk ? <CheckCircle2 size={12} /> : <AlertTriangle size={12} />}
                            {del.status}
                          </span>
                        </td>
                        <td className="py-2.5 px-3 font-mono text-[var(--muted)]">
                          {del.attempts}
                        </td>
                        <td className="py-2.5 px-3 text-[var(--muted)]">
                          {new Date(del.createdAt).toLocaleString()}
                        </td>
                        <td className="py-2.5 px-3 text-[var(--danger)] font-mono text-[11px] max-w-xs truncate">
                          {del.lastError || "—"}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
        </div>
      ) : null}
    </div>
  );
}

function Field(props: InputHTMLAttributes<HTMLInputElement> & { label: string }) {
  const { label, ...input } = props;
  return (
    <label className="field">
      <span>{label}</span>
      <input {...input} required className="input-select" />
    </label>
  );
}
