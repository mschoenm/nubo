---
name: issue-triage
description: Work on Nubo's issue-triage page with the maintainer, or set it up again - the private claude.ai page that lists every open issue with an assessment, where the maintainer ticks issues to be solved in sessions of their own, kept up to date by an hourly routine. Use when asked to work on, open or update the issue triage, or to recreate, restore, repair or move the page, its session or its routine.
---

# The issue triage

`docs/ISSUE_TRIAGE.md` says what the issue triage does and is the instructions of every session that works on it; its "Current setup" section names the page, the session and the routine in use.

**To work on the triage with the maintainer** (they ask to work on it, open it or update it), follow part C of `docs/ISSUE_TRIAGE.md`.

## Setting it up again

Three parts work together, and each can be made again on its own:

- **The page:** a private claude.ai artifact made from `page.html` in this folder. Its database holds the assessments, the work on each issue and the maintainer's requests.
- **The triage session:** a Claude cloud session on this repository that the routine wakes every hour to bring the page up to date and start a session for each ticked issue.
- **The routine:** wakes the triage session every hour.

First find out what is gone: open the page's URL (the `Artifact` tool's `read`), look the session up (`get_session` of the claude-code-remote MCP server) and list the routines (`list_triggers`). Make again only what is missing, in this order.

1. **The page,** only when the artifact is gone. Publish `page.html` with the `Artifact` tool, `icon` `checklist`, the description "De open issues van Nubo, elk uur bijgewerkt; vink aan wat Claude in aparte sessies moet oplossen." and the capabilities `{"db": {}}`. Pin it to the maintainer's sidebar (`pin: true`). Its database starts empty: the triage session's first run assesses every open issue.
2. **The triage session,** when it is gone. Create it with `create_session`: `source_url` https://github.com/mschoenm/nubo, title `Nubo issue-triage`, tag `nubo-issue-triage`, and this prompt, with the page's URL filled in:

   > You are the standing issue-triage session of the Nubo repository. Your instructions are `docs/ISSUE_TRIAGE.md` on `origin/main`; the page is <page URL>. Read them now and do part A.

3. **The routine,** when it is gone or the session is new. Create it with `create_trigger`: name `Nubo issue-triage bijwerken`, `cron_expression` `CRON_TZ=Europe/Amsterdam 0 * * * *` (every hour), `persistent_session_id` the triage session, `initiation` `human_request`, and this prompt, with the page's URL filled in:

   > Issue-triage refresh (every hour). Fetch `origin/main` and follow `docs/ISSUE_TRIAGE.md`, part A. The page is <page URL>.

   Once it exists, delete every other routine named `Nubo issue-triage bijwerken` (`list_triggers`, then `delete_trigger`), whether or not its session is still there, so only one fires each hour and none updates a page that is gone. When step 2 made a new session, archive the old one if it still exists (`archive_session`).
4. **The docs:** update "Current setup" in `docs/ISSUE_TRIAGE.md` in a pull request, so the next setup starts from the right names.

## Changing the page

`page.html` is the page's whole source. Publish a change with the `Artifact` tool to the page's URL (`url`; read the artifact first, as the tool asks), without `capabilities` so the stored ones carry over, and commit the same file here. The page reads its list from the database; a change to the database's fields goes into `docs/ISSUE_TRIAGE.md` in the same pull request.
