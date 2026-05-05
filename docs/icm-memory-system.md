# ICM (Infinite Context Memory) — Functional Overview and Capability Impact

**Audience:** Wolfie, as a quick but comprehensive briefing on the ICM system wired into this project, the skills that surface it (`/recall`, `/remember`, the `mcp__icm__*` tools, the `using-superpowers` skill family), and the scientific framing that justifies them.

**Scope:** what ICM is, what it stores, how it is queried, how it is hooked into Claude Code, and — most importantly — how it changes the assistant's behaviour and effective capabilities across sessions.

**Sources of fact for this document:**
- `icm` CLI v0.10.43 on this machine (`/usr/local/bin/icm`).
- The `mcp__icm__*` tool schemas surfaced by ToolSearch in this session.
- The upstream repo at `github.com/rtk-ai/icm` (Apache-2.0).
- This project's `CLAUDE.md` (mandatory-triggers section) and the user's auto-memory at `~/.claude/projects/-Users-wgiersche-workspace-Project-Ellie-egras/memory/`.
- Peer-reviewed and arXiv literature on agent memory (cited inline; full list at the end).

---

## 1. The problem ICM is built to solve

LLM agents are stateless across sessions and bounded by a context window inside a session. Three failure modes follow:

1. **Re-discovery cost.** Every new session re-reads the same files, re-derives the same architecture, re-asks the same clarifying questions.
2. **Context dilution.** Long sessions push load-bearing facts past the attention horizon; recent tokens crowd out older but still-relevant ones (lost-in-the-middle effects, Liu et al. 2024).
3. **Cross-session drift.** Decisions, preferences, and corrections made on Monday are not honoured on Tuesday unless re-stated.

The standard mitigations — bigger context windows, RAG over project files, custom system prompts — each address part of this but not the whole. They do not give the agent a *writable* memory it actually controls.

ICM frames the problem as one of giving an agent **persistent, structured, multi-layer memory** that survives session boundaries and is shared across tools (Claude Code, Cursor, Gemini CLI, Copilot, Aider, etc. — 17 tools per the upstream README). It is built as a **single Rust binary** backed by **SQLite (FTS5 + sqlite-vec)**, exposed via **MCP**, **CLI**, and **hook handlers**.

This matches the framing of recent surveys and position papers, which argue that durable, structured memory — particularly **episodic** memory — is the missing piece for long-horizon agents:
- Sumers et al., *Cognitive Architectures for Language Agents (CoALA)*, TMLR 2024 — proposes a cognitive blueprint with working, episodic, semantic, and procedural stores around a central LLM executive.
- Pink et al., *Position: Episodic Memory is the Missing Piece for Long-Term LLM Agents*, ICML 2025.
- Zhang et al., *A Survey on the Memory Mechanism of Large Language Model-based Agents*, ACM TOIS 2025.
- Packer et al., *MemGPT: Towards LLMs as Operating Systems*, COLM 2024 — virtual-memory paging of context from a tiered store.

ICM is, in cognitive-architecture terms, a concrete CoALA-shaped memory back-end with a MemGPT-style retrieval layer.

---

## 2. Architecture at a glance

```
┌──────────────────────────────────────────────────────────────────┐
│                       Claude Code session                        │
│                                                                  │
│   SessionStart hook ──► icm hook start  ──► wake-up pack         │
│   UserPromptSubmit hook ──► icm hook prompt ──► auto-recall      │
│   PostToolUse hook ──► icm hook post ──► fact extraction         │
│   PreCompact hook ──► icm hook compact ──► distill before drop   │
│                                                                  │
│   Slash commands: /recall, /remember                             │
│   MCP tools:      mcp__icm__icm_memory_*                         │
│                   mcp__icm__icm_memoir_*                         │
│                   mcp__icm__icm_transcript_*                     │
│                   mcp__icm__icm_feedback_*                       │
│                   mcp__icm__icm_wake_up                          │
└──────────────────────────────────────────────────────────────────┘
                                │
                                ▼
              ┌─────────────────────────────────┐
              │  icm  (single Rust binary)      │
              │  ─ MCP server (stdio/JSON-RPC)  │
              │  ─ CLI                           │
              │  ─ Hook handlers                 │
              │  ─ TUI dashboard                 │
              └─────────────────────────────────┘
                                │
                                ▼
              ┌─────────────────────────────────┐
              │  SQLite database                 │
              │  ─ FTS5 (BM25 keyword)           │
              │  ─ sqlite-vec (cosine, 768-d)    │
              │  ─ Embeddings: multilingual-e5   │
              │  ─ Hybrid scoring: 30% FTS + 70% │
              │      cosine                      │
              └─────────────────────────────────┘
```

Storage is local, single-file, no external service. Embeddings are computed locally with `intfloat/multilingual-e5-base` (768-dim, multilingual). Search is **hybrid**: BM25 over FTS5 plus cosine over vector embeddings, weighted 30/70 by default — a configuration that aligns with the empirical sweet spot reported in hybrid-retrieval studies (e.g., Bruch et al., *An Analysis of Fusion Functions for Hybrid Retrieval*, ACM TOIS 2024).

---

## 3. The five memory layers

ICM is not a single store. It exposes **five distinct layers**, each mapped to a different cognitive role.

### 3.1 Episodic memories — `icm_memory_*`

Time-stamped, topic-scoped facts with **importance-driven decay**:

| Importance | Decay     | Auto-prune | Typical use                              |
|------------|-----------|------------|------------------------------------------|
| critical   | none      | never      | Identity, hard architectural constraints |
| high       | slow      | never      | Decisions, resolved errors, preferences  |
| medium     | normal    | yes        | Routine context                          |
| low        | fast      | yes        | Ephemeral notes                          |

Decay is **access-aware**: `effective_weight = base_weight × decay / (1 + access_count × 0.1)`. Frequently recalled memories age more slowly. This implements a power-law forgetting curve closer to human episodic decay (Ebbinghaus 1885; Anderson & Schooler 1991, *Reflections of the environment in memory*) than a flat TTL.

Auto-deduplication: storing a memory >85% similar to an existing one in the same topic **updates** instead of duplicating.

**MCP surface:** `icm_memory_store`, `icm_memory_recall`, `icm_memory_update`, `icm_memory_forget`, `icm_memory_consolidate`, `icm_memory_health`, `icm_memory_list_topics`, `icm_memory_extract_patterns`, `icm_memory_embed_all`, `icm_memory_forget_topic`, `icm_memory_stats`.

This is the layer the CLAUDE.md in this repo refers to with its **mandatory store triggers** (errors-resolved, decisions-egras, preferences, context-egras).

### 3.2 Memoirs — `icm_memoir_*` (semantic / permanent)

A memoir is a **named knowledge graph** of permanent concepts, not subject to decay. Concepts have:
- a unique name within the memoir,
- a dense definition,
- labels (`domain:auth,type:decision`),
- typed directed edges to other concepts using a fixed relation vocabulary: `part_of`, `depends_on`, `related_to`, `contradicts`, `refines`, `alternative_to`, `caused_by`, `instance_of`, `superseded_by`.

Crucially, concepts are never deleted; outdated ones are linked `superseded_by` so the lineage is preserved. This is closer to the **semantic-memory** layer in CoALA, and to the structured-knowledge view advocated by MIRIX (Wang et al. 2025) and MemMachine (2026).

Export formats include `dot` (Graphviz) and `ai` (compact markdown for LLM injection). The latter is the channel by which a memoir can be re-loaded into a future agent's prompt.

**MCP surface:** `icm_memoir_create`, `icm_memoir_add_concept`, `icm_memoir_link`, `icm_memoir_refine`, `icm_memoir_inspect`, `icm_memoir_search`, `icm_memoir_search_all`, `icm_memoir_show`, `icm_memoir_list`, `icm_memoir_export`, plus `icm_learn` (scan a project directory and bootstrap a memoir from its structure).

### 3.3 Transcripts — `icm_transcript_*` (verbatim episodic)

Raw, unsummarized message logs — role-tagged (`user`, `assistant`, `system`, `tool`) with optional tool name and token count. Searchable via FTS5 BM25 with boolean operators, phrase match, prefix match. Cascade delete on session.

This is the "videotape" layer: it does *not* try to be smart, it preserves ground truth. It maps to MemMachine's "ground-truth-preserving" design and is the layer one would query for compliance, debugging, or to settle "but you said X" disputes.

### 3.4 Feedback — `icm_feedback_*` (correction loop)

Distinct from generic memory. Records `(context, predicted, corrected, reason)` whenever the agent was wrong, then exposes `icm_feedback_search` to consult past mistakes **before** making a similar prediction. This is a closed-loop learning channel — analogous to a mini reinforcement-from-corrections store, sitting in the spirit of test-time learning probed by MemoryAgentBench (Tan et al. 2025).

### 3.5 Wake-up pack — `icm_wake_up`

A session-startup primer. Selects critical/high memories (and global preferences), ranks by `importance × recency × weight`, packs into a token budget (default 200, max 4000), formats as markdown or plain text. Runs as the SessionStart hook in Claude Code; the `# ICM Wake-up` block at the top of this very session was produced by it.

This is the operationalization of MemGPT's "core memory" idea — a small, always-resident block of the most load-bearing facts.

---

## 4. How it is wired into Claude Code

There are four orthogonal access channels:

1. **MCP tools** (~31 in this server). Explicit calls by the agent. Token-cheap (~20–50 tokens/call).
2. **Hooks**, configured by `icm init`. Five hook points are supported in Claude Code:
   - `SessionStart` → wake-up pack injection.
   - `UserPromptSubmit` → auto-recall (the `Here is context from previous analysis…` block at the top of this session is one such injection).
   - `PostToolUse` → fact extraction from tool output.
   - `PreCompact` → distill before context compression.
   - `PreToolUse` → policy / auto-allow.
3. **Slash commands** — `/recall <query>`, `/remember <text>` — surfaced as user-invokable skills.
4. **Auto-injected instructions** in `CLAUDE.md` (the "Persistent memory (ICM) — MANDATORY" block in this project, and the MCP-server instructions block at session start).

The `using-superpowers` skill enforces a **hard rule**: invoke applicable skills *before* responding. ICM-related skills fire under that rule whenever the user mentions memory, recall, prior decisions, or starts a non-trivial task.

---

## 5. How this changes my (the assistant's) capabilities

I'll be specific about what shifts, because the value proposition is otherwise hand-wavy.

### 5.1 Persistence across sessions
Without ICM I treat every session as cold-start; I re-derive your preferences (e.g., "Wolfie, not Wolfgang"; "postgres only via docker"; "fmt + clippy + nextest before push") from scratch or from `CLAUDE.md`. With ICM these are stored as `critical`/`high` memories and auto-injected via the wake-up hook, so I behave consistently from message 1.

### 5.2 Larger effective context for the same window
Rather than re-reading 30 files at the start of a session to re-build a mental model, I receive a distilled wake-up pack (~200 tokens) that captures the load-bearing decisions. The published agent-efficiency benchmark on the upstream repo reports −29% to −44% input tokens and −17% to −22% cost across sessions 2–3 of a workflow. Even discounting those numbers, the directional effect is clear: **less re-reading, more acting**.

### 5.3 Higher recall accuracy on long-horizon tasks
The upstream LongMemEval (ICLR 2025, 500-question suite) figure is 100% retrieval precision (ICM-side) with 82% end-to-end answer accuracy when paired with Claude Sonnet. This is in line with — and at the high end of — the band reported across recent agent-memory systems.

### 5.4 Correction durability
The `feedback` layer means a correction you make once is consultable forever. This is the channel by which "stop summarizing at the end of every response" or "never rebase main" becomes durable behaviour rather than a per-session tax.

### 5.5 Structured project knowledge, not just facts
Memoirs let me carry a **graph** of project concepts (services, invariants, decisions, contradictions) rather than a flat bag of memories. This is what enables answers like "X depends on Y, but Y was superseded by Z in decision D" — the kind of reasoning that flat RAG does poorly.

### 5.6 What it does *not* do
- It is **not** a substitute for reading current code. Memory can be stale; the file system is authoritative. Per `CLAUDE.md`'s memory protocol, before acting on a recalled fact I must verify it against the current code.
- It does **not** improve in-session context handling — it improves the *boundary* between sessions.
- It does **not** train the model. There is no weight update; it is purely retrieval-augmented prompting.
- Embedding quality is bounded by `multilingual-e5-base`. Highly domain-specific queries may need tuned retrieval.

---

## 6. Operational protocol for this project

Pulled directly from `CLAUDE.md` and the project memory index. Mandatory store triggers:

| Trigger                         | Topic                  | Importance |
|---------------------------------|------------------------|------------|
| Error resolved                  | `errors-resolved`      | high       |
| Architecture / design decision  | `decisions-egras`      | high       |
| User preference / correction    | `preferences`          | critical   |
| Significant task completed      | `context-egras`        | high       |
| ~20 tool calls without a store  | (progress summary)     | high       |

Recall must happen at the **start** of any non-trivial task, scoped to the relevant topic — not as a context dump.

Hygiene commands worth knowing:

```bash
icm health                 # staleness + consolidation audit
icm topics                 # list topics with counts
icm consolidate <topic>    # collapse a noisy topic into one summary
icm extract-patterns <t>   # find recurring patterns, optionally promote to memoir concepts
icm decay                  # apply temporal decay
icm prune                  # drop low-weight memories
```

---

## 7. What this looks like inside a conversation with me

Abstract architecture is uninteresting if it doesn't change turn-by-turn behaviour. Here is the concrete picture, end-to-end, of ICM during a session with you.

### 7.1 Before your first message even arrives

Three things happen automatically, **before** I see your prompt:

1. The `SessionStart` hook runs `icm wake-up` and prepends a compact pack of `critical`/`high` memories to my context. In this very session it produced the block headed *"# ICM Wake-up (project: egras)"* — your name preference and the Ignatius backstory landed in my prompt that way.
2. The MCP server's instructions block is injected, restating the **mandatory store triggers** so I can't quietly ignore them.
3. The repo's `CLAUDE.md` is loaded, including the *Persistent memory (ICM) — MANDATORY* section.

Net effect: I start the session with your identity, project conventions, and recent decisions already in working memory — not as code I have to grep for.

### 7.2 When you send a turn

The `UserPromptSubmit` hook runs **before** I respond. It can inject relevant memories scoped to your prompt's keywords. In this session it produced the *"Here is context from previous analysis…"* block before my first reply. That block is generated, not curated by me.

This means: when you say *"continue the auth refactor"*, I see the prior decisions on auth attached to your message — without you re-pasting them, and without me running a tool call to fetch them.

### 7.3 During my turn

Two patterns dominate:

- **I call `mcp__icm__icm_memory_recall` (or the `/recall` skill) early** when your task plausibly has prior context — past errors on the same module, prior architectural decisions, previous corrections from you. Cost: 20–50 tokens per call. Benefit: I avoid re-deriving things you already settled.
- **I call `mcp__icm__icm_memory_store` (or the `/remember` skill) immediately** when one of the five mandatory triggers fires:
  1. you correct me → `topic=preferences`, `importance=critical`
  2. we make a design decision → `topic=decisions-egras`, `importance=high`
  3. I resolve a non-trivial error → `topic=errors-resolved`, `importance=high`
  4. I finish a significant task → `topic=context-egras`, `importance=high`
  5. ~20 tool calls without a store → progress summary

The protocol from `CLAUDE.md` is explicit: **store before responding**, not after. This is to avoid the failure mode where I summarise the work, you say "thanks", the session ends, and the lesson is lost.

### 7.4 During tool use and compaction

- `PostToolUse` hook can extract facts from tool output (e.g., a long `cargo test` failure summary) and store them, so the next time I hit the same error I have the resolution path.
- `PreCompact` hook lets ICM distill the about-to-be-dropped tail of the context into durable memory, instead of losing it when the session is auto-compressed.

This is the channel that protects against silent context loss when our conversation crosses the auto-compaction threshold.

### 7.5 At the end of a turn or session

Nothing dramatic — but if you say *"forget that, we changed our mind"*, I should call `icm_memory_forget` or `icm_memory_update` rather than letting the stale fact age out. And if a topic has accumulated more than a handful of overlapping entries, `icm_memory_consolidate` collapses them into a single clean summary.

### 7.6 What you can do to steer it

Three useful levers from your side:

- **`/remember <text>`** — force a store right now, no ambiguity.
- **`/recall <query>`** — make me show you what I have on a topic, useful for auditing what I "know" about you or the project.
- **Tell me the topic** — saying *"this is a preference"* or *"file this under decisions-egras"* reliably routes the store to the right bucket and importance.

Conversely, if you say *"don't use memory for this"*, I disable the recall side: I won't cite or compare against memory content for that turn (per the `using-superpowers` rule on user instructions overriding skills).

### 7.7 A worked example from this very session

When you opened with *"can you please do me a favour…"*, the chain was:

1. `SessionStart` injected the wake-up pack (your name, the Ignatius origin, project pointers).
2. `UserPromptSubmit` injected previous-analysis context (the auto-memory index).
3. I invoked `Skill(recall)` and `Skill(remember)` per the `using-superpowers` rule (they were applicable: the user is asking *about* memory).
4. I used `ToolSearch` to load the full `mcp__icm__*` schema — those tools are deferred and need to be hydrated before use.
5. I queried the upstream repo via `WebFetch` and the literature via `WebSearch` for the scientific framing you asked for.
6. I wrote the document. Once we settle on the final form, the right move is a `mcp__icm__icm_memory_store` under `context-egras`, `importance=high`, with content roughly *"Generated docs/icm-memory-system.md as the canonical ICM briefing for this project; update if icm tool surface changes."*

That last step is the one that matters for *next* time: it means the next session knows this document exists, so I don't generate a parallel one a week from now.

---

## 8. Mapping to the cognitive-science literature

ICM is best understood as one concrete realisation of the multi-store memory model that current agent research is converging on. A short crosswalk:

| Cognitive role                  | Classical reference                                  | ICM layer                  |
|---------------------------------|------------------------------------------------------|----------------------------|
| Working memory                  | Baddeley & Hitch 1974                                | (the LLM context itself)   |
| Episodic memory                 | Tulving 1972, *Episodic and semantic memory*         | `memory_*` (decaying)      |
| Semantic memory                 | Tulving 1972; Collins & Quillian 1969                | `memoir_*` (graph)         |
| Procedural memory               | Anderson 1982, *Acquisition of cognitive skill*      | (skills + feedback corrections) |
| Verbatim trace / videotape      | —                                                    | `transcript_*`             |
| Forgetting curve                | Ebbinghaus 1885                                      | importance × access decay  |
| Power-law of practice           | Anderson & Schooler 1991                             | access-aware decay formula |
| Core / always-resident memory   | Packer et al., MemGPT 2024                           | `wake_up`                  |
| Cognitive architecture skeleton | Sumers et al., CoALA, TMLR 2024                      | (whole system)             |

No single citation justifies the system as a whole; the design is a synthesis. But each component has a clear precedent.

---

## 9. References

Peer-reviewed / arXiv:

- Sumers, Yao, Narasimhan, Griffiths. *Cognitive Architectures for Language Agents.* TMLR 2024. arXiv:2309.02427.
- Packer, Wooders, Lin, Fang, Patil, Stoica, Gonzalez. *MemGPT: Towards LLMs as Operating Systems.* COLM 2024. arXiv:2310.08560.
- Zhang, Chen, Bao, et al. *A Survey on the Memory Mechanism of Large Language Model-based Agents.* ACM Transactions on Information Systems, 2025. doi:10.1145/3748302.
- Pink, Vo, et al. *Position: Episodic Memory is the Missing Piece for Long-Term LLM Agents.* ICML 2025. arXiv:2502.06975.
- Wu, Zhu, et al. *LongMemEval: Benchmarking Chat Assistants on Long-Term Interactive Memory.* ICLR 2025. arXiv:2410.10813.
- Liu, Lin, Hewitt, et al. *Lost in the Middle: How Language Models Use Long Contexts.* TACL 2024. arXiv:2307.03172.
- Bruch, Gai, Ingber. *An Analysis of Fusion Functions for Hybrid Retrieval.* ACM TOIS 2024.
- Tulving. *Episodic and Semantic Memory.* In *Organization of Memory*, 1972.
- Baddeley, Hitch. *Working Memory.* In *Psychology of Learning and Motivation*, vol. 8, 1974.
- Anderson, Schooler. *Reflections of the Environment in Memory.* Psychological Science, 1991.
- Ebbinghaus. *Über das Gedächtnis.* 1885.

System / product:

- ICM repository and README, github.com/rtk-ai/icm (Apache-2.0, v0.10.43, 2026-05-02).
- Project-local `CLAUDE.md`, *Persistent memory (ICM) — MANDATORY* section.
- Anthropic, *Model Context Protocol* specification, modelcontextprotocol.io.

Related systems referenced in the upstream comparison and the literature:

- Mem0 (Singh et al., arXiv:2504.19413, 2025).
- MIRIX (Wang et al., 2025) — multi-agent six-store memory architecture.
- MemMachine (2026) — ground-truth-preserving episodic memory.
- MemoryAgentBench (Tan et al., 2025); MemBench; LongMemEval.

---

*Document generated 2026-05-05 in this project's repository. Update in place if ICM's tool surface changes (current pinned version: icm 0.10.43).*
