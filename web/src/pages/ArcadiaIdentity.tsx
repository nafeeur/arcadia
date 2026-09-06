import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { api, request } from "../api";
import { S } from "../i18n";
import { Button, Input, NativeSelect } from "../ui";
const A = S.arcadia;
interface Identity {
  user_id: string;
  subject: string;
  email: string;
}
export function ArcadiaIdentity() {
  const qc = useQueryClient();
  const [user, setUser] = useState("");
  const [subject, setSubject] = useState("");
  const me = useQuery({ queryKey: ["me"], queryFn: api.me });
  const status = useQuery({
    queryKey: ["sso-status"],
    queryFn: () => request<{ enabled: boolean }>("/api/v1/auth/oidc/status"),
  });
  const identities = useQuery({
    queryKey: ["oidc-identities"],
    queryFn: () =>
      request<{
        issuer: string;
        client_id: string;
        redirect_uri: string;
        identities: Identity[];
      }>("/api/v1/admin/oidc/identities"),
    enabled: !!status.data?.enabled && !!me.data?.is_admin,
  });
  const users = useQuery({
    queryKey: ["org-users"],
    queryFn: api.orgUsers,
    enabled: !!me.data?.is_admin,
  });
  const link = useMutation({
    mutationFn: () =>
      request("/api/v1/admin/oidc/identities", {
        method: "POST",
        body: JSON.stringify({ user_id: user, subject }),
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["oidc-identities"] });
      setSubject("");
    },
  });
  const unlink = useMutation({
    mutationFn: (id: string) =>
      request(`/api/v1/admin/oidc/identities/${id}`, { method: "DELETE" }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["oidc-identities"] }),
  });
  const error = identities.error ?? users.error ?? link.error ?? unlink.error;
  return (
    <div className="arc-page">
      <header className="arc-heading">
        <div>
          <p className="arc-eyebrow">{A.edition}</p>
          <h1>{A.sso}</h1>
          <p>{A.ssoIntro}</p>
        </div>
      </header>
      {!me.data?.is_admin ? (
        <p>{me.isPending ? A.loading : A.permission}</p>
      ) : !status.data?.enabled ? (
        <p>{status.isPending ? A.loading : A.ssoDisabled}</p>
      ) : (
        <>
          <p className="arc-muted">
            {A.issuer}: {identities.data?.issuer}
          </p>
          <p className="arc-muted">{A.ssoNote}</p>
          <form
            className="arc-panel arc-proposal"
            onSubmit={(e) => {
              e.preventDefault();
              link.mutate();
            }}
          >
            <div className="arc-form-grid">
              <label>
                {A.account}
                <NativeSelect
                  value={user}
                  onChange={(e) => setUser(e.target.value)}
                  required
                >
                  <option value="">{A.selectAccount}</option>
                  {users.data?.map((u) => (
                    <option key={u.id} value={u.id}>
                      {u.email}
                    </option>
                  ))}
                </NativeSelect>
              </label>
              <label>
                {A.subject}
                <Input
                  required
                  maxLength={512}
                  value={subject}
                  onChange={(e) => setSubject(e.target.value)}
                />
              </label>
            </div>
            <Button variant="primary" type="submit" busy={link.isPending}>
              {A.linkIdentity}
            </Button>
          </form>
          <div className="arc-panel arc-dependent">
            {identities.data?.identities.map((i) => (
              <div className="arc-list-row" key={i.user_id}>
                <span>
                  <strong>{i.email}</strong>
                  <small>{i.subject}</small>
                </span>
                <Button
                  busy={unlink.isPending}
                  onClick={() => unlink.mutate(i.user_id)}
                >
                  {A.unlinkIdentity}
                </Button>
              </div>
            ))}
          </div>
        </>
      )}
      {error && (
        <p className="arc-error" role="alert">
          {error instanceof Error ? error.message : A.error}
        </p>
      )}
    </div>
  );
}
