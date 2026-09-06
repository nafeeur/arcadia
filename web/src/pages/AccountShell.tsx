/* Account-level shell: hosts Profile / Administration.
   Not tied to any KB, so no KB switcher, no tab nav — just the wordmark, back link, and user menu. */
import { useQuery } from "@tanstack/react-query";
import { Link, Outlet, useNavigate } from "@tanstack/react-router";
import { usePageTitle } from "../useTitle";
import {
  KeyRound,
  Layers,
  ShieldCheck,
  UserRound,
} from "lucide-react";
import { api, ApiError } from "../api";
import { S } from "../i18n";
import {
  RAIL_CLS,
  rowClass,
  SectionMark,
} from "../ui";
import { ServerDown } from "./ServerDown";
import { HeaderActions } from "./HeaderActions";

export function AccountShell() {
  const navigate = useNavigate();
  const me = useQuery({ queryKey: ["me"], queryFn: api.me });
  const health = useQuery({ queryKey: ["health"], queryFn: api.health, staleTime: Infinity });
  // Title: `Arcadia | Persona` — the account area shares one name, not broken down per page
  usePageTitle(S.app.name, S.account.titleTag);

  if (me.isPending) {
    return (
      <div className="min-h-screen flex items-center justify-center text-ink-3 text-body">
        {S.nav.loading}
      </div>
    );
  }
  if (me.isError) {
    if (me.error instanceof ApiError && me.error.status === 401) {
      navigate({ to: "/login" });
      return null;
    }
    return <ServerDown />;
  }

  const rail = rowClass(false, "nav");
  const railActive = rowClass(true, "nav");

  return (
    <div className="h-screen flex flex-col overflow-hidden u-arrive">
      {/* Header mirrors the Docs page: section wordmark (click to go home) + back + GitHub·version + user */}
      {/* px-8 matches the app header's own inset — that top-right cluster shouldn't shift between pages */}
      <header className="glass-strong relative z-40 border-x-0 border-t-0 h-14 shrink-0 flex items-center px-8">
        <SectionMark text={S.account.brand} title={S.docs.backTitle} />
        <HeaderActions
          link={{ to: "/", label: S.account.backToApp }}
          version={health.data?.version}
          user={me.data}
        />
      </header>

      <div className="flex-1 min-h-0 flex">
        {/* Account nav rail (two items, one more for admins) */}
        <aside className={`${RAIL_CLS} p-3 space-y-1`}>
          {/* exact: /account is a prefix of /account/kbs, so default prefix matching would light up both */}
          <Link
            to="/account"
            activeOptions={{ exact: true }}
            className={rail}
            activeProps={{ className: railActive }}
          >
            <UserRound size={14} />
            {S.account.profile}
          </Link>
          <Link to="/account/kbs" className={rail} activeProps={{ className: railActive }}>
            <Layers size={14} />
            {S.account.kbsNav}
          </Link>
          <Link to="/account/tokens" className={rail} activeProps={{ className: railActive }}>
            <KeyRound size={14} />
            {S.account.tokensNav}
          </Link>
          {me.data.is_admin && <Link to="/identity" className={rail} activeProps={{className:railActive}}><ShieldCheck size={14}/>{S.arcadia.sso}</Link>}
          {me.data.is_admin && (
            <Link to="/admin" className={rail} activeProps={{ className: railActive }}>
              <ShieldCheck size={14} />
              {S.account.administration}
            </Link>
          )}
        </aside>
        <main className="flex-1 min-w-0 overflow-y-auto u-scroll">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
