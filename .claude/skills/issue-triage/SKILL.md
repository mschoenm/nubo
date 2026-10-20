---
name: issue-triage
description: Work on Nubo's issue-triage page with the maintainer, or set it up again - the private claude.ai page that lists every open issue with an assessment, where the maintainer starts issues to be solved in sessions of their own, kept up to date by an hourly routine. Use when asked to work on, open or update the issue triage, or to recreate, restore, repair or move the page, its session or its routine.
---

# The issue triage

`docs/ISSUE_TRIAGE.md` says what the issue triage does and is the instructions of every session that works on it; its "Current setup" section names the page, and the page's `meta/page` document holds the ids of the triage session and routine in use (`session_id`, `trigger_id`), which change every week.

**To work on the triage with the maintainer** (they ask to work on it, open it or update it), follow part C of `docs/ISSUE_TRIAGE.md`.

## Setting it up again

Three parts work together, and each can be made again on its own:

- **The page:** a private claude.ai artifact made from `page.html` in this folder. Its database holds the assessments, the work on each issue and the maintainer's requests.
- **The triage session:** a Claude cloud session on this repository that the routine wakes every hour to bring the page up to date and start a session for each issue the maintainer starts there. Once a week it hands over to a successor (part A, step 8 of `docs/ISSUE_TRIAGE.md`).
- **The routine:** wakes the triage session every hour. Each triage session makes its own, bound to itself.

First find out what is gone: open the page's URL (the `Artifact` tool's `read`), read `meta/page` from its database (`ArtifactData`), look its `session_id` up (`get_session` of the claude-code-remote MCP server) and list the routines (`list_triggers`). Make again only what is missing, in this order. A routine whose session (`persistent_session_id`) isn't `meta/page` `session_id` means the triage session was archived and the routine started one in its place: that session takes over by itself (part A of `docs/ISSUE_TRIAGE.md`), unless its routine's prompt is older than the one step 8 gives and doesn't attach the repository; then treat the triage session as gone (step 2).

1. **The page,** only when the artifact is gone. Publish `page.html` with the `Artifact` tool, `icon` `checklist`, the description "De open issues van Nubo, elk uur bijgewerkt; druk op Start bij wat Claude in een aparte sessie moet oplossen." and the capabilities `{"db": {}}`. Pin it to the maintainer's sidebar (`pin: true`). Its database starts empty: the triage session's first run assesses every open issue.
2. **The triage session and its routine,** when the session is gone. Create it with `create_session` exactly as the weekly handover does (part A, step 8 of `docs/ISSUE_TRIAGE.md`: the same title, tag and prompt). In its first turn it makes its own routine, turns the old one off (a daily run deletes it later), writes `meta/page` `session_id`, `trigger_id` and `session_started_at`, and archives the old session if it still exists.
3. **The routine alone,** when the session is there but its routine is gone. Create it with `create_trigger` as step 8 describes, but with `persistent_session_id` the triage session, and write its id to `meta/page` `trigger_id`.

   Then delete every other routine named `Nubo issue-triage bijwerken` (`list_triggers`, then `delete_trigger`), so only one fires each hour, none updates a page that is gone, and they don't pile up in the maintainer's list. Deleting a routine also deletes the sessions it started, which for a routine bound to a session is only one it started in place of an archived triage session.
4. **The docs:** when the page's URL changed, update "Current setup" in `docs/ISSUE_TRIAGE.md` in a pull request. The session and routine ids live only in `meta/page`.

## Changing the page

`page.html` is the page's whole source. Publish a change with the `Artifact` tool to the page's URL (`url`; read the artifact first, as the tool asks), without `capabilities` so the stored ones carry over, and commit the same file here. The page reads its list from the database; a change to the database's fields goes into `docs/ISSUE_TRIAGE.md` in the same pull request.
