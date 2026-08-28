import { FormEvent, useState, type InputHTMLAttributes, type ReactNode } from "react";
import { CheckCircle2, Database, Eye, EyeOff, KeyRound, LockKeyhole } from "lucide-react";
import { useTranslation } from "react-i18next";
import { LocaleSelect, ThemeSelect } from "../components/PreferenceSelects";
import { ApiError, api } from "../lib/api";

type DatabaseType = "sqlite" | "postgresql" | "mysql";

export function SetupPage({ onComplete }: { onComplete: () => void }) {
  const { t, i18n } = useTranslation();
  const [databaseType, setDatabaseType] = useState<DatabaseType>("sqlite");
  const [showPassword, setShowPassword] = useState(false);
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setError(""); setSubmitting(true);
    const form = new FormData(event.currentTarget);
    try {
      await api("/api/v1/setup/complete", {
        method: "POST",
        body: JSON.stringify({
          databaseType,
          databaseUrl: databaseType === "sqlite" ? null : form.get("databaseUrl"),
          locale: i18n.language,
          timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
          email: form.get("email"), username: form.get("username"), password: form.get("password"),
          secureCookie: location.protocol === "https:"
        })
      });
      onComplete();
    } catch (cause) {
      if (cause instanceof ApiError && cause.body.code === "password_length") setError(t("setup.passwordLengthError"));
      else if (cause instanceof ApiError && cause.body.code === "password_blocked") setError(t("setup.passwordBlockedError"));
      else setError(cause instanceof Error ? cause.message : t("common.error"));
    }
    finally { setSubmitting(false); }
  }

  const databaseUrl = databaseType === "postgresql"
    ? "postgres://sonde:password@localhost/sonde"
    : "mysql://sonde:password@localhost/sonde";

  return <main className="auth-layout">
    <section className="auth-context" aria-labelledby="setup-title">
      <div className="brand-lockup inverse"><div className="sonde-mark" aria-hidden="true"><span /></div><div><strong>Sonde</strong><small>{t("brand.tagline")}</small></div></div>
      <div className="context-copy"><span className="eyebrow">{t("setup.eyebrow")}</span><h1 id="setup-title">{t("setup.title")}</h1><p>{t("setup.description")}</p></div>
      <ol className="commissioning-list"><li className="active"><Database aria-hidden="true" /><span>01</span><strong>{t("setup.storageStep")}</strong></li><li><KeyRound aria-hidden="true" /><span>02</span><strong>{t("setup.adminStep")}</strong></li><li><LockKeyhole aria-hidden="true" /><span>03</span><strong>{t("setup.sealStep")}</strong></li></ol>
      <div className="grid-field" aria-hidden="true" />
    </section>
    <section className="auth-panel">
      <div className="auth-preferences"><ThemeSelect compact /><LocaleSelect compact /></div>
      <form onSubmit={(event) => void submit(event)} aria-describedby={error ? "setup-error" : undefined}>
        <div className="form-heading"><span>01 — 03</span><h2>{t("setup.title")}</h2></div>
        <fieldset><legend>{t("setup.engine")}</legend><div className="segmented-control">{(["sqlite", "postgresql", "mysql"] as DatabaseType[]).map((type) => <button type="button" aria-pressed={databaseType === type} key={type} onClick={() => setDatabaseType(type)}>{type}</button>)}</div></fieldset>
        {databaseType === "sqlite" ? <p className="managed-database"><Database aria-hidden="true" />{t("setup.sqliteManaged")}</p> : <Field label={t("setup.database")} name="databaseUrl" defaultValue={databaseUrl} key={databaseType} autoComplete="url" />}
        <div className="two-columns"><Field label={t("setup.email")} name="email" type="email" autoComplete="email" /><Field label={t("setup.username")} name="username" autoComplete="username" /></div>
        <label className="field"><span>{t("setup.password")}</span><div className="input-wrap"><input name="password" type={showPassword ? "text" : "password"} minLength={15} maxLength={128} autoComplete="new-password" required /><button type="button" className="input-action" onClick={() => setShowPassword((value) => !value)} aria-label={showPassword ? t("common.hidePassword") : t("common.showPassword")}>{showPassword ? <EyeOff aria-hidden="true" /> : <Eye aria-hidden="true" />}</button></div><small>{t("setup.passwordHint")}</small></label>
        {error ? <p id="setup-error" className="form-error" role="alert">{error}</p> : null}
        <button className="primary-button" disabled={submitting}>{submitting ? t("common.loading") : t("setup.submit")}<CheckCircle2 size={18} aria-hidden="true" /></button>
      </form>
    </section>
  </main>;
}

function Field({ label, icon, ...props }: { label: string; icon?: ReactNode } & InputHTMLAttributes<HTMLInputElement>) {
  return <label className="field"><span>{label}</span><div className="input-wrap">{icon}<input {...props} required /></div></label>;
}
