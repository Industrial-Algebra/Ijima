// E2E harness: loads the built extension, simulates pi's event flow
// (session_start -> turn_end -> session_start refresh -> before_agent_start)
// against a real daemon, asserting capture + injection. IJIMA_URL + token.
const mod = await import("./index.js");

const events = new Map();
const tools = [];
const pi = {
  registerTool: (t) => tools.push(t.name),
  on: (name, handler) => events.set(name, handler),
};
mod.default(pi);
console.log("tools:", tools.length, "| events:", [...events.keys()].join(", "));

const branch = [
  {
    type: "message",
    message: { role: "user", content: "E2E probe: does autocapture fire?" },
  },
  {
    type: "message",
    message: {
      role: "assistant",
      content: [
        {
          type: "text",
          text: "Yes — this assistant turn is long enough to clear the twenty-character gate for capture.",
        },
      ],
    },
  },
];
const ctx = {
  cwd: "/work/project",
  sessionManager: {
    getBranch: () => branch,
    getSessionId: () => "sess_e2e",
  },
};

// 1. initial wake-up refresh
await events.get("session_start")({}, ctx);

// 2. auto-capture the exchange (dedup absorbs the prior run's copy)
await events.get("turn_end")({ message: branch[1].message }, ctx);

// 3. refresh wake-up again — the capture must now be in the essentials
await events.get("session_start")({}, ctx);

// 4. prompt injection
const res = await events.get("before_agent_start")(
  { systemPrompt: "BASE PROMPT." },
  ctx,
);
const injected = res?.systemPrompt ?? "";
console.log(
  "injection applied:",
  injected.includes("Agent Memory (ACTIVE)"),
);
console.log(
  "captures itself:",
  injected.includes("E2E probe"),
);
console.log("prompt length:", injected.length);
