// Offline unit tests for project-local config resolution (`.pi/ijima.json`)
// and the env > file > default precedence chain. No daemon required.
// Run: node config.test.mjs  (after `npm run build`)

import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

const mod = await import("./index.js");
const { resolveProjectConfig, homeNamespace, withHome, resolveIjimaUrl } = mod;

let pass = 0;
let fail = 0;
function check(name, cond) {
  if (cond) {
    pass++;
    console.log(`  ok  ${name}`);
  } else {
    fail++;
    console.error(`FAIL  ${name}`);
  }
}

const savedEnv = {
  IJIMA_NAMESPACE: process.env.IJIMA_NAMESPACE,
  IJIMA_URL: process.env.IJIMA_URL,
  IJIMA_TOKEN: process.env.IJIMA_TOKEN,
  IJIMA_TOKEN_FILE: process.env.IJIMA_TOKEN_FILE,
  IJIMA_URL_FILE: process.env.IJIMA_URL_FILE,
};
for (const k of Object.keys(savedEnv)) delete process.env[k];

const savedCwd = process.cwd();
const root = fs.mkdtempSync(path.join(os.tmpdir(), "ijima-cfg-"));

function mkdirp(p) {
  fs.mkdirSync(p, { recursive: true });
  return p;
}
function writeCfg(dir, obj) {
  mkdirp(path.join(dir, ".pi"));
  fs.writeFileSync(path.join(dir, ".pi", "ijima.json"), JSON.stringify(obj));
}

try {
  // t1: no config anywhere up from an isolated dir → empty config, personal ns
  const bare = mkdirp(path.join(root, "bare"));
  process.chdir(bare);
  check("t1 no file → empty config", Object.keys(resolveProjectConfig()).length === 0);
  check("t1 no file → namespace default ''", homeNamespace() === "");

  // t2: project file declares a namespace
  const orthant = mkdirp(path.join(root, "orthant"));
  writeCfg(orthant, { namespace: "ns_orthant_shared" });
  process.chdir(orthant);
  check("t2 file namespace read", resolveProjectConfig().namespace === "ns_orthant_shared");
  check("t2 homeNamespace from file", homeNamespace() === "ns_orthant_shared");
  check("t2 withHome appends", withHome("/memories") === "/memories?namespace=ns_orthant_shared");
  check("t2 withHome joins existing query", withHome("/memories?limit=1").includes("&namespace=ns_orthant_shared"));

  // t3: env beats file
  process.env.IJIMA_NAMESPACE = "ns_override";
  check("t3 env overrides file", homeNamespace() === "ns_override");
  delete process.env.IJIMA_NAMESPACE;

  // t4/t5: walk-up from subdirectory; nearest config wins
  process.chdir(mkdirp(path.join(orthant, "deep", "nested")));
  check("t4 walk-up from subdir", homeNamespace() === "ns_orthant_shared");
  writeCfg(path.join(orthant, "deep"), { namespace: "ns_kellas_shared" });
  process.chdir(path.join(orthant, "deep", "nested"));
  check("t5 nearest config wins", homeNamespace() === "ns_kellas_shared");
  process.chdir(orthant);
  check("t5 outer still its own", homeNamespace() === "ns_orthant_shared");

  // t6: invalid JSON ignored silently
  fs.writeFileSync(path.join(orthant, ".pi", "ijima.json"), "{ not json");
  check("t6 invalid JSON ignored", homeNamespace() === "");

  // t7: url + token_file keys
  fs.writeFileSync(path.join(orthant, ".pi", "ijima.json"), JSON.stringify({ url: "http://example:7373" }));
  check("t7 file url when env unset", resolveIjimaUrl() === "http://example:7373");
  process.env.IJIMA_URL = "http://env-wins:1";
  check("t7 env url beats file", resolveIjimaUrl() === "http://env-wins:1");
  delete process.env.IJIMA_URL;

  // t8: ~ expansion in the well-known token file fallback (regression:
  //     readTokenFile used replace("^~") which never matched)
  const tokenHome = fs.mkdtempSync(path.join(os.tmpdir(), "ijima-tok-"));
  fs.mkdirSync(path.join(tokenHome, ".config", "ijima"), { recursive: true });
  fs.writeFileSync(path.join(tokenHome, ".config", "ijima", "token"), "tok-from-file");
  // point the resolver at the fake home via the file candidate env
  process.env.IJIMA_TOKEN_FILE = path.join(tokenHome, ".config", "ijima", "token");
  const tok = mod.resolveIjimaToken();
  check("t8 token via IJIMA_TOKEN_FILE", tok === "tok-from-file");
  delete process.env.IJIMA_TOKEN_FILE;
  // t8b: ~ expansion via the well-known candidate path (IJIMA_HOME
  //     overrides homedir for the test; also fixes the old replace("^~")
  //     bug where the fallback never expanded)
  try {
    process.env.IJIMA_HOME = tokenHome;
    const tok2 = mod.resolveIjimaToken();
    check("t8b ~ expansion in well-known token path", tok2 === "tok-from-file");
  } finally {
    delete process.env.IJIMA_HOME;
    delete process.env.IJIMA_TOKEN_FILE;
    delete process.env.IJIMA_FAKE_HOME;
  }
} finally {
  process.chdir(savedCwd);
  for (const [k, v] of Object.entries(savedEnv)) {
    if (v === undefined) delete process.env[k];
    else process.env[k] = v;
  }
}

console.log(`\n${pass} passed, ${fail} failed`);
process.exit(fail === 0 ? 0 : 1);
