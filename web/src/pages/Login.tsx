import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";

/* lucide dropped its brand icons, so the GitHub mark is inlined here (official mark path, fill=currentColor) */
function GithubMark({ size = 16 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="currentColor"
      aria-hidden
    >
      <path d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12" />
    </svg>
  );
}
import { api, ApiError, request } from "../api";
import { S } from "../i18n";
import { Button, Input, Segmented, Wordmark } from "../ui";

import { usePageTitle } from "../useTitle";

export function Login() {
  usePageTitle(S.app.name, S.login.signIn);
  const navigate = useNavigate();
  const sso = useQuery({
    queryKey: ["sso-status"],
    queryFn: () => request<{ enabled: boolean }>("/api/v1/auth/oidc/status"),
  });
  const [mode, setMode] = useState<"login" | "register">("login");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [leaving, setLeaving] = useState(false);

  const mutation = useMutation({
    mutationFn: async () => {
      if (mode === "login") return api.login(email, password);
      return api.register(email, password, displayName);
    },
    onSuccess: () => {
      // Curtain call: the card rises and fades out, the monument zooms through, then we land on the graph home
      setLeaving(true);
      window.setTimeout(() => navigate({ to: "/" }), 650);
    },
  });

  const error =
    mutation.error instanceof ApiError
      ? mutation.error.message
      : mutation.error
        ? S.login.networkError
        : null;

  return (
    <div className="arc-login">
      <section className="arc-login-story">
        <p className="arc-eyebrow">{S.arcadia.loginLabel}</p>
        <h2>{S.arcadia.loginTitle}</h2>
        <p>{S.arcadia.loginBody}</p>
        <div className="arc-login-caption">{S.arcadia.edition}</div>
      </section>
      {/* Monument transformation backdrop: planet → ring city → city plain → wavering monolith */}

      <div className={`arc-login-form ${leaving ? "u-depart" : ""}`}>
        <div className="mb-8 text-center u-rise">
          <h1 className="u-wordmark-hero font-normal">
            <Wordmark />
          </h1>
          <p className="mt-2 text-body text-ink-2">
            {S.app.tagline}
            <span className="ml-2 text-fine">{S.app.taglineSource}</span>
          </p>
        </div>

        <div
          className="u-card-opaque rounded-xl p-6 u-rise"
          style={{ animationDelay: "90ms" }}
        >
          <Segmented
            fill
            className="mb-6"
            value={mode}
            onChange={setMode}
            options={(["login", "register"] as const).map((m) => ({
              value: m,
              label: m === "login" ? S.login.signIn : S.login.signUp,
            }))}
          />

          <form
            className="space-y-3"
            onSubmit={(e) => {
              e.preventDefault();
              mutation.mutate();
            }}
          >
            {mode === "register" && (
              <Input
                className="w-full"
                placeholder={S.login.displayName}
                value={displayName}
                onChange={(e) => setDisplayName(e.target.value)}
                required
              />
            )}
            <Input
              type="email"
              className="w-full"
              placeholder={S.login.email}
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              required
            />
            <Input
              type="password"
              className="w-full"
              placeholder={S.login.password}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
              minLength={8}
            />
            {error && <p className="text-body text-danger">{error}</p>}
            <Button
              variant="primary"
              size="md"
              className="w-full"
              type="submit"
              disabled={mutation.isPending || leaving}
            >
              {mutation.isPending || leaving
                ? S.login.submitting
                : mode === "login"
                  ? S.login.signIn
                  : S.login.createAccount}
            </Button>
          </form>
          {sso.data?.enabled && (
            <a
              className="arc-primary-link arc-sso-login"
              href="/api/v1/auth/oidc/start"
            >
              {S.arcadia.ssoLogin}
            </a>
          )}
        </div>

        {/* Footer: the usual consent sentence with inline terms/privacy links + GitHub entry */}
        <div
          className="mt-6 text-center u-rise"
          style={{ animationDelay: "180ms" }}
        >
          <p className="u-balance text-fine leading-relaxed text-ink-3">
            {S.login.agreePrefix}
            <Link to="/terms" className="u-link whitespace-nowrap">
              {S.legal.termsTitle}
            </Link>
            {S.login.agreeAnd}
            <Link to="/privacy" className="u-link whitespace-nowrap">
              {S.legal.privacyTitle}
            </Link>
            {S.login.agreeSuffix}
          </p>
          <a
            href={S.login.githubUrl}
            target="_blank"
            rel="noreferrer"
            title={S.arcadia.upstreamSource}
            className="u-hover-ink mt-3 inline-flex text-ink-3"
          >
            <GithubMark size={16} />
          </a>
        </div>
      </div>
    </div>
  );
}
