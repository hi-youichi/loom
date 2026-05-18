# task

Lightweight task manager CLI. Tasks are stored in a local SQLite database (`tasks.db`) within the working directory.

## Build

```bash
cd task && cargo build --release
```

Binary output: `target/release/task`

## Usage

### Global Options

| Option | Description | Default |
|---|---|---|
| `--work-folder <DIR>` | Working directory (where `tasks.db` lives) | Current directory |
| `--json` | Output in JSON format | `false` |
| `--no-color` | Disable colored output | `false` |

### Commands

#### `task create`

Create a new task.

```bash
task create --name "Build API" --description "Implement REST endpoints" --assignee "Alice" --start-time "2025-08-20T10:00:00" --status pending
```

| Option | Required | Default | Description |
|---|---|---|---|
| `--name` | Yes | — | Task name |
| `--description` | No | `""` | Task description |
| `--assignee` | No | `""` | Person responsible |
| `--start-time` | No | Now | Start time (see Time Formats) |
| `--status` | No | `pending` | `pending`, `in_progress`, `completed`, `cancelled` |

#### `task show <id>`

Show a single task by ID (full UUID or short ID prefix, minimum 4 chars).

```bash
task show a1b2c3d4
task --json show 550e8400-e29b-41d4-a716-446655440000
```

#### `task list`

List tasks with filtering, sorting, and pagination.

```bash
task list --status pending --assignee "Alice" --name "API" --sort-by created_at --sort-order desc --limit 20 --page 1
```

| Option | Default | Description |
|---|---|---|
| `--status` | All | Filter by status |
| `--assignee` | All | Filter by assignee (exact match) |
| `--name` | All | Filter by task name (fuzzy match) |
| `--sort-by` | `created_at` | Sort field: `created_at`, `start_time`, `name`, `status` |
| `--sort-order` | `desc` | Sort direction: `asc`, `desc` |
| `--limit` | `20` | Tasks per page |
| `--page` | `1` | Page number |
| `--no-header` | `false` | Hide table header (script-friendly) |

#### `task update <id>`

Update a task. Only provided fields are changed.

```bash
task update a1b2c3d4 --status in_progress --assignee "Bob"
task update a1b2c3d4 --description "Updated description"
```

All fields from `create` are optional. Passing `--assignee ""` clears the field; omitting `--assignee` leaves it unchanged.

#### `task delete <id>`

Delete a task. Prompts for confirmation unless `--force` is used.

```bash
task delete a1b2c3d4
task delete a1b2c3d4 --force
```

### Time Formats

`--start-time` accepts:

| Format | Example |
|---|---|
| ISO 8601 | `2025-08-20T10:00:00` |
| With space | `2025-08-20 10:00:00` |
| Without seconds | `2025-08-20 10:00` |
| Date only | `2025-08-20` (defaults to 00:00:00) |
| RFC 3339 with timezone | `2025-08-20T10:00:00+08:00` |

All inputs without timezone are treated as local time.

### Exit Codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Invalid arguments (handled by clap) |
| 2 | Business error (not found, ambiguous ID) |
| 3 | Database error |

### Output Examples

**Text mode (default)**:

```
ID         Name       Assignee  Status      Start Time
a1b2c3d4   Build API  Alice     pending     2025-08-20 10:00:00
e5f6g7h8   Fix bug    Bob       completed   2025-08-19 09:00:00

Showing 2 of 2 tasks (page: 1, limit: 20)
```

**JSON mode (`--json`)**:

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "Build API",
  "description": "Implement REST endpoints",
  "assignee": "Alice",
  "start_time": "2025-08-20T10:00:00+08:00",
  "created_at": "2025-08-19T15:30:00+08:00",
  "status": "pending"
}
```

### Storage

- Database file: `<work-folder>/tasks.db`
- SQLite with WAL journal mode
- Single `tasks` table with CHECK constraint on status

## Project Structure

```
task/
├── Cargo.toml
├── README.md
└── src/
    ├── main.rs     # Entry point + dispatch
    ├── args.rs     # Clap CLI definitions
    ├── db.rs       # SQLite operations (CRUD)
    ├── display.rs  # Text/JSON formatting
    └── models.rs   # Task struct + TaskStatus enum
```
