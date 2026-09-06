/* The right-hand cluster in the header: the first link (Docs in the app, back-to-app
   on Docs/account pages) + GitHub·version pill + alert bell + user menu. All three
   headers share this one component — they used to each assemble it separately, off
   by a few pixels of padding and spacing each time, so the top-right corner jittered on page changes. */
import { Link } from "@tanstack/react-router";
import type { ReactNode } from "react";
import type { User } from "../api";
import { S } from "../i18n";
import { GithubMark } from "../ui";
import { AlertBell } from "./AlertBell";
import { UserMenu } from "./UserMenu";

export function HeaderActions({
  link,
  version,
  user,
  signedOut,
}: {
  /** The leftmost link: where it goes, what it says */
  link: { to: "/" | "/docs"; label: string };
  version?: string;
  user: User | null | undefined;
  /** What to put where the bell and user menu would be when signed out (the Docs page's "Sign in"); nothing by default */
  signedOut?: ReactNode;
}) {
  return (
    <div className="ml-auto flex items-center gap-3">
      {/* The project entry is a pair (link + GitHub·version), sitting closer together than the gap between groups */}
      <div className="flex items-center gap-2">
        <Link to={link.to} className="u-navlink">
          {link.label}
        </Link>
        <a
          href={S.login.githubUrl}
          target="_blank"
          rel="noreferrer"
          title={S.arcadia.upstreamSource}
          className="u-pill"
        >
          <GithubMark size={13} />
          {version && <span className="u-num text-fine">v{version}</span>}
        </a>
      </div>
      {user ? (
        <>
          {/* The alert badge follows the person, not the page: knowledge bases keep running while you're reading docs or editing your account */}
          <AlertBell />
          <UserMenu user={user} />
        </>
      ) : (
        signedOut
      )}
    </div>
  );
}
