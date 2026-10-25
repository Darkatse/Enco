# Cairn API

Version 1 · Last updated September 2026

The Cairn API lets you read and change your own lists and tasks from scripts, home automations and other apps. Anything you can do to a task in the Cairn apps, you can do here.

- Base URL: `http://api.cairnlist.app/v1`
- Requests and responses are JSON in UTF-8. Send `Content-Type: application/json` with every request that has a body.
- Dates are calendar dates, `YYYY-MM-DD`, in the time zone set in your account (Settings → General). Tasks have due dates, not due times.

## Authentication

Every request needs a personal access token. Create one in **Settings → Integrations → Personal access tokens**. Tokens start with `cairn_pat_` and act as you, so treat them like a password; you can revoke one at any time on the same page.

Send the token in the `Authorization` header:

```sh
curl http://api.cairnlist.app/v1/lists \
  -H "Authorization: Bearer cairn_pat_..."
```

A missing, revoked or mistyped token gets `401 Unauthorized`:

```json
{ "error": "unauthorized", "message": "Missing or invalid access token" }
```

## Rate limits

Each token can make 300 requests per 15 minutes. Past that, requests get `429 Too Many Requests` with a `Retry-After` header giving the seconds to wait.

## Lists

Every account has an **Inbox**, plus the lists you create in the apps. The API can read lists; creating, renaming and deleting them is only possible in the apps for now.

### The list object

| Field | Type | Description |
|---|---|---|
| `id` | integer | |
| `name` | string | The name shown in the apps. |
| `inbox` | boolean | `true` for the Inbox, where new tasks go unless you choose another list. |

### GET /lists

Returns all of your lists.

```sh
curl http://api.cairnlist.app/v1/lists \
  -H "Authorization: Bearer cairn_pat_..."
```

```json
[
  { "id": 87, "name": "Inbox", "inbox": true },
  { "id": 88, "name": "Errands", "inbox": false }
]
```

## Tasks

### The task object

| Field | Type | Description |
|---|---|---|
| `id` | integer | Assigned by Cairn. |
| `title` | string | |
| `notes` | string | Free text; an empty string when there are none. |
| `list_id` | integer | The list the task is in. |
| `due_date` | string | `YYYY-MM-DD`; absent when the task has no due date. |
| `status` | string | `open` or `completed`. |

### GET /tasks

Returns every task that matches all of the given filters, oldest first. There is no pagination.

| Query parameter | Returns |
|---|---|
| `status` | Only tasks with this status: `open` or `completed`. |
| `list_id` | Only tasks in this list. |
| `due_date` | Only tasks due on this date, `YYYY-MM-DD`. |

Open tasks due on March 14, 2026:

```sh
curl "http://api.cairnlist.app/v1/tasks?status=open&due_date=2026-03-14" \
  -H "Authorization: Bearer cairn_pat_..."
```

```json
[
  { "id": 1203, "title": "Renew passport", "notes": "", "list_id": 88, "due_date": "2026-03-14", "status": "open" }
]
```

### POST /tasks

Creates a task. Returns `201 Created` with the new task.

| Field | Required | Description |
|---|---|---|
| `title` | yes | |
| `notes` | no | Defaults to an empty string. |
| `list_id` | no | Defaults to your Inbox. |
| `due_date` | no | `YYYY-MM-DD`; leave it out for a task with no due date. |

New tasks start out `open`.

```sh
curl -X POST http://api.cairnlist.app/v1/tasks \
  -H "Authorization: Bearer cairn_pat_..." \
  -H "Content-Type: application/json" \
  -d '{"title": "Book the dentist", "due_date": "2026-03-20"}'
```

```json
{ "id": 1204, "title": "Book the dentist", "notes": "", "list_id": 87, "due_date": "2026-03-20", "status": "open" }
```

### GET /tasks/{id}

Returns one task, or `404 Not Found` if you have no task with that id.

### PATCH /tasks/{id}

Changes any of `title`, `notes`, `list_id`, `due_date` and `status`; fields you leave out stay as they are. Returns the updated task.

To complete a task, set its status:

```sh
curl -X PATCH http://api.cairnlist.app/v1/tasks/1203 \
  -H "Authorization: Bearer cairn_pat_..." \
  -H "Content-Type: application/json" \
  -d '{"status": "completed"}'
```

To reopen it, set `status` back to `open`.

### DELETE /tasks/{id}

Deletes a task for good; tasks deleted through the API do not go to the trash. Returns `204 No Content`.

## Errors

| Status | When |
|---|---|
| 400 | The body is not a JSON object. |
| 401 | The token is missing, revoked or mistyped. |
| 404 | No task with that id, or a path the API does not have. |
| 429 | Too many requests; wait for `Retry-After` seconds. |

Questions: developers@cairnlist.app
