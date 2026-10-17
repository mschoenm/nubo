---
name: issue-triage
description: Set up Nubo's issue-triage page again - the private claude.ai page that lists every open issue with an assessment, the standing triage session and the 06:50 routine - when the page, the session or the routine is gone or broken. Use when asked to recreate, restore, repair or move the issue-triage page or its routine.
---

# Setting up the issue triage again

`docs/ISSUE_TRIAGE.md` says what the issue triage does and is the triage session's instructions; its "Current setup" section names the page, the session and the routine in use. Three parts work together, and each can be made again on its own:

- **The page:** a private claude.ai artifact made from `page.html` in this folder. Its database holds the assessments, the work on each issue and the maintainer's requests.
- **The triage session:** a Claude cloud session on this repository that brings the page up to date and starts a session for each ticked issue. The page sends its messages to the session named in its database's `meta/page`.
- **The routine:** wakes the triage session every morning at 06:50 Amsterdam time.

First find out what is gone: open the page's URL (the `Artifact` tool's `read`), look the session up (`get_session` of the claude-code-remote MCP server) and list the routines (`list_triggers`). Make again only what is missing, in this order.

1. **The page,** only when the artifact is gone. Publish `page.html` with the `Artifact` tool, `icon` `checklist`, the description "De open issues van Nubo, elke ochtend om 06:50 bijgewerkt; vink aan wat Claude in aparte sessies moet oplossen." and these capabilities:

   ```json
   {"db": {}, "mcp": {"servers": [{"server": "Claude Code Remote", "tools": ["send_message"]}]}}
   ```

   Pin it to the maintainer's sidebar (`pin: true`). Its database starts empty: the triage session's first run assesses every open issue.
2. **The triage session,** when it is gone or the page is new. Create it with `create_session`: `source_url` https://github.com/mschoenm/nubo, title `Nubo issue-triage`, tag `nubo-issue-triage`, and this prompt, with the page's URL filled in:

   > You are the standing issue-triage session of the Nubo repository. Your instructions are `docs/ISSUE_TRIAGE.md` on `origin/main`; the page is <page URL>. Read them now and do part A: it writes this session's id to the page's `meta/page`, which is where the page sends its messages.

3. **The routine,** when it is gone or the session is new. Create it with `create_trigger`: name `Nubo issue-triage bijwerken`, `cron_expression` `CRON_TZ=Europe/Amsterdam 50 6 * * *`, `persistent_session_id` the triage session, `initiation` `human_request`, and this prompt, with the page's URL filled in:

   > Daily issue-triage refresh. Fetch `origin/main` and follow `docs/ISSUE_TRIAGE.md`: part A, then part B for every request still `nieuw`. The page is <page URL>.

   Delete the old routine if it still fires into a session that is gone.
4. **The docs:** update "Current setup" in `docs/ISSUE_TRIAGE.md` in a pull request, so the next setup starts from the right names.

## Changing the page

`page.html` is the page's whole source. Publish a change with the `Artifact` tool to the page's URL (`url`; read the artifact first, as the tool asks), without `capabilities` so the stored ones carry over, and commit the same file here. The page reads its list from the database; a change to the database's fields goes into `docs/ISSUE_TRIAGE.md` in the same pull request.
