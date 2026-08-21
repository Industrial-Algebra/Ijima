// Copyright 2026 Industrial Algebra. Licensed under Apache-2.0.
//
// Ijima pi integration — thin TS shim that registers the Ijima memory service
// as pi tools. The wasm core (./pkg/ijima_pi.js) owns all type-safe
// request/response mapping; this file owns HTTP fetch + pi tool registration.
import { Type } from "typebox";
import { build_search_request, parse_search_response, build_save_request, parse_save_response, build_check_duplicate_request, parse_check_duplicate_response, build_knowledge_add_request, parse_knowledge_add_response, parse_knowledge_query_response, parse_knowledge_timeline_response, } from "./pkg/ijima_pi.js";
async function ijimaFetch(path, cap, init, signal) {
    const ijimaUrl = process.env.IJIMA_URL ?? "http://127.0.0.1:7373";
    const token = process.env.IJIMA_TOKEN;
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
    }
    catch (err) {
        return {
            ok: false,
            status: 0,
            text: `Memory unavailable: ${err instanceof Error ? err.message : String(err)}`,
        };
    }
}
function errorContent(status, text) {
    return {
        content: [
            {
                type: "text",
                text: `Ijima error (${status}): ${text}`,
            },
        ],
        details: {},
    };
}
function parseError(msg) {
    return {
        content: [{ type: "text", text: `Parse error: ${msg}` }],
        details: {},
    };
}
// ---------------------------------------------------------------------------
// Extension entry point
// ---------------------------------------------------------------------------
export default function (pi) {
    // ----- memory_search (POST /memories/search, memory:read) -----
    pi.registerTool({
        name: "memory_search",
        label: "Memory Search",
        description: "Search persistent agent memory across projects using semantic similarity." +
            " Finds past conversations, decisions, and context matching the query.",
        parameters: Type.Object({
            query: Type.String({
                description: "What to search for (natural language)",
            }),
            project: Type.Optional(Type.String({ description: "Filter to a specific project" })),
            topic: Type.Optional(Type.String({ description: "Filter to a specific topic" })),
            n_results: Type.Optional(Type.Number({ description: "Number of results (default: 5, max: 20)" })),
        }),
        async execute(_tid, params, signal) {
            const body = build_search_request(params.query, params.n_results ?? 5, undefined);
            const { ok, status, text } = await ijimaFetch("/memories/search", "memory:read", { method: "POST", body }, signal);
            if (!ok)
                return errorContent(status, text);
            const hits = JSON.parse(parse_search_response(text));
            if (hits && typeof hits === "object" && "error" in hits) {
                return parseError(hits.error);
            }
            if (!Array.isArray(hits)) {
                return {
                    content: [
                        { type: "text", text: "No memories found matching your query." },
                    ],
                    details: {},
                };
            }
            let matched = hits;
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
            const lines = matched.map((h, i) => `${i + 1}. [${(h.similarity * 100).toFixed(1)}%] ${h.text} (${h.project}/${h.topic}, ${h.timestamp})`);
            return { content: [{ type: "text", text: lines.join("\n") }], details: {} };
        },
    });
    // ----- memory_save (POST /memories, memory:write) -----
    pi.registerTool({
        name: "memory_save",
        label: "Memory Save",
        description: "Explicitly save a piece of information to persistent memory." +
            " Use for important decisions, facts, or context to remember across sessions.",
        parameters: Type.Object({
            content: Type.String({
                description: "The information to remember (include context)",
            }),
            project: Type.Optional(Type.String({ description: "Project this belongs to" })),
            topic: Type.Optional(Type.String({
                description: "Topic category (e.g. 'auth', 'database', 'architecture')",
            })),
            importance: Type.Optional(Type.Number({
                description: "Importance weight 0.0-1.0 (default: 0.8 for manual saves). Higher = more likely to appear in wake-up.",
            })),
        }),
        async execute(_tid, params, signal) {
            const id = `mem_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
            const body = build_save_request(id, params.content, params.project ?? "general", params.topic ?? "general", params.importance);
            const { ok, status, text } = await ijimaFetch("/memories", "memory:write", { method: "POST", body }, signal);
            if (!ok)
                return errorContent(status, text);
            const result = JSON.parse(parse_save_response(text));
            if (result.error)
                return parseError(result.error);
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
            const { ok, status, text } = await ijimaFetch(`/memories/${params.id}`, "memory:write", { method: "DELETE" }, signal);
            if (!ok)
                return errorContent(status, text);
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
        description: "Check if content already exists in memory before storing." +
            " Returns the existing ID or null.",
        parameters: Type.Object({
            content: Type.String({ description: "Content to check for duplicates" }),
            threshold: Type.Optional(Type.Number({
                description: "Similarity threshold 0-1 (not used by Ijima — exact content-hash dedup)",
            })),
        }),
        async execute(_tid, params, signal) {
            const body = build_check_duplicate_request(params.content);
            const { ok, status, text } = await ijimaFetch("/memories/check", "memory:read", { method: "POST", body }, signal);
            if (!ok)
                return errorContent(status, text);
            const result = JSON.parse(parse_check_duplicate_response(text));
            if (result.error)
                return parseError(result.error);
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
                description: "The subject entity (e.g. 'myapp', 'Alice')",
            }),
            predicate: Type.String({
                description: "The relationship (e.g. 'uses', 'depends_on', 'decided')",
            }),
            object: Type.String({
                description: "The object entity (e.g. 'PostgreSQL', 'React')",
            }),
            valid_from: Type.Optional(Type.String({
                description: "When this fact became true (ISO date)",
            })),
            valid_to: Type.Optional(Type.String({
                description: "Ignored by Ijima — accepted for pi-mempalace compat",
            })),
            project: Type.Optional(Type.String({
                description: "Ignored by Ijima — KG is namespace-scoped server-side",
            })),
        }),
        async execute(_tid, params, signal) {
            const body = build_knowledge_add_request(params.subject, params.predicate, params.object, params.valid_from ?? null, null);
            const { ok, status, text } = await ijimaFetch("/kg/triples", "knowledge:write", { method: "POST", body }, signal);
            if (!ok)
                return errorContent(status, text);
            const triple = JSON.parse(parse_knowledge_add_response(text));
            if (triple.error)
                return parseError(triple.error);
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
            at_time: Type.Optional(Type.String({
                description: "Ignored by Ijima — accepted for pi-mempalace compat",
            })),
            project: Type.Optional(Type.String({
                description: "Ignored by Ijima — accepted for pi-mempalace compat",
            })),
        }),
        async execute(_tid, params, signal) {
            const { ok, status, text } = await ijimaFetch(`/kg/entities/${params.entity}`, "knowledge:read", undefined, signal);
            if (!ok)
                return errorContent(status, text);
            const rec = JSON.parse(parse_knowledge_query_response(text));
            if (rec.error)
                return parseError(rec.error);
            const name = rec.entity_name ?? params.entity;
            const lines = [
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
            const { ok, status, text } = await ijimaFetch("/kg/stats", "knowledge:read", undefined, signal);
            if (!ok)
                return errorContent(status, text);
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
        description: "Mark a knowledge graph fact as no longer true." +
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
            ended: Type.Optional(Type.String({
                description: "When it stopped being true (ignored by Ijima, accepts for compat)",
            })),
        }),
        async execute(_tid, params, signal) {
            // Step 1: find the triple
            const qs = `subject=${encodeURIComponent(params.subject)}&predicate=${encodeURIComponent(params.predicate)}&object=${encodeURIComponent(params.object)}`;
            const find = await ijimaFetch(`/kg/triples?${qs}`, "knowledge:read", undefined, signal);
            if (!find.ok)
                return errorContent(find.status, find.text);
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
            const inv = await ijimaFetch(`/kg/triples/${tripleId}/invalidate`, "knowledge:write", { method: "POST" }, signal);
            if (!inv.ok)
                return errorContent(inv.status, inv.text);
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
            entity: Type.Optional(Type.String({
                description: "Ignored by Ijima — accepts for pi-mempalace compat",
            })),
        }),
        async execute(_tid, _params, signal) {
            const { ok, status, text } = await ijimaFetch("/kg/timeline", "knowledge:read", undefined, signal);
            if (!ok)
                return errorContent(status, text);
            const triples = JSON.parse(parse_knowledge_timeline_response(text));
            if (triples.error)
                return parseError(triples.error);
            if (!Array.isArray(triples) || triples.length === 0) {
                return {
                    content: [
                        { type: "text", text: "Knowledge graph timeline is empty." },
                    ],
                    details: {},
                };
            }
            const lines = triples.map((t) => {
                const from = t.valid_from ? ` (${t.valid_from})` : "";
                const to = t.valid_to ? ` → ${t.valid_to}` : "";
                return `${t.subject} ${t.predicate} ${t.object}${from}${to}`;
            });
            return {
                content: [{ type: "text", text: lines.join("\n") }],
                details: {},
            };
        },
    });
}
