// E2E harness: loads the built extension, simulates pi's event flow
// (session_start -> turn_end -> session_start refresh -> before_agent_start)
// against a real daemon, asserting capture + injection. IJIMA_URL + token.
const mod = await import("./index.js");

const events = new Map();
const tools = [];
const pi = {
  registerTool: (t) => tools.push(t),
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

// ---------------------------------------------------------------------------
// v0.4.0 U5: evidence + supersede round-trips against a live daemon.
// Degrade gracefully when IJIMA_URL is unset (skip, never fail).
// ---------------------------------------------------------------------------
if (!process.env.IJIMA_URL) {
  console.log(
    "evidence/supersede round-trips: skipped (IJIMA_URL unset)",
  );
} else {
  const base = mod.resolveIjimaUrl();
  const token = mod.resolveIjimaToken();
  const headers = token ? { Authorization: `Bearer ${token}` } : {};
  const stamp = Date.now();
  const contentA = `E2E evidence probe A ${stamp}`;
  const contentB = `E2E evidence probe B ${stamp}`;
  const saveTool = tools.find((t) => t.name === "memory_save");

  // 4a. Observed + citation → 200 (id comes back on success).
  const resA = await saveTool.execute("e2e", {
    content: contentA,
    project: "general",
    topic: "e2e",
    evidence: "Observed",
    citations: [{ kind: "File", locator: "e2e" }],
  });
  const aText = resA?.content?.[0]?.text ?? "";
  const aId = /Saved memory: (\S+)/.exec(aText)?.[1];
  console.log("save A observed+citation 200:", Boolean(aId));

  // 4b. B supersedes A → 200.
  const resB = await saveTool.execute("e2e", {
    content: contentB,
    project: "general",
    topic: "e2e",
    supersedes: aId,
  });
  const bText = resB?.content?.[0]?.text ?? "";
  const bId = /Saved memory: (\S+)/.exec(bText)?.[1];
  console.log("save B supersedes A 200:", Boolean(bId));

  // 4c. Recall A shows superseded_by == B.id.
  const recallRes = await fetch(
    `${base}${mod.withHome(`/memories/${aId}`)}`,
    { headers },
  );
  const recalled = await recallRes.json();
  console.log(
    "recall A superseded_by == B.id:",
    recalled.superseded_by === bId,
  );

  // 4d. Wake-up omits the superseded A.
  const wakeRes = await fetch(`${base}${mod.withHome("/wakeup")}`, {
    headers,
  });
  const wakeText = await wakeRes.text();
  console.log(
    "wake-up omits superseded A:",
    !wakeText.includes(aId) && !wakeText.includes(contentA),
  );

  // 5. Observed without citations → non-2xx (400).
  const resBad = await saveTool.execute("e2e", {
    content: `E2E observed-no-citation ${stamp}`,
    evidence: "Observed",
  });
  const badText = resBad?.content?.[0]?.text ?? "";
  console.log(
    "observed without citations rejected (400):",
    badText.includes("(400)"),
  );
}
