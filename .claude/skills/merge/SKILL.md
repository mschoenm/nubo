---
name: merge
description: Merge a Nubo pull request - wait for green CI and a branch without conflicts, handle any review comments, then squash-merge. Use whenever the maintainer asks to merge a pull request.
---

# Merging a pull request

Codex no longer reviews pull requests (it is no longer connected to this repository), so a merge waits for CI only, never for a review.

1. **Wait for CI.** Read the checks and comments of the pull request (GitHub MCP `pull_request_read`, or `gh pr checks` and `gh pr view --comments`). Only "CI passed" is required, and it is the only workflow pull requests run (the packages are built nightly). When CI is red or the branch conflicts with `main`, fix that first, or tell the maintainer what blocks it when the cause isn't this pull request's.
2. **Handle review comments** that are there, from the maintainer or anyone else: fix and push, or answer why not, then resolve the thread. Don't ask for a review or wait for one.
3. **Merge** with a squash merge, as every pull request so far (the title becomes the commit's subject, with the pull request's number), and delete the branch. With the GitHub MCP tools that is `merge_pull_request` with the squash method; with the GitHub CLI, `gh pr merge <number> --squash --delete-branch`.
