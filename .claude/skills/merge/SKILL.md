---
name: merge
description: Merge a Nubo pull request - wait for green CI and Codex's review, handle its findings, ask for a new review after later pushes, then squash-merge. Use whenever the maintainer asks to merge a pull request.
---

# Merging a pull request

Codex reviews every pull request when it is opened. Its summary comment ("Codex Review Summary") shows the review as running and then as completed, and its findings arrive as review comments.

1. **Wait for CI and Codex.** Only "CI passed" is required; `platforms.yml` (macOS and Windows) never holds up a merge, but look at its result. When CI is red or the branch conflicts with `main`, fix that first, or tell the maintainer what blocks it when the cause isn't this pull request's.
2. **Handle Codex's findings** like any other review comments: fix and push, or answer why not, then resolve the thread.
3. **Ask for a new review after later pushes.** A push after Codex's review doesn't start a new one, so when commits were pushed after its last review, comment `@codex review` and wait for that one too.
4. **When Codex doesn't start:** if a requested review hasn't started after about five minutes (its summary comment doesn't show it running and nothing else comes from Codex), ask once more. If that one hasn't started after another five minutes either, merge without it. The maintainer's ChatGPT plan may change, and the Free plan may not include these reviews: when Codex stops starting reviews (both requests unanswered), tell the maintainer, who decides whether to stop asking for Codex reviews or to pay for a plan that has them.
5. **Merge** with a squash merge, as every pull request so far (the title becomes the commit's subject, with the pull request's number), and delete the branch. With the GitHub MCP tools that is `merge_pull_request` with the squash method; with the GitHub CLI, `gh pr merge <number> --squash --delete-branch`.
