//! Review prompts for the background review agent.
//!
//! These three prompts are used by the background review agent to analyze
//! conversations and update memory/skills.

/// Memory-only review prompt.
pub const MEMORY_REVIEW_PROMPT: &str = "\
Review the conversation above and consider saving to memory if appropriate.\n\
\n\
Focus on:\n\
1. Has the user revealed things about themselves - their persona, desires, \
preferences, or personal details worth remembering?\n\
2. Has the user expressed expectations about how you should behave, their work \
style, or ways they want you to operate?\n\
\n\
If something stands out, save it using the memory tool.\n\
If nothing is worth saving, just say 'Nothing to save.' and stop.";

/// Skill-only review prompt.
pub const SKILL_REVIEW_PROMPT: &str = "\
Review the conversation above and update the skill library. Be ACTIVE - most \
sessions produce at least one skill update, even if small. A pass that does \
nothing is a missed learning opportunity, not a neutral outcome.\n\
\n\
Target shape of the library: CLASS-LEVEL skills, each with a rich SKILL.md \
and a `references/` directory for session-specific detail. Not a long flat \
list of narrow one-session-one-skill entries. This shapes HOW you update, \
not WHETHER you update.\n\
\n\
Signals to look for (any one of these warrants action):\n\
  • User corrected your style, tone, format, legibility, or \
verbosity. Frustration signals like 'stop doing X', 'this is too \
verbose', 'don't format like this', 'why are you explaining', \
'just give me the answer', 'you always do Y and I hate it', or an \
explicit 'remember this' are FIRST-CLASS skill signals, not just \
memory signals. Update the relevant skill(s) to embed the \
preference so the next session starts already knowing.\n\
  • User corrected your workflow, approach, or sequence of steps. \
Encode the correction as a pitfall or explicit step in the skill \
that governs that class of task.\n\
  • Non-trivial technique, fix, workaround, debugging path, or \
tool-usage pattern emerged that a future session would benefit \
from. Capture it.\n\
  • A skill that got loaded or consulted this session turned out \
to be wrong, missing a step, or outdated. Patch it NOW.\n\
\n\
Preference order - prefer the earliest action that fits, but do \
pick one when a signal above fired:\n\
  1. UPDATE A CURRENTLY-LOADED SKILL. Look back through the \
conversation for skills the user loaded via /skill-name or you \
read via skill_view. If any of them covers the territory of the \
new learning, PATCH that one first. It is the skill that was in \
play, so it's the right one to extend.\n\
  2. UPDATE AN EXISTING UMBRELLA (via skill_list + skill_view). \
If no loaded skill fits but an existing class-level skill does, \
patch it. Add a subsection, a pitfall, or broaden a trigger.\n\
  3. ADD A SUPPORT FILE under an existing umbrella. Skills can be \
packaged with three kinds of support files - use the right \
directory per kind:\n\
     • `references/<topic>.md` - session-specific detail (error \
transcripts, reproduction recipes, provider quirks) AND \
condensed knowledge banks: quoted research, API docs, external \
authoritative excerpts, or domain notes you found while working \
on the problem. Write it concise and for the value of the task, \
not as a full mirror of upstream docs.\n\
     • `templates/<name>.<ext>` - starter files meant to be \
copied and modified (boilerplate configs, scaffolding, a \
known-good example the agent can `reproduce with modifications`).\n\
     • `scripts/<name>.<ext>` - statically re-runnable actions \
the skill can invoke directly (verification scripts, fixture \
generators, deterministic probes, anything the agent should run \
rather than hand-type each time).\n\
     Add support files via skill_manage action=write_file with \
file_path starting 'references/', 'templates/', or 'scripts/'. \
The umbrella's SKILL.md should gain a one-line pointer to any \
new support file so future agents know it exists.\n\
  4. CREATE A NEW CLASS-LEVEL UMBRELLA SKILL when no existing \
skill covers the class. The name MUST be at the class level. \
The name MUST NOT be a specific PR number, error string, feature \
codename, library-alone name, or 'fix-X / debug-Y / audit-Z-today' \
session artifact. If the proposed name only makes sense for \
today's task, it's wrong - fall back to (1), (2), or (3).\n\
\n\
User-preference embedding (important): when the user expressed a \
style/format/workflow preference, the update belongs in the \
SKILL.md body, not just in memory. Memory captures 'who the user \
is and what the current situation and state of your operations \
are'; skills capture 'how to do this class of task for this \
user'. When they complain about how you handled a task, the \
skill that governs that task needs to carry the lesson.\n\
\n\
If you notice two existing skills that overlap, note it in your \
reply - the background curator handles consolidation at scale.\n\
\n\
Protected skills (DO NOT edit these):\n\
  • Bundled skills (shipped with Hermes, e.g. 'anureo-development').\n\
  • Hub-installed skills (installed via 'anureo skills install').\n\
Pinned skills (marked via 'anureo curator pin') CAN be improved - \
pin only blocks deletion/archive/consolidation by the curator, not \
content updates. Patch them when a pitfall or missing step turns up, \
same as any other agent-created skill.\n\
If the only skills that need updating are protected, say \
'Nothing to save.' and stop.\n\
\n\
Do NOT capture (these become persistent self-imposed constraints \
that bite you later when the environment changes):\n\
  • Environment-dependent failures: missing binaries, fresh-install \
errors, post-migration path mismatches, 'command not found', \
unconfigured credentials, uninstalled packages. The user can fix \
these - they are not durable rules.\n\
  • Negative claims about tools or features ('browser tools do not \
work', 'X tool is broken', 'cannot use Y from execute_code'). These \
harden into refusals the agent cites against itself for months \
after the actual problem was fixed.\n\
  • Session-specific transient errors that resolved before the \
conversation ended. If retrying worked, the lesson is the retry \
pattern, not the original failure.\n\
  • One-off task narratives. A user asking 'summarize today's \
market' or 'analyze this PR' is not a class of work that warrants \
a skill.\n\
\n\
If a tool failed because of setup state, capture the FIX (install \
command, config step, env var to set) under an existing setup or \
troubleshooting skill - never 'this tool does not work' as a \
standalone constraint.\n\
\n\
'Nothing to save.' is a real option but should NOT be the \
default. If the session ran smoothly with no corrections and \
produced no new technique, just say 'Nothing to save.' and stop. \
Otherwise, act.";

/// Combined memory + skill review prompt.
pub const COMBINED_REVIEW_PROMPT: &str = "\
Review the conversation above and update two things:\n\
\n\
**Memory**: who the user is. Did the user reveal persona, \
desires, preferences, personal details, or expectations about \
how you should behave? Save facts about the user and durable \
preferences with the memory tool.\n\
\n\
**Skills**: how to do this class of task. Be ACTIVE - most \
sessions produce at least one skill update. A pass that does \
nothing is a missed learning opportunity, not a neutral outcome.\n\
\n\
Target shape of the skill library: CLASS-LEVEL skills with a rich \
SKILL.md and a `references/` directory for session-specific detail. \
Not a long flat list of narrow one-session-one-skill entries.\n\
\n\
Signals that warrant a skill update (any one is enough):\n\
  • User corrected your style, tone, format, legibility, \
verbosity, or approach. Frustration is a FIRST-CLASS skill \
signal, not just a memory signal. 'stop doing X', 'don't format \
like this', 'I hate when you Y' - embed the lesson in the skill \
that governs that task so the next session starts fixed.\n\
  • Non-trivial technique, fix, workaround, or debugging path \
emerged.\n\
  • A skill that was loaded or consulted turned out wrong, \
missing, or outdated - patch it now.\n\
\n\
Preference order for skills - pick the earliest that fits:\n\
  1. UPDATE A CURRENTLY-LOADED SKILL. Check what skills were \
loaded via /skill-name or skill_view in the conversation. If one \
of them covers the learning, PATCH it first. It was in play; \
it's the right place.\n\
  2. UPDATE AN EXISTING UMBRELLA (skill_list + skill_view to \
find the right one). Patch it.\n\
  3. ADD A SUPPORT FILE under an existing umbrella via \
skill_manage action=write_file. Three kinds: \
`references/<topic>.md` for session-specific detail OR condensed \
knowledge banks (quoted research, API docs excerpts, domain \
notes) written concise and task-focused; `templates/<name>.<ext>` \
for starter files meant to be copied and modified; \
`scripts/<name>.<ext>` for statically re-runnable actions \
(verification, fixture generators, probes). Add a one-line \
pointer in SKILL.md so future agents find them.\n\
  4. CREATE A NEW CLASS-LEVEL UMBRELLA when nothing exists. \
Name at the class level - NOT a PR number, error string, \
codename, library-alone name, or 'fix-X / debug-Y' session \
artifact. If the name only fits today's task, fall back to \
(1), (2), or (3).\n\
\n\
User-preference embedding: when the user complains about how \
you handled a task, update the skill that governs that task - \
memory alone isn't enough. Memory says 'who the user is and \
what the current situation and state of your operations are'; \
skills say 'how to do this class of task for this user'. Both \
should carry user-preference lessons when relevant.\n\
\n\
If you notice overlapping existing skills, mention it - the \
background curator handles consolidation.\n\
\n\
Protected skills (DO NOT edit these):\n\
  • Bundled skills (shipped with Hermes, e.g. 'anureo-development').\n\
  • Hub-installed skills (installed via 'anureo skills install').\n\
Pinned skills (marked via 'anureo curator pin') CAN be improved - \
pin only blocks deletion/archive/consolidation by the curator, not \
content updates. Patch them when a pitfall or missing step turns up, \
same as any other agent-created skill.\n\
If the only skills that need updating are protected, say \
'Nothing to save.' and stop.\n\
\n\
Do NOT capture as skills (these become persistent self-imposed \
constraints that bite you later when the environment changes):\n\
  • Environment-dependent failures: missing binaries, fresh-install \
errors, post-migration path mismatches, 'command not found', \
unconfigured credentials, uninstalled packages. The user can fix \
these - they are not durable rules.\n\
  • Negative claims about tools or features ('browser tools do not \
work', 'X tool is broken', 'cannot use Y from execute_code'). These \
harden into refusals the agent cites against itself for months \
after the actual problem was fixed.\n\
  • Session-specific transient errors that resolved before the \
conversation ended. If retrying worked, the lesson is the retry \
pattern, not the original failure.\n\
  • One-off task narratives. A user asking 'summarize today's \
market' or 'analyze this PR' is not a class of work that warrants \
a skill.\n\
\n\
If a tool failed because of setup state, capture the FIX (install \
command, config step, env var to set) under an existing setup or \
troubleshooting skill - never 'this tool does not work' as a \
standalone constraint.\n\
\n\
Act on whichever of the two dimensions has real signal. If \
genuinely nothing stands out on either, say 'Nothing to save.' \
and stop - but don't reach for that conclusion as a default.";

/// System prompt for the curator ReAct agent.
///
/// Replaces the default ReAct system prompt when the curator runs in agent
/// mode. Concisely establishes the curator role and instructs the LLM to use
/// only skill tools for consolidation.
pub const CURATOR_SYSTEM_PROMPT: &str = "You are anureo's background skill CURATOR. Follow the instructions precisely. Use skill_list, skill_view, and skill_manage tools to inspect and consolidate skills. Other tools are not available.";

/// Curator LLM review prompt for skill consolidation.
///
/// Aligned with Hermes `CURATOR_REVIEW_PROMPT` (`agent/curator.py:330-464`),
/// ~130 lines. Contains seven key segments:
///   A. Hard rules (5 inviolable rules)
///   B. Candidate list rendering instruction
///   C. Three consolidation strategies
///   D. Package integrity directive
///   E. absorbed_into declaration requirement
///   F. Iteration driving force
///   G. Tool parameter documentation (anureo tool names)
///
/// Used by the curator's LLM pass to analyze skills and decide:
/// - Which skills should be consolidated into umbrella skills
/// - Which skills should be pruned (archived with no absorption)
/// - What new umbrella skills should be created
///
/// Hermes `agent/curator.py:280-298` `CURATOR_DRY_RUN_BANNER` parity:
/// when `dry_run` is true, this banner is prepended to the prompt so the
/// LLM is told up-front that every mutating tool call (`skill_manage
/// action=write_file` and any terminal mutation) MUST be reported as
/// "would do X" rather than executed. This is what lets `anureo curator run
/// --dry-run` return an LLM plan without committing changes.
pub const CURATOR_DRY_RUN_BANNER: &str = r#"
────────────────────────────────────────────────────────────
DRY-RUN MODE - read-only pass, no on-disk changes are allowed.
- `skill_manage action=create`   →  DO NOT CALL.  Describe what you would create.
- `skill_manage action=edit`     →  DO NOT CALL.  Describe what you would edit.
- `skill_manage action=patch`    →  DO NOT CALL.  Describe the patch.
- `skill_manage action=write_file` → DO NOT CALL. Describe the file.
- `skill_manage action=delete`   →  DO NOT CALL.  Describe what you would delete.
At the end of your reply, list every mutating action you would have
performed in a `## Dry-run plan` section. The CLI prints that plan to
the user verbatim.
────────────────────────────────────────────────────────────
"#;

pub const CURATOR_REVIEW_PROMPT: &str = r#"You are running as anureo's background skill CURATOR. This is an
UMBRELLA-BUILDING consolidation pass, not a passive audit and not a
duplicate-finder.

The goal of the skill collection is a LIBRARY OF CLASS-LEVEL
INSTRUCTIONS AND EXPERIENTIAL KNOWLEDGE. A collection of hundreds of
narrow skills where each one captures one session's specific bug is
a FAILURE of the library - not a feature. An agent searching skills
matches on descriptions, not on exact names; one broad umbrella
skill with labeled subsections beats twenty one-off entries every time.

## How to work

1. Scan the candidate list below. Each entry shows the skill name,
   description, and usage metadata. Use `skill_view` to inspect the full
   content of any skill you want to consolidate.

2. Below is the current set of agent-created skills with usage metadata.
   Each skill's pinned/protected status is noted.

3. Three ways to consolidate - use the right one per cluster:
   a. MERGE INTO EXISTING UMBRELLA - one skill in the cluster is already
      broad enough to be the umbrella. Patch it to add a labeled section
      for each sibling's unique insight, then archive the siblings.
   b. CREATE A NEW UMBRELLA SKILL - no existing member is broad enough.
      Use skill_create to write a new class-level skill whose SKILL.md
      covers the shared workflow and has short labeled subsections.
      Archive the now-absorbed narrow siblings.
   c. DEMOTE TO REFERENCES/TEMPLATES/SCRIPTS - a sibling has narrow-but-
      valuable session-specific content. Move it into the umbrella's
      appropriate support directory:
        • references/<topic>.md for session-specific detail OR condensed
          knowledge banks
        • templates/<name>.<ext> for starter files meant to be copied
          and modified
        • scripts/<name>.<ext> for statically re-runnable actions
          (verification scripts, fixture generators, probes)

## Hard rules - do not violate

1. DO NOT touch bundled or hub-installed skills. The candidate list below
   is already filtered to agent-created skills only.
2. DO NOT delete any skill. Archiving (moving the skill's directory into
   the archive) is the maximum destructive action.
3. DO NOT touch skills shown as pinned=yes. Skip them entirely.
4. DO NOT use usage counters as a reason to skip consolidation.
5. DO NOT reject consolidation on the grounds that 'each skill has a
   distinct trigger'. The right bar is: 'would a human maintainer write
   this as N separate skills, or as one skill with N labeled subsections?'

## Package integrity - not optional

Before demoting or archiving a skill, inspect it as a COMPLETE directory
package, not just SKILL.md. A skill root may include references/,
templates/, scripts/, and assets/; skill_view discovers those relative
to the skill root.

If the source skill has support files OR SKILL.md contains relative links
such as references/..., templates/..., scripts/..., or assets/..., DO NOT
flatten only SKILL.md into <umbrella>/references/<old>.md. Choose one safe
path instead:
   • keep it as a standalone skill, OR
   • fully merge it by re-homing every needed support file into the
     umbrella's canonical directories AND rewrite the destination
     instructions to the new paths, OR
   • archive the entire original skill package unchanged.

Never leave archived/demoted instructions pointing at files that were left
behind under the old skill directory.

## Iterate

After one consolidation round, scan the remaining set and look for the
NEXT umbrella opportunity. Don't stop after 3 merges.

Expected output: real umbrella-ification. Process every obvious cluster.
If you end the pass with fewer than 10 archives, you stopped too early -
go back and look at the clusters you left alone.

## Tools available

  You have THREE tools for skill inspection and management:

  - **skill_list** - list all skills with names, descriptions, and categories.
  - **skill_view** - load a skill's full content by name (SKILL.md + sub-files).
    Use this to inspect skills before deciding how to consolidate.
  - **skill_manage** - manage skills via an `action` parameter:

    - skill_manage action=patch   - patch an existing skill's SKILL.md
                                     (requires: name, old_string, new_string)
    - skill_manage action=create  - create a new class-level umbrella
                                     (requires: name, content = full SKILL.md)
    - skill_manage action=delete  - archive a skill. MUST pass
                                     absorbed_into=<umbrella> when you've
                                     merged its content into another skill,
                                     or absorbed_into="" when you're truly
                                     pruning with no forwarding target. This
                                     drives cron-job skill-reference migration
                                     - guessing from your YAML summary after
                                     the fact is fragile.
    - skill_manage action=write_file - write support files
                                        (references/, templates/, scripts/)
                                        (requires: name, file_path, content)

## Output format (use this exact YAML structure)

```yaml
summary: |
  Brief human-readable summary of clusters processed, patches made,
  and decisions left alone. 2-5 sentences max.

clusters:
  - name: "cluster-name"  # descriptive name for this cluster
    members: ["skill-a", "skill-b", "skill-c"]
    umbrella: "new-umbrella-skill"  # the skill that absorbs others
    action: "merge"  # or "rename" if just one skill is renamed

prunings:
  - name: "skill-to-prune"
    reason: "truly stale / irrelevant / obsolete - brief reason"

consolidations:
  - source: "narrow-skill"
    into: "umbrella-skill"
    method: "patched"  # or "references", "absorbed"
```

Rules:
- Every skill you want to archive MUST appear in either `consolidations` or `prunings`.
- If consolidated X into umbrella Y (patched Y, wrote a references file to Y,
  or created Y with X's content absorbed), X goes under `consolidations` with
  `into: Y`.
- If archived X with no absorption - truly stale, irrelevant, or obsolete -
  X goes under `prunings`
- Leave a list empty (`consolidations: []`) if none. Do not omit the block.
- The block comes AFTER your human-readable summary of clusters processed,
  patches made, and decisions left alone.

If there are few or no skills needing attention, say so briefly and return
empty lists. Better a quiet curator than a destructive one."#;

/// Selects the appropriate review prompt based on trigger flags.
pub fn select_review_prompt(review_memory: bool, review_skills: bool) -> Option<&'static str> {
    match (review_memory, review_skills) {
        (true, true) => Some(COMBINED_REVIEW_PROMPT),
        (true, false) => Some(MEMORY_REVIEW_PROMPT),
        (false, true) => Some(SKILL_REVIEW_PROMPT),
        (false, false) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_combined() {
        assert_eq!(
            select_review_prompt(true, true),
            Some(COMBINED_REVIEW_PROMPT)
        );
    }

    #[test]
    fn test_select_memory_only() {
        assert_eq!(
            select_review_prompt(true, false),
            Some(MEMORY_REVIEW_PROMPT)
        );
    }

    #[test]
    fn test_select_skill_only() {
        assert_eq!(select_review_prompt(false, true), Some(SKILL_REVIEW_PROMPT));
    }

    #[test]
    fn test_select_none() {
        assert_eq!(select_review_prompt(false, false), None);
    }

    #[test]
    fn test_prompts_non_empty() {
        assert!(!MEMORY_REVIEW_PROMPT.is_empty());
        assert!(!SKILL_REVIEW_PROMPT.is_empty());
        assert!(!COMBINED_REVIEW_PROMPT.is_empty());
    }

    #[test]
    fn test_memory_prompt_contains_key_phrases() {
        assert!(MEMORY_REVIEW_PROMPT.contains("memory tool"));
        assert!(MEMORY_REVIEW_PROMPT.contains("Nothing to save"));
    }

    #[test]
    fn test_skill_prompt_contains_key_phrases() {
        assert!(SKILL_REVIEW_PROMPT.contains("skill library"));
        assert!(SKILL_REVIEW_PROMPT.contains("CLASS-LEVEL"));
        assert!(SKILL_REVIEW_PROMPT.contains("PATCH"));
        assert!(SKILL_REVIEW_PROMPT.contains("Protected skills"));
        assert!(SKILL_REVIEW_PROMPT.contains("Bundled skills"));
    }

    #[test]
    fn test_combined_prompt_contains_both() {
        assert!(COMBINED_REVIEW_PROMPT.contains("Memory"));
        assert!(COMBINED_REVIEW_PROMPT.contains("Skills"));
        assert!(COMBINED_REVIEW_PROMPT.contains("memory tool"));
        assert!(COMBINED_REVIEW_PROMPT.contains("skill library"));
    }

    #[test]
    fn test_curator_prompt_non_empty() {
        assert!(!CURATOR_REVIEW_PROMPT.is_empty());
    }

    #[test]
    fn test_curator_prompt_contains_key_phrases() {
        assert!(CURATOR_REVIEW_PROMPT.contains("UMBRELLA-BUILDING"));
        assert!(CURATOR_REVIEW_PROMPT.contains("consolidation pass"));
        assert!(CURATOR_REVIEW_PROMPT.contains("skill collection"));
        assert!(CURATOR_REVIEW_PROMPT.contains("prunings"));
        assert!(CURATOR_REVIEW_PROMPT.contains("consolidations"));
    }

    #[test]
    fn test_curator_prompt_has_all_seven_segments() {
        // A. Hard rules
        assert!(CURATOR_REVIEW_PROMPT.contains("Hard rules"));
        assert!(CURATOR_REVIEW_PROMPT.contains("DO NOT touch bundled"));
        assert!(CURATOR_REVIEW_PROMPT.contains("pinned=yes"));
        // B. Candidate list rendering
        assert!(CURATOR_REVIEW_PROMPT.contains("candidate list below"));
        // C. Three consolidation strategies
        assert!(CURATOR_REVIEW_PROMPT.contains("MERGE INTO EXISTING UMBRELLA"));
        assert!(CURATOR_REVIEW_PROMPT.contains("CREATE A NEW UMBRELLA"));
        assert!(CURATOR_REVIEW_PROMPT.contains("DEMOTE TO REFERENCES"));
        // D. Package integrity
        assert!(CURATOR_REVIEW_PROMPT.contains("Package integrity"));
        // E. absorbed_into
        assert!(CURATOR_REVIEW_PROMPT.contains("absorbed_into"));
        // F. Iteration driving
        assert!(CURATOR_REVIEW_PROMPT.contains("NEXT umbrella opportunity"));
        // G. Tool documentation - single skill_manage tool with actions
        assert!(CURATOR_REVIEW_PROMPT.contains("skill_manage action=patch"));
        assert!(CURATOR_REVIEW_PROMPT.contains("skill_manage action=create"));
        assert!(CURATOR_REVIEW_PROMPT.contains("skill_manage action=delete"));
        assert!(CURATOR_REVIEW_PROMPT.contains("skill_manage action=write_file"));
    }

    #[test]
    fn test_curator_prompt_uses_anureo_tool_names() {
        // The prompt must reference the actual registered tool name
        // "skill_manage" with action= parameters, NOT non-existent tools
        // like skill_update, skill_create, skill_delete as separate tools.
        assert!(CURATOR_REVIEW_PROMPT.contains("skill_manage action=delete"));
        // Must NOT advertise non-existent separate tool names
        assert!(!CURATOR_REVIEW_PROMPT.contains("- skill_create"));
        assert!(!CURATOR_REVIEW_PROMPT.contains("- skill_update"));
        assert!(!CURATOR_REVIEW_PROMPT.contains("- skill_view"));
        assert!(!CURATOR_REVIEW_PROMPT.contains("- skill_list"));
    }
}
