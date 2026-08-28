import { FormEvent, useRef, useState, type InputHTMLAttributes, type Ref } from "react";
import { ArrowLeft, ArrowRight, KeyRound, RadioTower, ShieldCheck } from "lucide-react";
import { useTranslation } from "react-i18next";
import { motion } from "motion/react";
import type { User } from "../App";
import { LocaleSelect, ThemeSelect } from "../components/PreferenceSelects";
import { ApiError, api } from "../lib/api";

type Challenge = { id: string; prompt: string };

type LoginResponse = User | { requires2fa: true; tempToken: string };

export function LoginPage({ onLogin }: { onLogin: (user: User) => void }) {
  const { t } = useTranslation();
  const [error, setError] = useState("");
  const [challenge, setChallenge] = useState<Challenge | null>(null);
  const [twoFactorToken, setTwoFactorToken] = useState<string | null>(null);
  const [twoFactorCode, setTwoFactorCode] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const challengeInput = useRef<HTMLInputElement>(null);
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
          challengeResponse: form.get("challengeResponse"),
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
        cause.body.challengeId &&
        cause.body.challengePrompt
      ) {
        setChallenge({ id: cause.body.challengeId, prompt: cause.body.challengePrompt });
        queueMicrotask(() => challengeInput.current?.focus());
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
    <main className="relative flex min-h-screen w-full items-center justify-center p-4 bg-[radial-gradient(ellipse_at_center,var(--panel-strong)_0%,var(--bg)_100%)] overflow-hidden">
      {/* Background Concentric Radar Rings */}
      <div className="pointer-events-none absolute flex items-center justify-center text-[var(--signal)]/15">
        <div className="absolute h-[640px] w-[640px] rounded-full border border-current opacity-30 animate-pulse" />
        <div className="absolute h-[460px] w-[460px] rounded-full border border-current opacity-50" />
        <div className="absolute h-[280px] w-[280px] rounded-full border border-current opacity-70" />
        <RadioTower size={48} className="text-[var(--signal)]/25" />
      </div>

      {/* Glassmorphic Login Card */}
      <motion.section
        initial={{ opacity: 0, scale: 0.95, y: 14 }}
        animate={{ opacity: 1, scale: 1, y: 0 }}
        transition={{ type: "spring", stiffness: 420, damping: 32 }}
        className="relative z-10 w-full max-w-md rounded-3xl border border-[var(--border)] bg-[var(--bg-elevated)]/90 p-7 sm:p-8 shadow-2xl backdrop-blur-3xl transition-all"
        aria-labelledby="login-title"
      >
        {/* Card Header: Brand + Compact Theme/Locale Controls */}
        <div className="flex items-center justify-between border-b border-[var(--border-soft)] pb-4 mb-6">
          <div className="flex items-center gap-2.5">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-[var(--signal)] text-white shadow-sm flex-shrink-0">
              <span className="h-3 w-3 rounded-sm bg-white" />
            </div>
            <div>
              <strong className="block text-sm font-bold tracking-tight text-[var(--text)] leading-none">Sonde</strong>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <ThemeSelect compact />
            <LocaleSelect compact />
          </div>
        </div>

        {twoFactorToken ? (
          /* 2FA Step Form */
          <div>
            <div className="mb-6">
              <div className="flex items-center gap-2 text-[var(--signal)] mb-1">
                <KeyRound size={20} />
                <span className="text-xs font-bold uppercase tracking-wider">Two-Factor Auth</span>
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
                  className="w-full text-center text-2xl tracking-[0.5em] font-mono font-bold rounded-xl border border-[var(--border)] bg-[var(--input-bg)] px-3.5 py-3 text-[var(--text)] outline-none focus:border-[var(--signal)] focus:ring-2 focus:ring-[var(--signal)]/20 transition-all"
                />
              </label>

              {error ? (
                <p className="text-xs text-[var(--danger)] bg-[var(--danger-subtle)] border border-[var(--danger)]/20 p-2.5 rounded-xl font-medium" role="alert">
                  {error}
                </p>
              ) : null}

              <button
                type="submit"
                className="w-full flex items-center justify-center gap-2 rounded-xl bg-[var(--signal)] px-4 py-3 text-sm font-bold text-white shadow-lg shadow-[var(--signal)]/25 hover:brightness-110 active:scale-[0.98] transition-all cursor-pointer disabled:opacity-50 mt-2"
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
          /* Primary Username & Password Form */
          <div>
            {/* Title & System Status */}
            <div className="mb-6">
              <h1 id="login-title" className="text-xl sm:text-2xl font-bold tracking-tight text-[var(--text)]">
                {t("login.title")}
              </h1>
              <p className="mt-1.5 flex items-center gap-2 text-xs text-[var(--muted)]">
                <span className="h-2 w-2 rounded-full bg-[var(--signal)] shadow-[0_0_8px_var(--signal)]" />
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
                <div className="rounded-xl border border-[var(--amber)]/30 bg-[var(--amber-subtle)] p-3 text-xs space-y-2">
                  <div className="flex items-center gap-2 text-[var(--amber)] font-bold">
                    <ShieldCheck size={16} aria-hidden="true" />
                    <span>{t("login.verification")}</span>
                  </div>
                  <p className="text-[var(--muted)]">{t("login.verificationHint")}</p>
                  <output className="block font-mono font-bold text-center text-sm py-1 bg-[var(--panel-strong)] rounded-lg border border-[var(--border)]">
                    {challenge.prompt}
                  </output>
                  <Field
                    ref={challengeInput}
                    label={t("login.verificationResponse")}
                    name="challengeResponse"
                    autoComplete="off"
                  />
                </div>
              ) : null}

              {error ? (
                <p id="login-error" className="text-xs text-[var(--danger)] bg-[var(--danger-subtle)] border border-[var(--danger)]/20 p-2.5 rounded-xl font-medium" role="alert">
                  {error}
                </p>
              ) : null}

              <button
                type="submit"
                className="w-full flex items-center justify-center gap-2 rounded-xl bg-[var(--signal)] px-4 py-3 text-sm font-bold text-white shadow-lg shadow-[var(--signal)]/25 hover:brightness-110 active:scale-[0.98] transition-all cursor-pointer disabled:opacity-50 mt-2"
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
        className="w-full rounded-xl border border-[var(--border)] bg-[var(--input-bg)] px-3.5 py-2.5 text-xs sm:text-sm text-[var(--text)] outline-none focus:border-[var(--signal)] focus:ring-2 focus:ring-[var(--signal)]/20 transition-all"
      />
    </label>
  );
};
