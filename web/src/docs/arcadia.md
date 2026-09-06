# Arcadia: evidence and change

Arcadia builds on Utopia's temporal knowledge graph and retrieval assistant. The overview, change desk, answer ledger and historical evidence workspace connect the existing tools into a reviewable workflow.

## Propose and review

In **Changes**, an editor chooses a ready document and submits a title, reason and full replacement text. The original text is retained with the proposal. Proposing does not update the live document.

The impact panel lists facts supported by the document, counts dependent derived conclusions and links your own saved answers that cited it. These are potential dependencies, not proof that each answer will change. Private answers belonging to other people are never included.

A knowledge-base administrator can approve or reject. Approval checks that the underlying document is still ready and unchanged. A stale proposal must be recreated. Successful approval adds one document version, enqueues processing and writes an audit event atomically. Processing is asynchronous: watch its status in the library.

## Inspect answers

New assistant messages save a private trace alongside the message. The answer ledger shows the exact answer, captured document text, tool exchange and available model metadata. Export downloads JSON for the selected trace and its recent replays.

This records evidence and execution artifacts, not hidden model reasoning. It does not recreate a model's internal state or guarantee deterministic output. Deleting the conversation also deletes its trace records. A physical source purge redacts copied evidence and affected replays.

## Compare a new answer

Choose a saved answer, then run a document-only comparison. Leave both controls empty for current evidence, choose an **as known at** UTC time for retained historical evidence, or select a current pending proposal for a preview.

A proposal preview substitutes its full text for that document. It does not change live knowledge. A rerun uses the currently configured chat model and document retrieval; it does not execute old graph tools, external SQL queries or prior conversation history. Citation checks verify numbering only. They do not verify factual accuracy.

## Browse history

Historical evidence search selects chunks and documents that were live at the requested record time before ranking them. It retrieves retained text from Postgres. Current search retains Tantivy's multilingual tokenizer; historical keyword tokenization can therefore produce different matches.

The graph's **as known at** selector is also record time. It is separate from world time, which describes when a fact was true. The graph and opened entity details use the selected record time.

## Organization sign-in

An operator configures the OIDC issuer, client ID, optional client secret and callback URL. An organization administrator links existing accounts to exact provider subject identifiers under **Account → Organization SSO**. Matching email addresses are not sufficient to link an identity. Password sign-in remains available.
