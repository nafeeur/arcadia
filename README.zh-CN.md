<div align="center">

<img src="assets/banner.webp" alt="Utopia" width="820">

</div>

# Utopia

<div align="center">

[Worldview](#the-projects-worldview) · [Quick start](#quick-start) · [Features](#features) · [Roadmap](#roadmap)

[![Stars](https://img.shields.io/github/stars/deeplethe/utopia?style=flat-square&label=STARS&labelColor=161B22&color=FFC220&logo=github&logoColor=FFFFFF)](https://github.com/deeplethe/utopia/stargazers)
[![License](https://img.shields.io/badge/LICENSE-APACHE%202.0-3FB950?style=flat-square&labelColor=161B22)](LICENSE)
[![Rust](https://img.shields.io/badge/BUILT%20WITH-RUST-F74C00?style=flat-square&labelColor=161B22&logo=rust&logoColor=FFFFFF)](https://www.rust-lang.org)

[![Official site](https://img.shields.io/badge/OFFICIAL-UTOPIA.BI-FFFFFF?style=flat-square&labelColor=161B22&logo=safari&logoColor=FFFFFF)](https://utopia.bi)
[![Container](https://img.shields.io/badge/GHCR-DEEPLETHE%2FUTOPIA-2496ED?style=flat-square&labelColor=161B22&logo=docker&logoColor=FFFFFF)](https://github.com/deeplethe/utopia/pkgs/container/utopia)
[![Discussions](https://img.shields.io/badge/DISCUSSIONS-8957E5?style=flat-square&labelColor=161B22&logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIxNiIgaGVpZ2h0PSIxNiIgZmlsbD0iI0ZGRkZGRiIgY2xhc3M9ImJpIGJpLWNoYXQtZG90cy1maWxsIiB2aWV3Qm94PSIwIDAgMTYgMTYiPgogIDxwYXRoIGQ9Ik0xNiA4YzAgMy44NjYtMy41ODIgNy04IDdhOSA5IDAgMCAxLTIuMzQ3LS4zMDZjLS41ODQuMjk2LTEuOTI1Ljg2NC00LjE4MSAxLjIzNC0uMi4wMzItLjM1Mi0uMTc2LS4yNzMtLjM2Mi4zNTQtLjgzNi42NzQtMS45NS43Ny0yLjk2NkMuNzQ0IDExLjM3IDAgOS43NiAwIDhjMC0zLjg2NiAzLjU4Mi03IDgtN3M4IDMuMTM0IDggN001IDhhMSAxIDAgMSAwLTIgMCAxIDEgMCAwIDAgMiAwbTQgMGExIDEgMCAxIDAtMiAwIDEgMSAwIDAgMCAyIDBtMyAxYTEgMSAwIDEgMCAwLTIgMSAxIDAgMCAwIDAgMiIvPgo8L3N2Zz4%3D)](https://github.com/deeplethe/utopia/discussions)
[![Built by DeepLethe](https://img.shields.io/badge/BUILT%20BY-DEEPLETHE-2D333B?style=flat-square&labelColor=161B22)](https://github.com/deeplethe)
[![English](https://img.shields.io/badge/LANG-ENGLISH-DA3633?style=flat-square&labelColor=161B22)](README.md)

</div>

**An enterprise knowledge world model built by [DeepLethe](https://deeplethe.com).** It is the first open-source knowledge engineering foundation built on ontology-based passive learning and self-governance — unlike a knowledge graph or a vector knowledge base, this project builds time-awareness and ontology into the base of the system, evolves its knowledge from incoming corpora, and performs conflict detection, knowledge inference, and intelligent decision-making on top of an ontology. It supports offline deployment, letting an enterprise quickly stand up a knowledge foundation, a trustworthy decision hub, and a compliance audit hub, advancing the enterprise's push toward intelligent operation.

> Note that we don't think of this project as an open-source attempt at Palantir, but rather as a new approach to enterprise intelligence that works bottom-up — from knowledge governance up to trustworthy intelligent decision-making and inference.

---

<!-- Video: drag an mp4 into any issue/PR comment box, GitHub returns a
     https://github.com/user-attachments/assets/xxx link;
     paste that link on its own line here and it renders as a player. -->

<div align="center">

https://github.com/user-attachments/assets/aa226443-75de-437e-bd80-88e592ed8457

</div>

---

## The project's worldview

We gave it a somewhat romantic name — **Utopia**. Ptolemy's geocentric model was for a long time held to be true, and was disproved step by step by Copernicus, Kepler, Galileo, and Newton. Looking back now, what we remember is not just "the heliocentric model was right," but how that history unfolded.

Unlike existing vector knowledge bases and knowledge graph efforts, which chase the correctness of present-day knowledge, one of Utopia's founding design goals is to record the complete history of how understanding changed. Engineering-wise, that is implemented as a **bitemporal knowledge graph**. When reviewing a decision, the system can retrieve the full decision process and its basis. To improve usability, we iterated extensively using public corpora from enterprise information, education, finance, law, scientific research, and other domains. Temporal capability is only one facet; for how the system takes in knowledge, how it projects the future, and how logic constrains action, see [utopia.bi/philosophy](https://utopia.bi/philosophy).

## Features

The system is made up of a Rust binary and a Postgres service. Using pgvector and a queue-table design, we kept the stack and its service dependencies light.

| Capability | Highlights |
| --- | --- |
| **Complete application** | System console · graph browser · online ontology workbench · works out of the box |
| **Document ingestion** | Supports many document types (pdf, md, html, ppt, word, excel) · scheduled sync from web pages, RSS, GitHub, Jira, Notion, WebDAV, and S3-compatible storage |
| **Hybrid retrieval** | Tantivy · pgvector vectors · RRF fusion · chunk provenance |
| **Bitemporal graph** | Knowledge time + provenance time · a graph at any point in time · a chain of knowledge changes |
| **AgentHarness · Agentic RAG** | The application itself carries harness capability, driving its full feature set through conversation · a built-in agent with a range of tools supports multi-turn tool calls and dialogue |
| **Built-in ontology packs** | Ships schema.org · W3C Org · PROV-O · FOAF · IOF Core · continually expanding · [request extra support for your own industry](https://github.com/deeplethe/utopia/issues/new?labels=enhancement&title=Ontology%20pack%20request) |
| **Semantic extraction** | Entity, relation, and time normalization · every fact carries a mandatory evidence quote · vector-and-ontology recall plus LLM adjudication auto-disambiguates, traceably and reversibly · ontology revision proposals are raised automatically from the text |
| **Knowledge derivation and reasoning** | Temporal Datalog · forward chaining · compiled ontology axioms · traceable derivation paths · a lightweight reasoning engine built in-house in Rust |
| **Conflict detection** | Temporal conflicts · reflexivity, antisymmetry, transitive cycles, cardinality violations · defects in the ontology itself · facts can be retracted, axioms can be changed, coexistence can be acknowledged |
| **Human review and an audit ledger** | Low-confidence extractions and merge candidates enter a review queue automatically · every operation's user, time, and change snapshot is recorded for compliance audit |
| **Intelligent mapping and question-answering over data** | Given a database and a knowledge base, an agent automatically explores and builds the mapping between them · question-answering over data built on Ontology2SQL · [achieved SOTA (best result) on BIRD Mini-Dev](https://github.com/bird-bench/bird-bench.github.io/pull/218) |
| **Model access** | Any OpenAI-compatible endpoint · locally deployed models supported |
| **Multi-user, multi-knowledge-base** | Roles and permissions scoped per knowledge base, with system administrators, users, and tiered knowledge-base admin/edit/view access |
| **[Decision intelligence (in development)](#roadmap)** | Decision records · replay of the reasoning and decision process · scenario-overlay inference |

## Quick start

Dependencies: Docker (local development additionally needs Rust 1.85+, Node 20+, pnpm).

Quick start from a prebuilt image:

```bash
git clone https://github.com/deeplethe/utopia.git
cd utopia
docker compose --profile app up -d
```

Open http://localhost:1516 to sign up — the first account automatically becomes the administrator, and the system also creates a public knowledge base readable by everyone. Before extracting from business documents, configure the model endpoints (chat and embedding) under "Admin → Models".

Or build from source:

```bash
docker compose -f docker-compose.yml -f docker-compose.build.yml --profile app up -d --build
```

### Local development

```bash
# 1. Start the database (Postgres with pgvector)
docker compose up -d db

# 2. Start the backend (runs migrations automatically, defaults to :1516)
cargo run -p utopia-server

# 3. Start the frontend (:5173, proxies /api to the backend)
cd web && pnpm install && pnpm dev
```

## Roadmap

- [ ] **Decision reasoning**: compute constraints, review decisions after the fact
- [ ] **Business rules**: rules written by a person over an entity's attribute facts (thresholds, category sets) that classify an entity into a derived fact carrying a premise, with the rule and its premise serving as its explanation ([#277](https://github.com/deeplethe/utopia/issues/277))
- [ ] **An execution validation layer**: validate an agent's calls against ontology rules and symbolic logic
- [ ] **Data lakehouse support for question-answering and mapping**: Iceberg / Delta Lake, plus mapping discovery and Ontology2SQL support for Databricks, Snowflake, and MaxCompute
- [ ] **More data sources**: a ClickHouse driver, a Feishu connector
- [ ] **Instant-level precision**: add an `instant` precision tier beyond year / month / day, for sources that already carry a timestamp — connectors currently truncate to UTC days, so events that cross midnight can be off by a day
- [ ] **Agent memory over MCP**: fill in episode writes, a retrieve endpoint, and an MCP server
- [ ] **Enterprise readiness**: OIDC SSO, backup/restore commands, a performance benchmark at the 100k-document scale

## Current status

Utopia is still **v0.1**. The database schema will evolve across versions, and migrations only roll forward, never back — in production, pin a specific version with `UTOPIA_IMAGE` and back up the database and the `data` directory before upgrading.

Read [SECURITY.md](SECURITY.md) before deploying to the public internet.

## Community

- 💬 [Discussions](https://github.com/deeplethe/utopia/discussions): come discuss the project, share your experience, leave feedback
- 🐛 [Issues](https://github.com/deeplethe/utopia/issues): any bug, design question, or feature request
- 🤝 [Contributing](CONTRIBUTING.zh-CN.md): dev environment, pre-submission checks, DCO sign-off
- 🔌 [Ontology2SQL](https://github.com/deeplethe/ontology2sql): the ontology-driven text-to-SQL method mentioned above

## License

[Apache-2.0](LICENSE)
