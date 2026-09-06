-- Retained chunk text is the authority for historical lexical retrieval.
CREATE INDEX chunks_history_lexical_idx ON chunks USING gin (to_tsvector('simple', text));

CREATE TABLE arcadia_traces (
 id UUID PRIMARY KEY,
 kb_id UUID NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
 user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 message_id UUID NOT NULL UNIQUE REFERENCES conversation_messages(id) ON DELETE CASCADE,
 question TEXT NOT NULL,
 answer TEXT NOT NULL,
 evidence JSONB NOT NULL DEFAULT '[]',
 tool_exchange JSONB NOT NULL DEFAULT '[]',
 metadata JSONB NOT NULL DEFAULT '{}',
 created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX arcadia_traces_owner_idx ON arcadia_traces(kb_id, user_id, created_at DESC);
CREATE INDEX arcadia_traces_evidence_idx ON arcadia_traces USING gin(evidence);

CREATE TABLE arcadia_changes (
 id UUID PRIMARY KEY,
 kb_id UUID NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
 document_id UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
 -- Actor UUIDs retain attribution independently of account deletion, like audit_events.
 proposed_by UUID NOT NULL,
 title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 160),
 reason TEXT NOT NULL,
 content TEXT NOT NULL CHECK (length(content) BETWEEN 1 AND 500000),
 base_content TEXT NOT NULL,
 base_sha TEXT NOT NULL,
 base_updated_at TIMESTAMPTZ NOT NULL,
 status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','approved','rejected')),
 decided_by UUID,
 decision_note TEXT,
 created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
 decided_at TIMESTAMPTZ
);
CREATE INDEX arcadia_changes_kb_idx ON arcadia_changes(kb_id, created_at DESC);
CREATE TABLE arcadia_replays (
 id UUID PRIMARY KEY,
 trace_id UUID NOT NULL REFERENCES arcadia_traces(id) ON DELETE CASCADE,
 answer TEXT NOT NULL,
 evidence JSONB NOT NULL,
 metadata JSONB NOT NULL,
 as_of TIMESTAMPTZ,
 created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX arcadia_replays_trace_idx ON arcadia_replays(trace_id, created_at DESC);

-- A physical purge must not leave copied evidence recoverable through trace APIs.
CREATE FUNCTION arcadia_redact_purged_document() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.purged_at IS NOT NULL AND OLD.purged_at IS NULL THEN
  UPDATE arcadia_traces SET evidence = '[]', answer = '[redacted after source purge]',
   tool_exchange = '[]', question = '[redacted after source purge]',
   metadata = (metadata - 'question') || '{"redacted":true}'::jsonb
   WHERE kb_id = NEW.kb_id AND evidence @> jsonb_build_array(jsonb_build_object('document_id',NEW.id));
  DELETE FROM arcadia_replays WHERE evidence @> jsonb_build_array(jsonb_build_object('document_id',NEW.id)) OR trace_id IN
   (SELECT id FROM arcadia_traces WHERE kb_id = NEW.kb_id AND metadata->>'redacted' = 'true');
  UPDATE arcadia_changes SET content = '[redacted]', base_content = '[redacted]', decision_note = '[redacted]', reason = '[redacted]', title = '[redacted]'
   WHERE document_id = NEW.id;
  UPDATE conversation_messages SET content='[redacted after source purge]', sources='[]',
   steps='[]', resolved='[]', tool_exchange='[]' WHERE id IN
   (SELECT message_id FROM arcadia_traces WHERE kb_id=NEW.kb_id AND metadata->>'redacted'='true');
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER arcadia_purge AFTER UPDATE OF purged_at ON documents
 FOR EACH ROW EXECUTE FUNCTION arcadia_redact_purged_document();
