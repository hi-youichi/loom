# Instance Model Refactor (workflow tool)

This changelog tracks the refactor described in
docs/design/workflow-instance-model.md. It is a living checklist -
tick boxes as each task ships to main.

## breaking changes
  - workflow tool action run renamed execute (legacy alias kept
    for one minor, returns a deprecation field).
  - list-runs renamed list-instances.
  - run-status renamed instance-summary and now returns the
    curated InstanceMeta payload instead of the raw
    checkpoint+events dump.
  - New actions instance-events and instance-source added.
  - run_dir parameter renamed instance_dir.
  - Workflow storage path moves from .luft/{workflows,runs}/ to
    .loom/{workflows,instances}/. Legacy .luft/runs/ entries
    remain readable via list-instances with source:legacy
    until the next minor.

## task tracking
  - [x] T-01 instance.rs clean-layer module
  - [x] T-02 paths + action rename
  - [x] T-03 execute wiring writes instance.json
  - [x] T-04 list-instances pagination + status filter + legacy tag
  - [x] T-05 instance-summary handler
  - [x] T-06 instance-events handler
  - [x] T-07 instance-source handler
  - [x] T-08a skill markdown rewrite
  - [x] T-08b skill wiring in tool.rs
  - [x] T-09a docs + changelog (this entry)
  - [x] T-09b CLI audit + final acceptance

## migration for users
  - Move .luft/workflows/*.lua files to .loom/workflows/. The
    resolver only looks under .loom/workflows/ from now on.
  - Past runs under .luft/runs/ are auto-discovered by
    list-instances for one minor release; copy them to
    .loom/instances/ and rename luft-workflow_<ts> ->
    loom-instance_<ts> if you want them treated as current.
