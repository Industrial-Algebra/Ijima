// Copyright 2026 Industrial Algebra. Licensed under Apache-2.0.
//
// Ijima pi integration — thin TS shim that registers the Ijima memory service
// as pi tools. The wasm core (./pkg/ijima_pi.js) owns all type-safe
// request/response mapping; this file owns HTTP fetch + pi tool registration.

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import {
  build_search_request,
  parse_search_response,
} from "./pkg/ijima_pi.js";

export default function (pi: ExtensionAPI) {

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
    async execute(_toolCallId, params, signal) {
      const ijimaUrl = process.env.IJIMA_URL ?? "http://127.0.0.1:7373";
      const token = process.env.IJIMA_TOKEN_MEMORY_READ;
      if (!token) {
        return {
          content: [
            {
              type: "text",
              text: "Error: IJIMA_TOKEN_MEMORY_READ not set. Configure your Schubert capability token.",
            },
          ],
          details: {},
        };
      }

      const body = build_search_request(
        params.query,
        params.n_results ?? 5,
        undefined, // scope → Rust defaults to "visible"
      );

      try {
        const response = await fetch(
          // Scope travels in the JSON body (scope=visible, set by the wasm
          // core); no query param needed.
          `${ijimaUrl}/memories/search`,
          {
            method: "POST",
            headers: {
              "Content-Type": "application/json",
              Authorization: `Bearer ${token}`,
            },
            body,
            signal,
          },
        );

        if (!response.ok) {
          const errText = await response.text();
          return {
            content: [
              {
                type: "text",
                text: `Ijima search error (${response.status}): ${errText}`,
              },
            ],
            details: {},
          };
        }

        const responseText = await response.text();
        const hitsJson = parse_search_response(responseText);
        const hits: unknown = JSON.parse(hitsJson);

        // Graceful: the wasm core returns { error: "..." } on parse failure
        if (hits && typeof hits === "object" && "error" in hits) {
          return {
            content: [
              {
                type: "text",
                text: `Parse error: ${(hits as { error: string }).error}`,
              },
            ],
            details: {},
          };
        }

        if (!Array.isArray(hits)) {
          return {
            content: [
              { type: "text", text: "No memories found matching your query." },
            ],
            details: {},
          };
        }

        // Client-side project/topic filter: Ijima search is namespace-wide
        // vector similarity; these refine the retrieved set afterwards.
        let matched = hits as Array<Record<string, unknown>>;
        if (params.project) {
          matched = matched.filter((h) => h.project === params.project);
        }
        if (params.topic) {
          matched = matched.filter((h) => h.topic === params.topic);
        }
        if (matched.length === 0) {
          return {
            content: [
              { type: "text", text: "No memories found matching your query." },
            ],
            details: {},
          };
        }

        const lines = matched.map((h, i) => {
          const pct = ((h.similarity as number) * 100).toFixed(1);
          return `${i + 1}. [${pct}%] ${h.text} (${h.project}/${h.topic}, ${h.timestamp})`;
        });

        return {
          content: [{ type: "text", text: lines.join("\n") }],
          details: {},
        };
      } catch (err) {
        // Offline / network error → graceful degradation (§10)
        return {
          content: [
            {
              type: "text",
              text: `Memory unavailable: ${err instanceof Error ? err.message : String(err)}`,
            },
          ],
          details: {},
        };
      }
    },
  });
}
