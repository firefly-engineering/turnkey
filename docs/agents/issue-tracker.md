# Issue tracker: beadwork (`bw`)

Issues and specs for this repo live in [beadwork](https://github.com/jallum/beadwork), stored on the `beadwork` git branch. IDs look like `turnkey-XYZ` (children of an epic: `turnkey-XYZ.N`). Use the `bw` CLI for all operations; add `--json` when you need to parse output.

GitHub Issues on `firefly-engineering/turnkey` are **not** the tracker — don't create issues there.

## Conventions

- **Create an issue**: `bw create "<title>" -t <type> -p <0-4> -d "<description>"`. Types: `task`, `bug`, `feature`, `epic`, `question`, `docs`. Priority defaults to P2. Add `--silent` to print only the new ID.
- **Read an issue**: `bw show <id>` (includes dependencies and comments); `bw history <id>` for the change log.
- **List issues**: `bw list` (open + in-progress, limit 10). Filter with `--status`, `--type`, `--label`, `--parent`, `--grep`; use `--all` to drop the status/limit filter.
- **Comment on an issue**: `bw comment <id> "<text>"`
- **Apply / remove labels**: `bw label <id> +<label> -<label>`
- **Start work**: `bw start <id>` (sets in_progress, assigns to the git user, refuses blocked issues)
- **Close**: `bw close <id> --reason "<why>"`
- **Defer**: `bw defer <id> <when>` — hides it from `bw ready` until the date.
- **Sync**: `bw sync` after writes, so the `beadwork` branch is pushed.

## Pull requests as a triage surface

**PRs as a request surface: no.** _(Set to `yes` if this repo treats external PRs as feature requests; `/triage` reads this flag. PRs live on GitHub — use `gh pr view/list/comment` to read them, and track any resulting work as a `bw` issue.)_

## When a skill says "publish to the issue tracker"

Create a beadwork issue with `bw create`, then `bw sync`.

## When a skill says "fetch the relevant ticket"

Run `bw show <id>`.

## Wayfinding operations

Used by `/wayfinder`. The **map** is a beadwork epic with **child** issues as tickets.

- **Map**: `bw create "<title>" -t epic -d "<Notes / Decisions-so-far / Fog>"`, then `bw label <map> +wayfinder:map`, then `bw start <map>`. `bw ready` only lists the ready children of an **in-progress** epic, so an open map hides its frontier.
- **Child ticket**: `bw create "<title>" --parent <map> -d "..."`, labelled `wayfinder:<type>` (`research`/`prototype`/`grilling`/`task`) via `bw label`.
- **Blocking**: `bw dep add <blocker> blocks <blocked>`. A ticket is unblocked when every blocker is closed.
- **Frontier query**: `bw ready` lists unblocked open issues; keep those that are children of the map (cross-check with `bw list --parent <map>`) and have no assignee. First in map order wins.
- **Claim**: `bw start <id>` — the session's first write.
- **Resolve**: `bw comment <id> "<answer>"`, then `bw close <id> --reason "..."`, then append a context pointer (gist + ID) to the map's Decisions-so-far with `bw update <map> -d "..."`. Finish with `bw sync`.
