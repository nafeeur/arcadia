-- Explicit subject binding avoids silently linking accounts by an email claim.
CREATE TABLE arcadia_oidc_identities (
 issuer TEXT NOT NULL,
 subject TEXT NOT NULL,
 user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
 PRIMARY KEY(issuer,subject), UNIQUE(issuer,user_id)
);
CREATE TABLE arcadia_oidc_flows (
 state TEXT PRIMARY KEY,
 nonce TEXT NOT NULL,
 verifier TEXT NOT NULL,
 expires_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX arcadia_oidc_expiry_idx ON arcadia_oidc_flows(expires_at);
