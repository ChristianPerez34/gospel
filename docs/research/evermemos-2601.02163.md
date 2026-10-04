# EverMemOS (arXiv:2601.02163) — findings for Gospel

Primary sources:

- Paper HTML: <https://arxiv.org/html/2601.02163> (v2, 9 Jan 2026)
- Hugging Face: <https://huggingface.co/papers/2601.02163>
- Code claim in the paper: <https://github.com/EverMind-AI/EverMemOS> (the public repo currently presents as EverOS)

## What the paper actually claims

EverMemOS is a **three-phase memory lifecycle**, not a bigger context window and not a flat RAG store. Failures in long-horizon agents often come from **poor integration** of retrieved fragments (missed conflicts, unstable user models), not from missing records. ([paper §1](https://arxiv.org/html/2601.02163#S1))

### Memory primitive — MemCell

A MemCell is `c = (E, F, P, M)` ([paper §3.2](https://arxiv.org/html/2601.02163#S3.SS2)):

| Field | Meaning |
| --- | --- |
| **Episode `E`** | Concise third-person narrative; semantic anchor |
| **Atomic facts `F`** | Discrete verifiable statements, used for retrieval matching |
| **Foresight `P`** | Forward-looking inferences (plans, temporary states) with validity interval `[t_start, t_end]` |
| **Metadata `M`** | Timestamps and source pointers |

### Phase I — Episodic Trace Formation

Dialogue stream → MemCells via ([paper §3.3](https://arxiv.org/html/2601.02163#S3.SS3)):

1. **Contextual segmentation** — LLM sliding-window topic-boundary detector
2. **Narrative synthesis** — rewrite the window into a coreference-resolved third-person Episode
3. **Structural derivation** — constrained-schema extraction of Atomic Facts + Foresight intervals from the Episode

Ablation: semantic segmentation beats fixed message/token chunking and even LoCoMo’s ground-truth session boundaries (89.16 vs 87.66 on GPT-4.1-mini). The gap vs session boundaries is small (≤0.7 across backbones). ([paper Table 3](https://arxiv.org/html/2601.02163#S4.T3))

### Phase II — Semantic Consolidation

Online, not batch ([paper §3.4](https://arxiv.org/html/2601.02163#S3.SS4)):

- Embed the new MemCell; assign to nearest **MemScene** centroid if similarity ≥ τ, else open a new scene
- Clustering also uses a **max time gap** (7 days on LoCoMo, 30 on LongMemEval) so temporally distant cells do not merge ([paper Table 6](https://arxiv.org/html/2601.02163#A1.T6))
- Scene summaries drive a compact **User Profile**: explicit facts (including time-varying measurements) + implicit traits, with recency-aware updates and conflict tracking ([paper Appendix B.3](https://arxiv.org/html/2601.02163#A2.SS3))

Default τ: 0.70 (LoCoMo), 0.50 (LongMemEval).

### Phase III — Reconstructive Recollection

Not “top-k chunks”. Principle: **necessity and sufficiency** ([paper §3.5](https://arxiv.org/html/2601.02163#S3.SS5)):

1. Hybrid dense + BM25 over **Atomic Facts**, fused with RRF
2. Score each MemScene by the **max** relevance of its cells; take top **N = 10** scenes
3. Re-rank Episodes inside those scenes; take top **K = 10**
4. Keep only Foresight where `t_now ∈ [t_start, t_end]`
5. LLM **sufficiency check**; if insufficient, rewrite 2–3 complementary queries (pivot, temporal, HyDE, constraint relaxation) and retrieve again. On LoCoMo this second round fired for **31%** of questions ([paper Appendix A.1](https://arxiv.org/html/2601.02163#A1.SS1))

Two task modes share retrieval: **Reasoning** (episodes only) vs **Chat** (episodes + profile + time-valid foresight).

### Results that matter for product choices

- Biggest gains vs strongest baseline: LoCoMo multi-hop **+19.7%**, temporal **+10.0%**, LongMemEval knowledge-update **+20.6%** ([paper Tables 1–2](https://arxiv.org/html/2601.02163#S4.T1))
- Ablation is stepwise: full system > flat MemCells > raw dialogue > no memory ([paper Figure 4](https://arxiv.org/html/2601.02163#S4.F4))
- Profile + episodes beat episodes-only by **9.32 points** on PersonaMem-v2 ([paper Table 4](https://arxiv.org/html/2601.02163#S4.T4))
- Retrieval budget saturates around N=10, K=10 ([paper §4.4](https://arxiv.org/html/2601.02163#S4.SS4))
- Cost: extra LLM calls for construction and retrieval; paper treats async/batch/cache as future work ([paper Limitations](https://arxiv.org/html/2601.02163#S5))

### Canonical failure the system is designed to catch

Fragment retrieval recalls “user likes IPAs” and recommends beer, missing last week’s “on antibiotics for 2 weeks”. Consolidation + valid foresight prefers a non-alcoholic option. ([paper Figure 2](https://arxiv.org/html/2601.02163#S1.F2))

Gospel analog: recall “this repo uses npm” while a later turn established “use bun”; or recommend a mutation while a temporary constraint (“ReadOnly until the review lands”) is still valid.

## What Gospel already has (and does not)

Gospel’s Harness Interface treats **conversation history as implicit memory** and added PLAN.md because “verification and long-horizon progress tracking were limited to what fits in conversational memory” (`CONTEXT.md`). There is **no** user model, **no** cross-session episodic store, **no** summarization, **no** reconstructive retrieval.

| Paper idea | Closest Gospel seam | Gap |
| --- | --- | --- |
| MemCell | `SessionNote` (`session_notes` table) | String blob; no episode/facts/foresight/intervals |
| Segmentation | Session Turn | One turn ≠ one topic; no boundary detector |
| MemScene | `.gospel/PLAN.md` | One markdown file, not clustered episodes |
| User Profile | App-config SQLite (provider visibility, theme) | No agent-facing profile |
| Retrieval | Context Search FTS over **code** | Not conversation; notes are dump-all |
| Sufficiency | Verification Agent (post-turn correctness) | Different job; not “is recalled context enough?” |
| Compaction | `ConversationStore` drop-oldest (50 msgs / 64 KiB) | Drops, never distills |
| Harvest | Skill Optimization reads Display Transcripts | Instruction evolution, not episodic memory |

Critical existing contradiction: `CONTEXT.md` says Session Context Notes are **not** persisted into Model History, but `append_unresolved_notes` suffixes them onto the **user prompt**, so they enter both Display Transcript and Model History on success.

## ADR constraints on any port

- **ADR-0004**: Sessions and private indexed memory stay in **app-global SQLite**, not `.gospel/`. Unscoped sessions have no workspace substrate.
- **ADR-0005**: Do not blur Display Transcript vs Model History. Recalled memory must not ride the user prompt.
- **ADR-0001**: Non-secret preferences live in app-global SQLite.
- **ADR-0002**: Retrieved memory is not source of truth for files; live workspace tools still verify.
- **ADR-0011**: Trace Log is never agent-readable. Harvest Display Transcript (and memory records), never traces.
- **ADR-0008**: Read-Only Sessions must not gain source-mutation via a memory writer.

None forbid a memory lifecycle. They force a split: **private/indexed** in app-global SQLite; **human-inspectable control** in `.gospel/` (PLAN.md stays the PEV artifact).

## Implementable subset (ranked)

Worth implementing in Gospel, adapted — not a Python EverOS embed:

1. **Structured traces from Display Transcript** (Episode + atomic facts + time-bounded foresight), ingested asynchronously after a successful Session Turn, same scheduling shape as the Verification Agent.
2. **Thematic scenes + compact Memory Profile** (explicit facts + implicit traits, recency + conflict tracking), workspace-scoped for affine Sessions, app-global for Unscoped Sessions.
3. **Reconstructive recollection into the Harness Profile preamble** with a token budget: FTS over facts → scene max-pool → episode re-rank → drop expired foresight. Fixes the notes-in-Model-History bug as part of the same seam.
4. **Promote pruned Conversation history into traces** instead of silent drop-oldest.

Defer (paper-important, Gospel-expensive or ADR-hostile):

- Dense embeddings + RRF + dedicated reranker (Qwen3-Embedding/Reranker-4B)
- LLM topic-boundary detector (small gain over Turn/session boundaries)
- Hot-path sufficiency LLM + query rewrite (31% extra round; coding-turn latency)
- Markdown-as-canonical-memory under `.gospel/` (EverOS product choice; fights ADR-0004)
- LoCoMo/LongMemEval harness, Knowledge Wiki, multimodal ingest

## Sources not followed

EverOS README (GitHub) describes a later product (Markdown + SQLite + LanceDB, user vs agent tracks). This note uses the **paper** as the source of truth for the lifecycle. The public repo name/path has drifted from the paper’s `EverMind-AI/EverMemOS` claim.
