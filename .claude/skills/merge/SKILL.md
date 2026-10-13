---
name: merge
description: Merge a Nubo pull request - wait for green CI and Codex's review, handle its findings, ask for a new review after later pushes, then merge. Use whenever the maintainer asks to merge a pull request.
---

# Merging pull requests

Codex reviews every pull request when it is opened. Its summary comment ("Codex Review Summary") shows the review as running and then as completed, and its findings arrive as review comments. When the maintainer asks Claude to merge a pull request, Claude waits until CI is green and Codex's review has completed. It then handles Codex's findings like any other review comments (fix and push, or answer why not, then resolve the thread) before it merges. A push after Codex's review doesn't start a new one, so when commits were pushed after its last review, Claude asks for a new review with a comment saying `@codex review` and waits for that one too. Codex doesn't always start: if a requested review hasn't started after about five minutes (its summary comment doesn't show it running and nothing else comes from Codex), Claude asks once more, and if that one hasn't started after another five minutes either, it merges without it. The maintainer's ChatGPT plan may change, and the Free plan may not include these reviews: when Codex stops starting reviews (both requests unanswered), Claude tells the maintainer, who decides whether to stop asking for Codex reviews or to pay for a plan that has them.
