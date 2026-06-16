#!/usr/bin/env node
"use strict";

const { SpriteBuilder, ApiError, DEFAULT_BASE_URL } = require("../src/client");

// ---- tiny arg parser (no deps) -------------------------------------------
// Splits argv into positionals + flags. Supports `--flag value`, `--flag=value`,
// and boolean flags (`--public`). Unknown booleans are fine; value flags are
// listed so we know to consume the next token.
const VALUE_FLAGS = new Set(["token", "base-url", "status", "project", "limit", "tail"]);

function parseArgs(argv) {
  const positionals = [];
  const flags = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith("--")) {
      const eq = a.indexOf("=");
      if (eq !== -1) {
        flags[a.slice(2, eq)] = a.slice(eq + 1);
      } else {
        const name = a.slice(2);
        if (VALUE_FLAGS.has(name)) {
          flags[name] = argv[++i];
        } else {
          flags[name] = true;
        }
      }
    } else {
      positionals.push(a);
    }
  }
  return { positionals, flags };
}

// ---- output helpers -------------------------------------------------------
const useColor = process.stdout.isTTY && !process.env.NO_COLOR;
const c = (code, s) => (useColor ? `\x1b[${code}m${s}\x1b[0m` : String(s));
const dim = (s) => c("2", s);
const bold = (s) => c("1", s);
const STATUS_COLOR = { succeeded: "32", failed: "31", running: "33", queued: "90" };
const status = (s) => c(STATUS_COLOR[s] || "0", s);

function die(msg, code = 1) {
  console.error(c("31", "error: ") + msg);
  process.exit(code);
}

const HELP = `sprite-builder — CLI for the sprite-builder deploy service

Usage:
  sprite-builder <command> [options]

Commands:
  me                              Show the authenticated user
  projects                        List your projects
  builds [--status S] [--project ID] [--limit N]
                                  List builds (newest first) across all projects
  logs <buildId> [--tail N]       Deploy (build) log for a build
  runtime <buildId>               Runtime log (docker logs) of the live deployment
  url <buildId> [--public|--private]
                                  Show or change a deployment's URL visibility
  env <projectId>                 List a project's environment variables
  env-set <projectId> <KEY> <VAL> Set/replace an environment variable
  env-rm <projectId> <KEY>        Delete an environment variable

Global options:
  --token <t>      API token (or env SPRITE_BUILDER_TOKEN)
  --base-url <u>   API base URL (or env SPRITE_BUILDER_URL)
                   default: ${DEFAULT_BASE_URL}
  --json           Emit raw JSON instead of formatted text
  -h, --help       Show this help
  --version        Show version

Build ids may be given as a unique prefix (e.g. "fbaab").
`;

function printLog(text, tail) {
  const lines = (text || "").split("\n");
  const shown = tail ? lines.slice(-Number(tail)) : lines;
  process.stdout.write(shown.join("\n"));
  if (shown.length && !shown[shown.length - 1].endsWith("\n")) process.stdout.write("\n");
}

// ---- commands -------------------------------------------------------------
async function main() {
  const argv = process.argv.slice(2);
  const { positionals, flags } = parseArgs(argv);
  const cmd = positionals[0];

  if (flags.version) {
    console.log(require("../package.json").version);
    return;
  }
  if (!cmd || cmd === "help" || flags.help || flags.h) {
    process.stdout.write(HELP);
    return;
  }

  const client = new SpriteBuilder({
    baseUrl: flags["base-url"] || process.env.SPRITE_BUILDER_URL,
    token: flags.token || process.env.SPRITE_BUILDER_TOKEN,
  });
  const json = (v) => console.log(JSON.stringify(v, null, 2));

  switch (cmd) {
    case "me": {
      const me = await client.me();
      if (flags.json) return json(me);
      console.log(`${bold(me.github_login)}  (${me.role})`);
      console.log(dim("capabilities: ") + (me.capabilities || []).join(", "));
      return;
    }

    case "projects": {
      const ps = await client.projects();
      if (flags.json) return json(ps);
      if (!ps.length) return console.log(dim("(no projects)"));
      for (const p of ps) {
        console.log(`${bold(p.name)}  ${dim(p.id)}`);
        console.log(`  ${p.repo_full_name} · ${p.default_branch} · :${p.container_port}`);
      }
      return;
    }

    case "builds": {
      let builds;
      if (flags.project) {
        const ps = await client.builds(flags.project);
        builds = ps.map((b) => ({ ...b, project_id: flags.project }));
      } else {
        builds = await client.allBuilds();
      }
      if (flags.status) builds = builds.filter((b) => b.status === flags.status);
      if (flags.limit) builds = builds.slice(0, Number(flags.limit));
      if (flags.json) return json(builds);
      if (!builds.length) return console.log(dim("(no builds)"));
      for (const b of builds) {
        const url = b.url ? dim(" " + b.url) : "";
        console.log(
          `${b.id.slice(0, 8)}  ${status(b.status).padEnd(useColor ? 18 : 9)}  ` +
            `${(b.commit_sha || "").slice(0, 10)}  ${dim(b.created_at)}` +
            (b.project_name ? `  ${dim("[" + b.project_name + "]")}` : "") +
            url
        );
      }
      return;
    }

    case "logs": {
      if (!positionals[1]) die("usage: sprite-builder logs <buildId>");
      const b = await client.resolveBuild(positionals[1]);
      const full = await client.build(b.id);
      if (flags.json) return json(full);
      console.error(
        dim(`# build ${b.id.slice(0, 8)} · ${full.status} · ${full.sprite_name || "-"}`)
      );
      printLog(full.logs, flags.tail);
      if (full.error) console.error(c("31", "\nerror: ") + full.error);
      return;
    }

    case "runtime": {
      if (!positionals[1]) die("usage: sprite-builder runtime <buildId>");
      const b = await client.resolveBuild(positionals[1]);
      const r = await client.runtimeLogs(b.id);
      if (flags.json) return json(r);
      if (!r.available) return console.log(dim(r.message || "runtime logs unavailable"));
      printLog(r.logs);
      return;
    }

    case "url": {
      if (!positionals[1]) die("usage: sprite-builder url <buildId> [--public|--private]");
      const b = await client.resolveBuild(positionals[1]);
      let v;
      if (flags.public || flags.private) {
        v = await client.setUrlVisibility(b.id, !!flags.public);
        console.error(dim(`# set ${b.id.slice(0, 8)} -> ${flags.public ? "public" : "private"}`));
      } else {
        v = await client.urlVisibility(b.id);
      }
      if (flags.json) return json(v);
      if (!v.available) return console.log(dim(v.message || "unavailable"));
      console.log(`${v.url || "(no url)"}  ${v.public ? c("32", "● public") : c("90", "● org-only")}`);
      return;
    }

    case "env": {
      if (!positionals[1]) die("usage: sprite-builder env <projectId>");
      const vars = await client.envVars(positionals[1]);
      if (flags.json) return json(vars);
      if (!vars.length) return console.log(dim("(no variables)"));
      for (const v of vars) console.log(`${bold(v.key)}=${v.value}`);
      return;
    }

    case "env-set": {
      const [, projectId, key, ...rest] = positionals;
      if (!projectId || !key || !rest.length)
        die("usage: sprite-builder env-set <projectId> <KEY> <VALUE>");
      const v = await client.setEnvVar(projectId, key, rest.join(" "));
      if (flags.json) return json(v);
      console.log(`${c("32", "set")} ${v.key}`);
      return;
    }

    case "env-rm": {
      const [, projectId, key] = positionals;
      if (!projectId || !key) die("usage: sprite-builder env-rm <projectId> <KEY>");
      await client.deleteEnvVar(projectId, key);
      console.log(`${c("32", "deleted")} ${key}`);
      return;
    }

    default:
      die(`unknown command "${cmd}" (try: sprite-builder help)`);
  }
}

main().catch((e) => {
  if (e instanceof ApiError) die(e.status ? `[${e.status}] ${e.message}` : e.message);
  die(e && e.stack ? e.stack : String(e));
});
