# sprite-builder-cli

A small, dependency-free command-line client for the **sprite-builder** deploy service.

## Install

```bash
# from this directory
npm install -g .
# or run without installing
node bin/cli.js <command>
```

This provides two binaries: `sprite-builder` and the short alias `sb`.

## Auth & config

Set your API token (create one in the sprite-builder UI → API Keys):

```bash
export SPRITE_BUILDER_TOKEN=sb_xxxxxxxxxxxxxxxx
# optional — defaults to the production instance
export SPRITE_BUILDER_URL=https://sprite-builder-production.up.railway.app
```

Or pass `--token` / `--base-url` per command.

## Commands

```
sb me                                   # who am I
sb projects                             # list projects
sb builds                               # all builds, newest first
sb builds --status running              # only running builds
sb builds --project <id> --limit 10
sb logs <buildId> [--tail 50]           # deploy/build log
sb runtime <buildId>                    # live docker logs of the deployment
sb url <buildId>                        # show URL + public/org-only
sb url <buildId> --public               # make the deployment public
sb url <buildId> --private              # restrict to org members
sb env <projectId>                      # list env vars
sb env-set <projectId> KEY value        # set/replace an env var
sb env-rm <projectId> KEY               # delete an env var
```

- Build ids accept a **unique prefix** (e.g. `sb logs fbaab`).
- Add `--json` to any command for raw JSON (good for scripting/piping to `jq`).
- `NO_COLOR=1` disables ANSI colors.

## Examples

```bash
# Are any builds running right now?
sb builds --status running

# Tail the deploy log of the latest build, then its runtime log
sb logs $(sb builds --limit 1 --json | jq -r '.[0].id') --tail 40
sb runtime <buildId>

# Flip a deployment private, then public again
sb url <buildId> --private
sb url <buildId> --public
```
