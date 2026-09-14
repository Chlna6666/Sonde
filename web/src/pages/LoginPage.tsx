import { FormEvent, useRef, useState, type InputHTMLAttributes, type Ref } from "react";
import { ArrowLeft, ArrowRight, KeyRound, ShieldCheck } from "lucide-react";
import { useTranslation } from "react-i18next";
import { motion } from "motion/react";
import type { User } from "../App";
import { LocaleSelect, ThemeSelect } from "../components/PreferenceSelects";
import { ApiError, api } from "../lib/api";

type Challenge = { id: string };

type LoginResponse = User | { requires2fa: true; tempToken: string };

export function LoginPage({ onLogin }: { onLogin: (user: User) => void }) {
  const { t } = useTranslation();
  const [error, setError] = useState("");
  const [challenge, setChallenge] = useState<Challenge | null>(null);
  const [twoFactorToken, setTwoFactorToken] = useState<string | null>(null);
  const [twoFactorCode, setTwoFactorCode] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const twoFactorInput = useRef<HTMLInputElement>(null);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSubmitting(true);
    setError("");
    const form = new FormData(event.currentTarget);
    const identifier = form.get("username") || form.get("email");
    try {
      const res = await api<LoginResponse>("/api/v1/auth/login", {
        method: "POST",
        body: JSON.stringify({
          username: identifier,
          password: form.get("password"),
          website: form.get("website"),
          challengeId: challenge?.id,
        }),
      });

      if ("requires2fa" in res && res.requires2fa) {
        setTwoFactorToken(res.tempToken);
        setTwoFactorCode("");
        queueMicrotask(() => twoFactorInput.current?.focus());
        return;
      }

      setChallenge(null);
      onLogin(res as User);
    } catch (cause) {
      if (
        cause instanceof ApiError &&
        cause.body.code === "challenge_required" &&
        cause.body.challengeId
      ) {
        setChallenge({ id: cause.body.challengeId });
      }
      if (
        cause instanceof ApiError &&
        (cause.body.code === "invalid_credentials" ||
          cause.body.code === "unauthorized" ||
          cause.body.code === "challenge_required")
      ) {
        setError(t("login.invalidCredentials"));
      } else if (cause instanceof ApiError && cause.body.code === "rate_limited") {
        setError(t("login.rateLimited"));
      } else {
        setError(cause instanceof Error ? cause.message : t("common.error"));
      }
    } finally {
      setSubmitting(false);
    }
  }

  async function submitTwoFactor(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!twoFactorToken) return;
    setSubmitting(true);
    setError("");
    try {
      const user = await api<User>("/api/v1/auth/2fa/verify", {
        method: "POST",
        body: JSON.stringify({
          tempToken: twoFactorToken,
          code: twoFactorCode.trim(),
        }),
      });
      setTwoFactorToken(null);
      onLogin(user);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("common.error"));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <main className="relative flex min-h-screen w-full items-center justify-center p-4 overflow-hidden">
      <motion.section
        initial={{ opacity: 0, y: 12 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.28, ease: [0.22, 1, 0.36, 1] }}
        className="relative z-10 w-full max-w-md rounded-[var(--radius-xl)] border border-[var(--border)] bg-[var(--bg-elevated)] p-7 sm:p-8"
        aria-labelledby="login-title"
      >
        <div className="flex items-center justify-between border-b border-[var(--border-soft)] pb-4 mb-6">
          <div className="brand-lockup">
            <div className="sonde-mark" aria-hidden="true">
              <span />
            </div>
            <div>
              <strong>Sonde</strong>
              <small>Telemetry</small>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <ThemeSelect compact />
            <LocaleSelect compact />
          </div>
        </div>

        {twoFactorToken ? (
          <div>
            <div className="mb-6">
              <div className="flex items-center gap-2 text-[var(--signal)] mb-1">
                <KeyRound size={16} />
                <span className="eyebrow">{t("login.twoFactorTitle")}</span>
              </div>
              <h1 id="login-title" className="text-xl sm:text-2xl font-bold tracking-tight text-[var(--text)]">
                {t("login.twoFactorTitle")}
              </h1>
              <p className="mt-1.5 text-xs text-[var(--muted)]">
                {t("login.twoFactorPrompt")}
              </p>
            </div>

            <form onSubmit={(event) => void submitTwoFactor(event)} className="space-y-4">
              <label className="block space-y-1.5">
                <span className="block text-xs font-semibold text-[var(--muted)]">{t("login.twoFactorCode")}</span>
                <input
                  ref={twoFactorInput}
                  type="text"
                  pattern="[0-9]*"
                  inputMode="numeric"
                  maxLength={6}
                  autoComplete="one-time-code"
                  required
                  placeholder="123456"
                  value={twoFactorCode}
                  onChange={(e) => setTwoFactorCode(e.target.value.replace(/\D/g, "").slice(0, 6))}
                  className="w-full text-center text-2xl tracking-[0.4em] font-mono font-bold rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--input-bg)] px-3.5 py-3 text-[var(--text)] outline-none focus:border-[var(--signal)] focus:ring-2 focus:ring-[var(--signal)]/20 transition-colors"
                />
              </label>

              {error ? (
                <p className="text-xs text-[var(--danger)] bg-[var(--danger-subtle)] border border-[var(--danger)]/20 p-2.5 rounded-[var(--radius-md)] font-medium" role="alert">
                  {error}
                </p>
              ) : null}

              <button
                type="submit"
                className="primary-button"
                disabled={submitting || twoFactorCode.length < 6}
              >
                {submitting ? t("common.loading") : t("login.twoFactorSubmit")}
                <ArrowRight size={16} aria-hidden="true" />
              </button>

              <button
                type="button"
                onClick={() => {
                  setTwoFactorToken(null);
                  setError("");
                }}
                className="w-full flex items-center justify-center gap-1.5 text-xs text-[var(--muted)] hover:text-[var(--text)] py-2 transition-colors cursor-pointer"
              >
                <ArrowLeft size={14} />
                <span>{t("common.cancel")}</span>
              </button>
            </form>
          </div>
        ) : (
          <div>
            <div className="mb-6">
              <h1 id="login-title" className="text-xl sm:text-2xl font-bold tracking-tight text-[var(--text)]">
                {t("login.title")}
              </h1>
              <p className="mt-1.5 flex items-center gap-2 text-xs text-[var(--muted)]">
                <span className="h-1.5 w-1.5 rounded-full bg-[var(--signal)]" />
                {t("login.available")}
              </p>
            </div>

            <form onSubmit={(event) => void submit(event)} className="space-y-4" aria-describedby={error ? "login-error" : undefined}>
              <Field label={t("login.username")} name="username" type="text" autoComplete="username" />
              <Field label={t("login.password")} name="password" type="password" autoComplete="current-password" />
              <label className="sr-only" aria-hidden="true">
                Website<input name="website" tabIndex={-1} autoComplete="off" />
              </label>

              {challenge ? (
                <div className="rounded-[var(--radius-md)] border border-[var(--amber)]/30 bg-[var(--amber-subtle)] p-3 text-xs space-y-2">
                  <div className="flex items-center gap-2 text-[var(--amber)] font-bold">
                    <ShieldCheck size={16} aria-hidden="true" />
                    <span>{t("login.verification")}</span>
                  </div>
                  <p className="text-[var(--muted)]">{t("login.verificationHint")}</p>
                </div>
              ) : null}

              {error ? (
                <p id="login-error" className="text-xs text-[var(--danger)] bg-[var(--danger-subtle)] border border-[var(--danger)]/20 p-2.5 rounded-[var(--radius-md)] font-medium" role="alert">
                  {error}
                </p>
              ) : null}

              <button
                type="submit"
                className="primary-button"
                disabled={submitting}
              >
                {submitting ? t("common.loading") : t("login.submit")}
                <ArrowRight size={16} aria-hidden="true" />
              </button>
            </form>
          </div>
        )}
      </motion.section>
    </main>
  );
}

const Field = function Field({
  label,
  ref,
  ...props
}: { label: string } & InputHTMLAttributes<HTMLInputElement> & { ref?: Ref<HTMLInputElement> }) {
  return (
    <label className="block space-y-1.5">
      <span className="block text-xs font-semibold text-[var(--muted)]">{label}</span>
      <input
        {...props}
        required
        ref={ref}
        className="w-full rounded-[var(--radius-md)] border border-[var(--border)] bg-[var(--input-bg)] px-3.5 py-2.5 text-xs sm:text-sm text-[var(--text)] outline-none focus:border-[var(--signal)] focus:ring-2 focus:ring-[var(--signal)]/20 transition-colors"
      />
    </label>
  );
};
