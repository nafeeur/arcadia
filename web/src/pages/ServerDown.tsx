/* Punishment pages: 500 server unreachable / 404 lost city.
   Reuses the login page scene as background — even errors keep up appearances (in self-hosted setups this page is often ops' first stop). */
import { Home, RefreshCw } from "lucide-react";
import { Link } from "@tanstack/react-router";
import { S } from "../i18n";
import {
  Button,
  Wordmark,
} from "../ui";
import { usePageTitle } from "../useTitle";
import { LoginScene } from "./LoginScene";

function PunishmentPage({
  message,
  children,
}: {
  message: string;
  children: React.ReactNode;
}) {
  return (
    <div className="min-h-screen flex items-center justify-center px-4">
      <LoginScene />
      <div className="relative z-10 text-center u-rise">
        <h1 className="u-wordmark-hero font-normal">
          <Wordmark />
        </h1>
        <p className="u-balance mt-4 text-body text-ink-2">{message}</p>
        <div className="mt-6 flex items-center justify-center gap-4">
          {children}
          <a
            href="/docs/arcadia"
            target="_blank"
            rel="noreferrer"
            className="u-link text-small"
          >
            {S.arcadia.guideTitle}
          </a>
        </div>
      </div>
    </div>
  );
}

export function ServerDown() {
  usePageTitle(S.app.name, "Punishment 500");
  return (
    <PunishmentPage message={S.nav.serverUnreachable}>
      <Button variant="secondary" size="sm" className="flex items-center gap-2"
        onClick={() => window.location.reload()}
      >
        <RefreshCw size={12} />
        {S.nav.refresh}
      </Button>
    </PunishmentPage>
  );
}

export function NotFound() {
  usePageTitle(S.app.name, "Punishment 404");
  return (
    <PunishmentPage message={S.nav.notFound}>
      <Link to="/" className="u-btn u-btn-ghost px-4 py-2 text-small flex items-center gap-2">
        <Home size={12} />
        {S.nav.returnHome}
      </Link>
    </PunishmentPage>
  );
}
