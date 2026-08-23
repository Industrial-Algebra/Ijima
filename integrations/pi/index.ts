// Copyright 2026 Industrial Algebra. Licensed under Apache-2.0.
//
// Ijima pi integration — thin TS shim that registers the Ijima memory service
// as pi tools. The wasm core (./pkg/ijima_pi.js) owns all type-safe
// request/response mapping; this file owns HTTP fetch + pi tool registration.

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import * as fs from "node:fs";
import * as os from "node:os";
import {
  build_search_request,
  parse_search_response,
  build_save_request,
  parse_save_response,
  build_check_duplicate_request,
  parse_check_duplicate_response,
  build_knowledge_add_request,
  parse_knowledge_add_response,
  parse_knowledge_query_response,
  parse_knowledge_timeline_response,
} from "./pkg/ijima_pi.js";

// ---------------------------------------------------------------------------
// fetch helper — the shared boilerplate refactored from memory_search
// ---------------------------------------------------------------------------

type Capability =
  | "memory:read"
  | "memory:write"
  | "knowledge:read"
  | "knowledge:write";

// One multi-capability Schubert GrantToken (env `IJIMA_TOKEN`) admits
// every route below; the server's geometric `may()` checks containment
// per-request. See ijimaFetch().

/** Token-file candidates, first hit wins (0600 files, user-owned). */
const TOKEN_FILES = [
  process.env.IJIMA_TOKEN_FILE,
  "~/.config/ijima/token",
].filter((p): p is string => Boolean(p));

function readTokenFile(): string | null {
  for (const raw of TOKEN_FILES) {
    const path = raw.replace("^~", os.homedir());
    try {
      const t = fs.readFileSync(path, "utf8").trim();
      if (t) return t;
    } catch {
      // absent/unreadable — next candidate
    }
  }
  return null;
}

interface FetchResult {
  ok: boolean;
  status: number;
  text: string;
}

async function ijimaFetch(
  path: string,
  cap: Capability,
  init?: RequestInit,
  signal?: AbortSignal,
): Promise<FetchResult> {
  const ijimaUrl = process.env.IJIMA_URL ?? "http://127.0.0.1:7373";
  // Token resolution: env first, then the well-known private file. The
  // fallback kills the whole "shell didn't export it" failure class —
  // systemd units, cron, agent tabs, non-login shells all just work.
  const token =
    process.env.IJIMA_TOKEN ?? readTokenFile();
  if (!token) {
    return {
      ok: false,
      status: 0,
      text: `Error: IJIMA_TOKEN not set. Mint a grant token (route requires '${cap}') with: ijima token issue --principal <p> --capabilities memory:read,memory:write,knowledge:read,knowledge:write`,
    };
  }
  try {
    const response = await fetch(`${ijimaUrl}${path}`, {
      ...init,
      signal,
      headers: {
        ...(init?.body ? { "Content-Type": "application/json" } : {}),
        Authorization: `Bearer ${token}`,
        ...init?.headers,
      },
    });
    return {
      ok: response.ok,
      status: response.status,
      text: await response.text(),
    };
  } catch (err) {
    return {
      ok: false,
      status: 0,
      text: `Memory unavailable: ${err instanceof Error ? err.message : String(err)}`,
    };
  }
}

function errorContent(status: number, text: string) {
  return {
    content: [
      {
        type: "text" as const,
        text: `Ijima error (${status}): ${text}`,
      },
    ],
    details: {},
  };
}

function parseError(msg: string) {
  return {
    content: [{ type: "text" as const, text: `Parse error: ${msg}` }],
    details: {},
  };
}

// ---------------------------------------------------------------------------
// Extension entry point
// ---------------------------------------------------------------------------

export default function (pi: ExtensionAPI) {
  // ----- memory_search (POST /memories/search, memory:read) -----
  pi.registerTool({
    name: "memory_search",
    label: "Memory Search",
    description:
      "Search persistent agent memory across projects using semantic similarity." +
      " Finds past conversations, decisions, and context matching the query.",
    parameters: Type.Object({
      query: Type.String({
        description: "What to search for (natural language)",
      }),
      project: Type.Optional(
        Type.String({ description: "Filter to a specific project" }),
      ),
      topic: Type.Optional(
        Type.String({ description: "Filter to a specific topic" }),
      ),
      n_results: Type.Optional(
        Type.Number({ description: "Number of results (default: 5, max: 20)" }),
      ),
    }),
    async execute(_tid, params, signal) {
      const body = build_search_request(
        params.query,
        params.n_results ?? 5,
        undefined, // scope → Rust defaults to "visible"
      );
      const { ok, status, text } = await ijimaFetch(
        "/memories/search",
        "memory:read",
        { method: "POST", body },
        signal,
      );
      if (!ok) return errorContent(status, text);

      const hits: unknown = JSON.parse(parse_search_response(text));
      if (hits && typeof hits === "object" && "error" in hits) {
        return parseError((hits as { error: string }).error);
      }
      if (!Array.isArray(hits)) {
        return {
          content: [
            { type: "text", text: "No memories found matching your query." },
          ],
          details: {},
        };
      }

      let matched = hits as Array<Record<string, unknown>>;
      if (params.project)
        matched = matched.filter((h) => h.project === params.project);
      if (params.topic)
        matched = matched.filter((h) => h.topic === params.topic);
      if (matched.length === 0) {
        return {
          content: [
            { type: "text", text: "No memories found matching your query." },
          ],
          details: {},
        };
      }

      const lines = matched.map(
        (h, i) =>
          `${i + 1}. [${((h.similarity as number) * 100).toFixed(1)}%] ${h.text} (${h.project}/${h.topic}, ${h.timestamp})`,
      );
      return { content: [{ type: "text", text: lines.join("\n") }], details: {} };
    },
  });

  // ----- memory_save (POST /memories, memory:write) -----
  pi.registerTool({
    name: "memory_save",
    label: "Memory Save",
    description:
      "Explicitly save a piece of information to persistent memory." +
      " Use for important decisions, facts, or context to remember across sessions.",
    parameters: Type.Object({
      content: Type.String({
        description: "The information to remember (include context)",
      }),
      project: Type.Optional(
        Type.String({ description: "Project this belongs to" }),
      ),
      topic: Type.Optional(
        Type.String({
          description:
            "Topic category (e.g. 'auth', 'database', 'architecture')",
        }),
      ),
      importance: Type.Optional(
        Type.Number({
          description:
            "Importance weight 0.0-1.0 (default: 0.8 for manual saves). Higher = more likely to appear in wake-up.",
        }),
      ),
    }),
    async execute(_tid, params, signal) {
      const id = `mem_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
      const body = build_save_request(
        id,
        params.content,
        params.project ?? "general",
        params.topic ?? "general",
        params.importance,
      );
      const { ok, status, text } = await ijimaFetch(
        "/memories",
        "memory:write",
        { method: "POST", body },
        signal,
      );
      if (!ok) return errorContent(status, text);

      const result = JSON.parse(parse_save_response(text));
      if (result.error) return parseError(result.error);
      return {
        content: [
          { type: "text", text: `Saved memory: ${result.id}` },
        ],
        details: {},
      };
    },
  });

  // ----- memory_delete (DELETE /memories/{id}, memory:write) -----
  pi.registerTool({
    name: "memory_delete",
    label: "Memory Delete",
    description: "Delete a specific memory by ID. Irreversible.",
    parameters: Type.Object({
      id: Type.String({
        description: "The memory ID to delete (e.g. 'mem_abc123')",
      }),
    }),
    async execute(_tid, params, signal) {
      const { ok, status, text } = await ijimaFetch(
        `/memories/${params.id}`,
        "memory:write",
        { method: "DELETE" },
        signal,
      );
      if (!ok) return errorContent(status, text);
      return {
        content: [
          { type: "text", text: `Deleted memory: ${params.id}` },
        ],
        details: {},
      };
    },
  });

  // ----- memory_check_duplicate (POST /memories/check, memory:read) -----
  pi.registerTool({
    name: "memory_check_duplicate",
    label: "Memory Check Duplicate",
    description:
      "Check if content already exists in memory before storing." +
      " Returns the existing ID or null.",
    parameters: Type.Object({
      content: Type.String({ description: "Content to check for duplicates" }),
      threshold: Type.Optional(
        Type.Number({
          description:
            "Similarity threshold 0-1 (not used by Ijima — exact content-hash dedup)",
        }),
      ),
    }),
    async execute(_tid, params, signal) {
      const body = build_check_duplicate_request(params.content);
      const { ok, status, text } = await ijimaFetch(
        "/memories/check",
        "memory:read",
        { method: "POST", body },
        signal,
      );
      if (!ok) return errorContent(status, text);

      const result = JSON.parse(parse_check_duplicate_response(text));
      if (result.error) return parseError(result.error);
      if (result.duplicate) {
        return {
          content: [
            { type: "text", text: `Duplicate found: ${result.duplicate}` },
          ],
          details: {},
        };
      }
      return {
        content: [{ type: "text", text: "No duplicate found." }],
        details: {},
      };
    },
  });

  // ----- knowledge_add (POST /kg/triples, knowledge:write) -----
  pi.registerTool({
    name: "knowledge_add",
    label: "Knowledge Add",
    description: "Add a structured fact (triple) to the knowledge graph.",
    parameters: Type.Object({
      subject: Type.String({
        description:
          "The subject entity (e.g. 'myapp', 'Alice')",
      }),
      predicate: Type.String({
        description:
          "The relationship (e.g. 'uses', 'depends_on', 'decided')",
      }),
      object: Type.String({
        description:
          "The object entity (e.g. 'PostgreSQL', 'React')",
      }),
      valid_from: Type.Optional(
        Type.String({
          description: "When this fact became true (ISO date)",
        }),
      ),
      valid_to: Type.Optional(
        Type.String({
          description: "Ignored by Ijima — accepted for pi-mempalace compat",
        }),
      ),
      project: Type.Optional(
        Type.String({
          description: "Ignored by Ijima — KG is namespace-scoped server-side",
        }),
      ),
    }),
    async execute(_tid, params, signal) {
      const body = build_knowledge_add_request(
        params.subject,
        params.predicate,
        params.object,
        params.valid_from ?? null,
        null, // confidence defaults to 1.0 in Rust
      );
      const { ok, status, text } = await ijimaFetch(
        "/kg/triples",
        "knowledge:write",
        { method: "POST", body },
        signal,
      );
      if (!ok) return errorContent(status, text);

      const triple = JSON.parse(parse_knowledge_add_response(text));
      if (triple.error) return parseError(triple.error);
      return {
        content: [
          {
            type: "text",
            text: `Added fact: ${triple.subject} ${triple.predicate} ${triple.object} (${triple.id})${triple.valid_from ? `, valid from ${triple.valid_from}` : ""}`,
          },
        ],
        details: {},
      };
    },
  });

  // ----- knowledge_query (GET /kg/entities/{id}, knowledge:read) -----
  pi.registerTool({
    name: "knowledge_query",
    label: "Knowledge Query",
    description: "Query facts about an entity.",
    parameters: Type.Object({
      entity: Type.String({
        description: "The entity to query (e.g. 'myapp', 'PostgreSQL')",
      }),
      at_time: Type.Optional(
        Type.String({
          description: "Ignored by Ijima — accepted for pi-mempalace compat",
        }),
      ),
      project: Type.Optional(
        Type.String({
          description: "Ignored by Ijima — accepted for pi-mempalace compat",
        }),
      ),
    }),
    async execute(_tid, params, signal) {
      const { ok, status, text } = await ijimaFetch(
        `/kg/entities/${params.entity}`,
        "knowledge:read",
        undefined,
        signal,
      );
      if (!ok) return errorContent(status, text);

      const rec = JSON.parse(parse_knowledge_query_response(text));
      if (rec.error) return parseError(rec.error);

      const name = rec.entity_name ?? params.entity;
      const lines: string[] = [
        `Entity: ${name} (${rec.entity_type ?? "unknown"})`,
        `  Outgoing: ${rec.outgoing.length} triples`,
        `  Incoming: ${rec.incoming.length} triples`,
      ];
      for (const t of [...rec.outgoing, ...rec.incoming].slice(0, 15)) {
        const ts = t.valid_from ? ` (${t.valid_from})` : "";
        lines.push(`  ${t.subject} ${t.predicate} ${t.object}${ts}`);
      }
      return {
        content: [{ type: "text", text: lines.join("\n") }],
        details: {},
      };
    },
  });

  // ----- knowledge_status (GET /kg/stats, knowledge:read) -----
  pi.registerTool({
    name: "knowledge_status",
    label: "Knowledge Status",
    description: "Overview of the knowledge graph.",
    parameters: Type.Object({}),
    async execute(_tid, _params, signal) {
      const { ok, status, text } = await ijimaFetch(
        "/kg/stats",
        "knowledge:read",
        undefined,
        signal,
      );
      if (!ok) return errorContent(status, text);

      const stats = JSON.parse(text);
      return {
        content: [
          {
            type: "text",
            text: `Knowledge graph: ${stats.entities} entities, ${stats.triples} triples`,
          },
        ],
        details: {},
      };
    },
  });

  // ----- knowledge_invalidate (POST /kg/triples/{id}/invalidate, knowledge:write) -----
  // Two-step: find the triple by (subject,predicate,object), then invalidate it.
  pi.registerTool({
    name: "knowledge_invalidate",
    label: "Knowledge Invalidate",
    description:
      "Mark a knowledge graph fact as no longer true." +
      " Finds by subject/predicate/object, then invalidates by id.",
    parameters: Type.Object({
      subject: Type.String({
        description: "The subject entity",
      }),
      predicate: Type.String({
        description: "The relationship",
      }),
      object: Type.String({
        description: "The object entity",
      }),
      ended: Type.Optional(
        Type.String({
          description: "When it stopped being true (ignored by Ijima, accepts for compat)",
        }),
      ),
    }),
    async execute(_tid, params, signal) {
      // Step 1: find the triple
      const qs = `subject=${encodeURIComponent(params.subject)}&predicate=${encodeURIComponent(params.predicate)}&object=${encodeURIComponent(params.object)}`;
      const find = await ijimaFetch(
        `/kg/triples?${qs}`,
        "knowledge:read",
        undefined,
        signal,
      );
      if (!find.ok) return errorContent(find.status, find.text);

      const triples = JSON.parse(find.text);
      if (!Array.isArray(triples) || triples.length === 0) {
        return {
          content: [
            {
              type: "text",
              text: `No fact found matching: ${params.subject} ${params.predicate} ${params.object}`,
            },
          ],
          details: {},
        };
      }

      // Step 2: invalidate the first match
      const tripleId = triples[0].id;
      const inv = await ijimaFetch(
        `/kg/triples/${tripleId}/invalidate`,
        "knowledge:write",
        { method: "POST" },
        signal,
      );
      if (!inv.ok) return errorContent(inv.status, inv.text);
      return {
        content: [
          {
            type: "text",
            text: `Invalidated fact: ${tripleId} (${params.subject} ${params.predicate} ${params.object})`,
          },
        ],
        details: {},
      };
    },
  });

  // ----- knowledge_timeline (GET /kg/timeline, knowledge:read) -----
  pi.registerTool({
    name: "knowledge_timeline",
    label: "Knowledge Timeline",
    description: "Chronological timeline of facts in the knowledge graph.",
    parameters: Type.Object({
      entity: Type.Optional(
        Type.String({
          description: "Ignored by Ijima — accepts for pi-mempalace compat",
        }),
      ),
    }),
    async execute(_tid, _params, signal) {
      const { ok, status, text } = await ijimaFetch(
        "/kg/timeline",
        "knowledge:read",
        undefined,
        signal,
      );
      if (!ok) return errorContent(status, text);

      const triples = JSON.parse(parse_knowledge_timeline_response(text));
      if (triples.error) return parseError(triples.error);
      if (!Array.isArray(triples) || triples.length === 0) {
        return {
          content: [
            { type: "text", text: "Knowledge graph timeline is empty." },
          ],
          details: {},
        };
      }

      const lines = triples.map(
        (t: Record<string, unknown>) => {
          const from = t.valid_from ? ` (${t.valid_from})` : "";
          const to = t.valid_to ? ` → ${t.valid_to}` : "";
          return `${t.subject} ${t.predicate} ${t.object}${from}${to}`;
        },
      );
      return {
        content: [{ type: "text", text: lines.join("\n") }],
        details: {},
      };
    },
  });

  // ---------------------------------------------------------------------
  // Auto-capture + wake-up (pi-mempalace parity) — the loop-closers.
  // Without these the tools exist but nothing reminds the agent they're
  // there, and conversations evaporate unless saved by hand.
  // ---------------------------------------------------------------------

  let wakeUpText: string | null = null;

  const refreshWakeUp = async (): Promise<void> => {
    const { ok, text } = await ijimaFetch("/wakeup", "memory:read", {
      method: "GET",
    });
    if (!ok) {
      wakeUpText = null;
      return;
    }
    try {
      const w = JSON.parse(text) as {
        identity?: unknown;
        personal_essentials?: Array<{ content?: string }>;
        doctrine?: Array<{ content?: string }>;
      };
      const parts: string[] = [];
      if (Array.isArray(w.personal_essentials) && w.personal_essentials.length) {
        parts.push(
          "### Essentials (from your memory)\n" +
            w.personal_essentials
              .slice(0, 20)
              .map((m) => `- ${(m.content ?? "").slice(0, 200)}`)
              .join("\n"),
        );
      }
      if (Array.isArray(w.doctrine) && w.doctrine.length) {
        parts.push(
          "### Doctrine\n" +
            w.doctrine
              .slice(0, 20)
              .map((m) => `- ${(m.content ?? "").slice(0, 200)}`)
              .join("\n"),
        );
      }
      wakeUpText = parts.length ? parts.join("\n\n") : null;
    } catch {
      wakeUpText = null;
    }
  };

  pi.on("session_start", async () => {
    await refreshWakeUp();
  });

  // Auto-capture: after each assistant turn, store the exchange. The
  // extension does the remembering — the agent never has to remember to.
  // Gates mirror pi-mempalace: min lengths, 2000-char truncation, silent
  // failure (capture must never interrupt a session). Lands as
  // AutoCapture provenance server-side; dedup handles repeats.
  pi.on("turn_end", async (event: any, ctx: any) => {
    if (event?.message?.role !== "assistant") return;
    const assistantText = extractText(event?.message?.content);
    if (!assistantText || assistantText.length < 20) return;

    let userText = "";
    try {
      const branch = ctx?.sessionManager?.getBranch?.() ?? [];
      for (let i = branch.length - 1; i >= 0; i--) {
        const entry = branch[i];
        if (entry?.type === "message" && entry?.message?.role === "user") {
          userText = extractText(entry.message.content);
          break;
        }
      }
    } catch {
      // branch introspection is best-effort
    }
    if (!userText || userText.length < 10) return;

    let exchange = `> ${userText}\n\n${assistantText}`;
    if (exchange.length > 2000) exchange = exchange.slice(0, 2000) + "\n[truncated]";

    const project = currentProjectName(ctx);
    const sessionId =
      ctx?.sessionManager?.getSessionId?.() ??
      `sess_${Date.now()}`;
    const id = `mem_${Date.now().toString(36)}_${Math.random()
      .toString(36)
      .slice(2, 8)}`;
    // build_save_request hardcodes Explicit/0.8 (manual-save shape);
    // auto-capture rewrites the two fields the tiers care about.
    const body = JSON.parse(
      build_save_request(id, exchange, project, "general", 0.5),
    );
    body.source = "AutoCapture";
    body.session_id = sessionId;
    body.created_at = new Date().toISOString();
    try {
      await ijimaFetch("/memories", "memory:write", {
        method: "POST",
        body: JSON.stringify(body),
      });
    } catch {
      // silently fail — never interrupt the session
    }
  });

  // Wake-up + tool reminder, injected into every system prompt.
  pi.on("before_agent_start", async (event: any) => {
    // The reminder is unconditional — it matters MOST for fresh principals
    // (no history yet, nothing to recall). Wake-up context appends when
    // present; the field-reported 0.2.3 flaw skipped the whole injection
    // when wake-up was empty, so cold principals never saw the reminder.
    const reminder =
      "\n\n## Agent Memory (ACTIVE)\n" +
      "You have persistent memory across sessions, backed by the Ijima memory service.\n" +
      "Use `memory_search` to find past context (try it before concluding anything is new or unknown).\n" +
      "Use `memory_save` to explicitly remember important decisions, facts, or context.\n" +
      "Use `knowledge_add` for structured facts (X predicate Y) and `knowledge_query` to query them.\n" +
      "Conversations are auto-captured at low trust; `memory_save` marks what deserves attention.\n\n" +
      "### Memory model\n" +
      "- Memories live in **namespaces**: your personal one starts empty by design; bulk legacy\n" +
      "  corpora live in `ns_import_*` staging; org walls need membership. An \"empty\" result is\n" +
      "  usually correct scoping — never conclude the store is broken or misrouted from probes.\n" +
      "- `memory_search` spans everything you can read. Use it before concluding anything is new.\n" +
      "- Wake-up context self-primes: auto-captured exchanges surface as essentials next session." ;
    return {
      systemPrompt: event.systemPrompt + reminder + (wakeUpText ? "\n\n" + wakeUpText : ""),
    };
  });
}

function extractText(content: unknown): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((part: any) =>
        typeof part === "string" ? part : (part?.text ?? ""),
      )
      .filter((s: string) => typeof s === "string")
      .join(" ")
      .trim();
  }
  return "";
}

function currentProjectName(ctx: any): string {
  try {
    const cwd = ctx?.cwd ?? process.cwd();
    const base = String(cwd).split("/").filter(Boolean).pop();
    return base ? base.toLowerCase().slice(0, 60) : "general";
  } catch {
    return "general";
  }
}
