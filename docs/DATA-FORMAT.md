# Data format

All of BanKan's data lives under `~/.bankan/data/` (or `$BANKAN_HOME/data/`) as plain files. This is the folder that git backup commits.

```
data/
  .gitignore                    ignores temp files from atomic writes
  projects/
    <KEY>/
      project.yaml
      tickets/<KEY>-<n>.md
      proposals/<KEY>-P<n>.md
      docs/<path>.md            nested folders allowed
```

Every write goes to a temp file first (`.tmp-<name>`), which is then renamed into place, so a crash never leaves a half-written file behind.

## project.yaml

```yaml
key: RES                      # 2–10 chars, A–Z/0–9, starts with a letter; never changes
name: Resure
description: ''
color: '#5e6ad2'              # optional
archived: false
statuses:                     # board columns, in order
- name: Backlog
  category: backlog           # backlog | unstarted | started | completed | canceled
  color: '#8b8d98'
- name: Todo
  category: unstarted
- name: In Progress
  category: started
- name: Done
  category: completed
- name: Canceled
  category: canceled
types:                        # the first one is the default
- name: Feature
  color: '#6e8efb'
- name: Bug
  color: '#e5534b'
labels:
- name: api
  color: '#6e8efb'
milestones:
- name: v0.1
  description: ''
  due: 2026-11-01             # optional
  done: false
fields:                       # custom ticket fields
- name: Severity
  kind: select                # text | number | select | date | url
  options: [S1, S2, S3]
next_ticket: 13               # counters; ids are never reused
next_proposal: 4
created: 2026-10-22T14:11:13Z
updated: 2026-10-22T14:11:13Z
```

## Tickets: `tickets/RES-12.md`

The YAML front matter holds the fields. The Markdown below it is the description.

```markdown
---
id: RES-12
title: Add cursor pagination to list_tickets
status: In Review            # must match a status name in project.yaml
type: Feature
priority: medium             # none | urgent | high | medium | low
labels: [mcp, api]
assignee: claude
estimate: 3.0
due: 2026-10-30
milestone: v0.1
parent: RES-3                # makes this a sub-ticket
blocks: [RES-14]             # "blocked by" is worked out from these, not stored
related: [RES-9]
duplicate_of: RES-2
proposal: RES-P3             # the proposal this implements
branch: res-12-pagination
links:
- url: https://github.com/me/repo/pull/4
  title: 'PR #4'
fields:
  Severity: S2
order: 1500.0                # position within its column (lower = higher up)
created: 2026-10-22T14:11:13Z
updated: 2026-10-22T15:02:40Z
closed: 2026-10-23T09:00:00Z # set when it enters a completed or canceled status
comments:
- id: 1
  author: claude
  at: 2026-10-22T15:02:40Z
  body: Opened PR #4.
---

## Problem

The list returns every ticket…
```

Only `id`, `title`, `status`, `type`, `created` and `updated` are required. Empty fields are left out.

## Proposals: `proposals/RES-P3.md`

```markdown
---
id: RES-P3
title: Move auth to OAuth device flow
status: review               # draft | review | accepted | rejected | withdrawn | superseded | implemented
authors: [me, claude]
labels: [auth]
tickets: [RES-20, RES-21]    # tickets implementing it (kept in sync with ticket.proposal)
supersedes: RES-P1
superseded_by: null
decided: 2026-10-25          # set automatically on accept/reject
created: 2026-10-22T14:11:13Z
updated: 2026-10-22T14:11:13Z
---

## Summary
…
```

When a proposal that `supersedes` another is accepted, the older proposal is marked `superseded` and gets `superseded_by` set.

## Docs: `docs/guides/setup.md`

```markdown
---
title: Getting started
tags: [onboarding]
created: 2026-10-22T14:11:13Z
updated: 2026-10-22T14:11:13Z
---

# Getting started
…
```

A doc's path (`guides/setup`) comes from where its file sits, so it isn't stored in the file.

## Editing by hand

You can edit these files, but the server keeps everything in memory, so stop it first or restart it afterwards (see the README). Files that fail to parse are skipped with a warning in `~/.bankan/bankan.log`. They are never deleted. A project folder's name must match the `key` in its `project.yaml`, and ticket or proposal files must belong to that project. Mismatches are skipped with a warning too. Doc paths are unique ignoring case, because macOS disks treat `Guides/Setup.md` and `guides/setup.md` as the same file.
