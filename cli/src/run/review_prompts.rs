//! Review prompts copied from Hermes `run_agent.py`.
//!
//! These three prompts are used by the background review agent to analyze
//! conversations and update memory/skills. They are the exact text from
//! Hermes source code.

/// Memory-only review prompt.
pub const MEMORY_REVIEW_PROMPT: &str = "\
Review the conversation above and consider saving to memory if appropriate.\n\
\n\
Focus on:\n\
1. Has the user revealed things about themselves — their persona, desires, \
preferences, or personal details worth remembering?\n\
2. Has the user expressed expectations about how you should behave, their work \
style, or ways they want you to operate?\n\
\n\
If something stands out, save it using the memory tool.\n\
If nothing is worth saving, just say 'Nothing to save.' and stop.";

/// Skill-only review prompt.
pub const SKILL_REVIEW_PROMPT: &str = "\
Review the conversation above and update the skill library. Be ACTIVE — most \
sessions produce at least one skill update, even if small. A pass that does \
nothing is a missed learning opportunity, not a neutral outcome.\n\
\n\
Target shape of the library: CLASS-LEVEL skills, each with a rich SKILL.md \
and a `references/` directory for session-specific detail. Not a long flat \
list of narrow one-session-one-skill entries.\n\
\n\
Signals that warrant a skill update (any one is enough):\n\
  • User corrected your style, tone, format, legibility, verbosity, or \
approach. Frustration is a FIRST-CLASS skill signal, not just a memory \
signal. 'stop doing X', 'don't format like this', 'I hate when you Y' \
— embed the lesson in the skill that governs that task so the next \
session starts fixed.\n\
  • Non-trivial technique, fix, workaround, or debugging path emerged.\n\
  • A skill that was loaded or consulted turned out wrong, missing, or \
outdated — patch it now.\n\
\n\
Preference order for skills — pick the earliest that fits:\n\
  1. UPDATE A CURRENTLY-LOADED SKILL. Check what skills were loaded via \
/skill-name or skill_view in the conversation. If one of them covers \
the learning, PATCH it first. It was in play; it's the right place.\n\
  2. UPDATE AN EXISTING UMBRELLA (skills_list + skill_view to find the \
right one). Patch it.\n\
  3. ADD A SUPPORT FILE under an existing umbrella via skill_manage \
action=write_file. Three kinds: `references/<topic>.md` for \
session-specific detail OR condensed knowledge banks (quoted research, \
API docs excerpts, domain notes) written concise and task-focused; \
`templates/<name>.<ext>` for starter files meant to be copied and \
modified; `scripts/<name>.<ext>` for statically re-runnable actions \
(verification, fixture generators, probes). Add a one-line pointer \
in SKILL.md so future agents find them.\n\
  4. CREATE A NEW CLASS-LEVEL UMBRELLA when nothing exists. Name at the \
class level — NOT a PR number, error string, codename, \
library-alone name, or 'fix-X / debug-Y' session artifact. If the \
name only fits today's task, fall back to (1), (2), or (3).\n\
\n\
If you notice overlapping existing skills, mention it — the background \
curator handles consolidation.\n\
\n\
Do NOT capture as skills (these become persistent self-imposed constraints \
that bite you later when the environment changes):\n\
  • Environment-dependent failures: missing binaries, fresh-install errors, \
post-migration path mismatches, 'command not found', unconfigured \
credentials, uninstalled packages.\n\
  • Negative claims about tools or features ('browser tools do not work', \
'X tool is broken', 'cannot use Y from execute_code'). These harden \
into refusals the agent cites against itself for months after the \
actual problem was fixed.\n\
  • Session-specific transient errors that resolved before the conversation \
ended. If retrying worked, the lesson is the retry pattern, not the \
original failure.\n\
  • One-off task narratives. A user asking 'summarize today's market' or \
'analyze this PR' is not a class of work that warrants a skill.\n\
\n\
If a tool failed because of setup state, capture the FIX (install command, \
config step, env var to set) under an existing setup or troubleshooting \
skill — never 'this tool does not work' as a standalone constraint.\n\
\n\
Act on the skill dimension. If genuinely nothing stands out, say 'Nothing \
to save.' and stop — but don't reach for that conclusion as a default.";

/// Combined memory + skill review prompt.
pub const COMBINED_REVIEW_PROMPT: &str = "\
Review the conversation above and update two things:\n\
\n\
**Memory**: who the user is. Did the user reveal persona, desires, \
preferences, personal details, or expectations about how you should behave? \
Save facts about the user and durable preferences with the memory tool.\n\
\n\
**Skills**: how to do this class of task. Be ACTIVE — most sessions produce \
at least one skill update. A pass that does nothing is a missed learning \
opportunity, not a neutral outcome.\n\
\n\
Target shape of the skill library: CLASS-LEVEL skills with a rich SKILL.md \
and a `references/` directory for session-specific detail. Not a long flat \
list of narrow one-session-one-skill entries.\n\
\n\
Signals that warrant a skill update (any one is enough):\n\
  • User corrected your style, tone, format, legibility, verbosity, or \
approach. Frustration is a FIRST-CLASS skill signal, not just a memory \
signal. 'stop doing X', 'don't format like this', 'I hate when you Y' \
— embed the lesson in the skill that governs that task so the next \
session starts fixed.\n\
  • Non-trivial technique, fix, workaround, or debugging path emerged.\n\
  • A skill that was loaded or consulted turned out wrong, missing, or \
outdated — patch it now.\n\
\n\
Preference order for skills — pick the earliest that fits:\n\
  1. UPDATE A CURRENTLY-LOADED SKILL. Check what skills were loaded via \
/skill-name or skill_view in the conversation. If one of them covers \
the learning, PATCH it first. It was in play; it's the right place.\n\
  2. UPDATE AN EXISTING UMBRELLA (skills_list + skill_view to find the \
right one). Patch it.\n\
  3. ADD A SUPPORT FILE under an existing umbrella via skill_manage \
action=write_file. Three kinds: `references/<topic>.md` for \
session-specific detail OR condensed knowledge banks; `templates/\
<name>.<ext>` for starter files; `scripts/<name>.<ext>` for \
statically re-runnable actions. Add a one-line pointer in SKILL.md \
so future agents find them.\n\
  4. CREATE A NEW CLASS-LEVEL UMBRELLA when nothing exists. Name at the \
class level — NOT a PR number, error string, codename, \
library-alone name, or 'fix-X / debug-Y' session artifact.\n\
\n\
User-preference embedding: when the user complains about how you handled \
a task, update the skill that governs that task — memory alone isn't enough. \
Memory says 'who the user is and what the current situation and state of \
your operations are'; skills say 'how to do this class of task for this \
user'. Both should carry user-preference lessons when relevant.\n\
\n\
If you notice overlapping existing skills, mention it — the background \
curator handles consolidation.\n\
\n\
Do NOT capture as skills:\n\
  • Environment-dependent failures: missing binaries, fresh-install errors, \
post-migration path mismatches, 'command not found', unconfigured \
credentials, uninstalled packages.\n\
  • Negative claims about tools or features ('browser tools do not work', \
'X tool is broken'). These harden into refusals the agent cites against \
itself for months after the actual problem was fixed.\n\
  • Session-specific transient errors that resolved before the conversation \
ended. If retrying worked, the lesson is the retry pattern, not the \
original failure.\n\
  • One-off task narratives. A user asking 'summarize today's market' or \
'analyze this PR' is not a class of work that warrants a skill.\n\
\n\
If a tool failed because of setup state, capture the FIX (install command, \
config step, env var to set) under an existing setup or troubleshooting \
skill — never 'this tool does not work' as a standalone constraint.\n\
\n\
Act on whichever of the two dimensions has real signal. If genuinely \
nothing stands out on either, say 'Nothing to save.' and stop — but don't \
reach for that conclusion as a default.";

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
        assert_eq!(select_review_prompt(true, true), Some(COMBINED_REVIEW_PROMPT));
    }

    #[test]
    fn test_select_memory_only() {
        assert_eq!(select_review_prompt(true, false), Some(MEMORY_REVIEW_PROMPT));
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
    }

    #[test]
    fn test_combined_prompt_contains_both() {
        assert!(COMBINED_REVIEW_PROMPT.contains("Memory"));
        assert!(COMBINED_REVIEW_PROMPT.contains("Skills"));
        assert!(COMBINED_REVIEW_PROMPT.contains("memory tool"));
        assert!(COMBINED_REVIEW_PROMPT.contains("skill library"));
    }
}
