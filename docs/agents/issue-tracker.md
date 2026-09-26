# Issue tracker: GitHub Issues

Issues live in [GitHub Issues on `firefly-engineering/turnkey`](https://github.com/firefly-engineering/turnkey/issues), and priority and status live in the [**turnkey** org project](https://github.com/orgs/firefly-engineering/projects/3). Use the `gh` CLI; add `--json` when you need to parse output.

Before 2026-09-26 the tracker was [beadwork](https://github.com/jallum/beadwork), stored on the `beadwork` git branch; that branch is kept as a read-only archive. Open issues, and the closed ones they referenced, were migrated; each migrated issue's footer names its old `turnkey-XYZ` ID, and [#78](https://github.com/firefly-engineering/turnkey/issues/78) holds the full mapping. To find what an old commit's `(turnkey-XYZ)` refers to, search for the ID: `gh issue list --search '"turnkey-XYZ" in:body' --state all`. Unmigrated IDs are only in the archive (`git show beadwork:...`, or `bw show <id>` if you still have `bw`).

## Data model

| Concept | Where it lives |
|---|---|
| Kind | Issue **type** `Bug`, `Feature` or `Task` (org-level). Epics, questions and docs are `Task` plus the label `epic`, `question` or `documentation` |
| Priority | Project field **Priority**: `P0` critical … `P4` backlog. Default `P2` |
| Status | Open/closed on the issue; in-progress is project field **Status** = `In Progress` plus an assignee |
| Epic → child | GitHub **sub-issues** |
| Blocker | GitHub **issue dependencies** ("blocked by") |

## Conventions

The snippets assume `R=firefly-engineering/turnkey`.

- **Create an issue**:
  ```bash
  gh issue create -R $R --title "<title>" --body "<description>" --label <label> --project turnkey
  gh api -X PATCH repos/$R/issues/<n> -f type=Bug     # Bug | Feature | Task
  ```
  Then set its priority (below). `gh issue create` has no flag for the type.
- **Read an issue**: `gh issue view <n> --comments`. Sub-issues: `gh api repos/$R/issues/<n>/sub_issues --jq '.[]|"#\(.number) \(.state) \(.title)"'`. Blockers: `gh api repos/$R/issues/<n>/dependencies/blocked_by --jq '.[]|"#\(.number) \(.state) \(.title)"'`.
- **List issues**: `gh issue list` (open). Filter with `--label`, `--search`, `--assignee`, `--state all`.
- **Ready work** (open, unassigned, not blocked by an open issue): `gh issue list --search "-is:blocked no:assignee"`. `is:blocked`, `is:blocking` and `parent-issue:` only work in GitHub's advanced issue search, which `gh issue list` uses; the REST `search/issues` endpoint silently ignores them unless you pass `-f advanced_search=true`. By priority: `gh project item-list 3 --owner firefly-engineering --format json --limit 500 --jq '.items[]|select(.status=="Todo")|"\(.priority) #\(.content.number) \(.title)"' | sort`.
- **Comment**: `gh issue comment <n> --body "<text>"`
- **Labels**: `gh issue edit <n> --add-label <l> --remove-label <l>`
- **Start work**: check it has no open blockers, then `gh issue edit <n> --add-assignee @me` and set Status to `In Progress`.
- **Close**: commit with `Fixes #<n>` so the merge closes it. To close without a commit: `gh issue close <n> --comment "<why>"` (add `--reason "not planned"` for wontfix).
- **Sub-issue**: `gh api -X POST repos/$R/issues/<parent>/sub_issues -F sub_issue_id=$(gh api repos/$R/issues/<child> --jq .id)`
- **Blocker**: `gh api -X POST repos/$R/issues/<blocked>/dependencies/blocked_by -F issue_id=$(gh api repos/$R/issues/<blocker> --jq .id)`. Both endpoints take the issue's numeric `id`, not its `#number`.

### Setting a project field (Priority or Status)

```bash
set_field() {  # set_field <issue-number> <Priority|Status> <option, e.g. P1 or "In Progress">
  local p=3 o=firefly-engineering
  local proj=$(gh project view $p --owner $o --format json --jq .id)
  local item=$(gh project item-list $p --owner $o --format json --limit 500 \
    --jq ".items[]|select(.content.number==$1)|.id")
  local field=$(gh project field-list $p --owner $o --format json --jq ".fields[]|select(.name==\"$2\")")
  gh project item-edit --project-id "$proj" --id "$item" \
    --field-id "$(jq -r .id <<<"$field")" \
    --single-select-option-id "$(jq -r --arg v "$3" '.options[]|select(.name==$v).id' <<<"$field")"
}
```

## Pull requests as a triage surface

**PRs as a request surface: no.** _(Set to `yes` if this repo treats external PRs as feature requests; `/triage` reads this flag. Use `gh pr view/list/comment` to read them, and track any resulting work as an issue.)_

## When a skill says "publish to the issue tracker"

Create a GitHub issue as above, with its type and priority.

## When a skill says "fetch the relevant ticket"

Run `gh issue view <n> --comments`.

## Wayfinding operations

Used by `/wayfinder`. The **map** is an issue labelled `epic` and `wayfinder:map`, with **sub-issues** as tickets.

- **Map**: create it with body `Notes / Decisions-so-far / Fog`, labels `epic,wayfinder:map`, then set its Status to `In Progress`.
- **Child ticket**: create it labelled `wayfinder:<type>` (`research`/`prototype`/`grilling`/`task`), then add it as a sub-issue of the map.
- **Blocking**: add a "blocked by" dependency. A ticket is unblocked when every blocker is closed.
- **Frontier query**: the map's open sub-issues that are unassigned and not blocked: `gh issue list --search "-is:blocked no:assignee parent-issue:firefly-engineering/turnkey#<map>"`. Lowest sub-issue order wins.
- **Claim**: assign yourself — the session's first write.
- **Resolve**: `gh issue comment <n>` with the answer, `gh issue close <n>`, then append a context pointer (gist + `#n`) to the map's Decisions-so-far with `gh issue edit <map> --body-file -`.
