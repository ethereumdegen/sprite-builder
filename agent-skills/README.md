# agent-skills

Provider-agnostic **agent skills** for driving Sprite Builder. Each subfolder is
one skill: a `SKILL.md` with YAML frontmatter (`name`, `description`) followed by
a Markdown body that teaches an agent how to use part of the system.

These are plain Markdown — not tied to any one runtime. Any agent can use them:

- **Claude / Claude Code** — point a skill loader at this folder, symlink a skill
  into `~/.claude/skills/`, or paste a `SKILL.md` body into the system prompt.
- **OpenAI / other agents** — load `SKILL.md` as a tool/system instruction, or
  feed the body in as context. The frontmatter `description` is the trigger hint;
  the body is the how-to.

| Skill | What it teaches |
|-------|-----------------|
| [`sprite-builder/`](sprite-builder/SKILL.md) | Use the Sprite Builder HTTP API: create a project, trigger a build, poll it, get a live URL; manage env vars, codespaces, docuspaces. |

## Conventions

- One skill per folder; the entry file is always `SKILL.md`.
- Frontmatter requires `name` and `description`; keep `description` action-oriented
  so an agent knows *when* to reach for it.
- Skills read secrets (like the `sb_…` API key) from the **environment**, never
  hardcoded — see each skill for the variables it expects.
