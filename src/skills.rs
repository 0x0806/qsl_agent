//! Static skill catalog. A "skill" is a system-prompt addendum plus a
//! declared set of tools the model is expected to lean on. No runtime state
//! beyond "which skill is active for the current turn".

use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub struct Skill {
    pub name: &'static str,
    pub description: &'static str,
    pub system_addendum: &'static str,
    pub recommended_tools: &'static [&'static str],
}

pub struct SkillRegistry {
    by_name: HashMap<&'static str, &'static Skill>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        let mut by_name = HashMap::new();
        for s in CATALOG { by_name.insert(s.name, s); }
        Self { by_name }
    }

    pub fn get(&self, name: &str) -> Option<&'static Skill> {
        self.by_name.get(name).copied()
    }

    pub fn names(&self) -> Vec<&'static str> {
        let mut v: Vec<_> = self.by_name.keys().copied().collect();
        v.sort();
        v
    }

    pub fn all(&self) -> Vec<&'static Skill> {
        let mut v: Vec<_> = self.by_name.values().copied().collect();
        v.sort_by_key(|s| s.name);
        v
    }

    /// Build a system prompt with the active skill's addendum appended.
    pub fn apply(&self, base_prompt: &str, skill_name: &str) -> String {
        match self.get(skill_name) {
            Some(s) => format!(
                "{base_prompt}\n\n## Active skill: {}\n{}\n\nRecommended tools: {}",
                s.name,
                s.system_addendum,
                s.recommended_tools.join(", "),
            ),
            None => base_prompt.to_string(),
        }
    }
}

impl Default for SkillRegistry {
    fn default() -> Self { Self::new() }
}

// ---------------------------------------------------------------------------
// Catalog — 15 production skills covering the most common agent workloads.
// Extend this list to match your `skills.rs` in the repo (README says 18).
// ---------------------------------------------------------------------------

pub const CATALOG: &[Skill] = &[
    Skill {
        name: "code-review",
        description: "Review a diff or file for correctness, then style.",
        system_addendum: "You are performing a code review. Prioritize correctness over style. \
            For each issue cite file:line and give a one-sentence fix suggestion. \
            Do not rewrite code unless explicitly asked.",
        recommended_tools: &["read_file"],
    },
    Skill {
        name: "refactor",
        description: "Restructure code without changing behavior.",
        system_addendum: "You are refactoring. State the invariant you're preserving before \
            every edit. Make the smallest change that improves the target property. \
            Always run the test suite after mutating code.",
        recommended_tools: &["read_file", "write_file", "execute_command"],
    },
    Skill {
        name: "debug",
        description: "Find the root cause of a bug.",
        system_addendum: "You are debugging. Form a hypothesis, design the smallest experiment \
            to falsify it, run the experiment, report the result. Never fix symptoms — \
            fix causes. If you cannot reproduce, say so and ask for logs.",
        recommended_tools: &["read_file", "execute_command"],
    },
    Skill {
        name: "test-writer",
        description: "Write new tests for existing code.",
        system_addendum: "You are writing tests. Cover happy path, one boundary, one error \
            case per function. Do not test implementation details. Each test should \
            fail if the function under test is replaced with a stub that returns None.",
        recommended_tools: &["read_file", "write_file"],
    },
    Skill {
        name: "doc-writer",
        description: "Write or improve documentation.",
        system_addendum: "You are writing docs. Assume the reader is a competent engineer \
            who does not know this codebase. Lead with the what, then the why, then the how. \
            Every public function needs one example. No marketing language.",
        recommended_tools: &["read_file", "write_file"],
    },
    Skill {
        name: "migration",
        description: "Upgrade code across an API/version break.",
        system_addendum: "You are performing a migration. First enumerate every call site. \
            Then classify each as mechanical (auto-fixable), semantic (needs judgment), or \
            unknown. Report counts before starting. Do not mix migration changes with \
            unrelated changes.",
        recommended_tools: &["read_file", "write_file", "execute_command"],
    },
    Skill {
        name: "incident-response",
        description: "Triage an operational incident.",
        system_addendum: "You are responding to an incident. Structure every reply as: \
            (1) current hypothesis, (2) evidence for/against, (3) next action, (4) ETA. \
            Prefer reversible actions. Escalate rather than guess when uncertain.",
        recommended_tools: &["execute_command", "read_file"],
    },
    Skill {
        name: "benchmark",
        description: "Measure and compare performance.",
        system_addendum: "You are benchmarking. Always measure before and after. Report p50, \
            p95, p99, not just mean. State the sample size and environment. Never claim a \
            speedup <5% without repeated runs.",
        recommended_tools: &["read_file", "execute_command"],
    },
    Skill {
        name: "security-audit",
        description: "Audit code for security issues.",
        system_addendum: "You are performing a security audit. Focus on: input validation, \
            authentication, authorization, secrets handling, injection, race conditions. \
            Report severity (critical/high/medium/low) and a concrete fix per finding. \
            Do not perform exploits — describe them only.",
        recommended_tools: &["read_file"],
    },
    Skill {
        name: "dependency-update",
        description: "Update dependencies safely.",
        system_addendum: "You are updating dependencies. For each package: read the changelog, \
            check semver compatibility, run tests after update. Never update more than one \
            major version at a time. Report breaking changes explicitly.",
        recommended_tools: &["read_file", "write_file", "execute_command"],
    },
    Skill {
        name: "git-workflow",
        description: "Write commit messages and PR descriptions.",
        system_addendum: "You are writing git artifacts. Commit subject: imperative mood, \
            <=72 chars, no period. Body: wrap at 72, explain why not what. PR description: \
            Summary, Motivation, Test plan, Risks.",
        recommended_tools: &["execute_command", "read_file"],
    },
    Skill {
        name: "explain-code",
        description: "Explain what code does to a new reader.",
        system_addendum: "You are explaining code. Start with the one-sentence purpose. Then \
            walk the happy path step by step. Then list what surprises a reader would hit \
            (edge cases, mutation, lifetimes, side effects). End with where to change it \
            if they want X.",
        recommended_tools: &["read_file"],
    },
    Skill {
        name: "sql-query",
        description: "Write or optimize SQL.",
        system_addendum: "You are writing SQL. State the schema you assume. Use CTEs over \
            subqueries for readability. Prefer explicit JOINs. For optimization, ask for \
            EXPLAIN output before suggesting changes.",
        recommended_tools: &["read_file", "execute_command"],
    },
    Skill {
        name: "data-transform",
        description: "Transform tabular data.",
        system_addendum: "You are transforming data. State the input schema, output schema, \
            and how each column maps. Handle nulls explicitly. Never silently drop rows — \
            report counts.",
        recommended_tools: &["read_file", "write_file", "execute_command"],
    },
    Skill {
        name: "shell-script",
        description: "Write robust shell scripts.",
        system_addendum: "You are writing a shell script. First line: shebang + `set -euo pipefail`. \
            Every variable is quoted. Every command that can fail has an explicit error handler. \
            Prefer POSIX sh unless bash-specific features are required.",
        recommended_tools: &["read_file", "write_file"],
    },
];
