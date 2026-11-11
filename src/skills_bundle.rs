//! Built-in skills that ship as a folder (a `SKILL.md` plus companion files).
//! Each is embedded in the binary and seeded to `<vault>/QSL_Agent/Skills/<name>/`.
//!
//! The QSL Security Audit material derives from an MIT-licensed upstream skill;
//! see `THIRD_PARTY_NOTICES.md` and `LICENSE-MIT.txt` beside the skill files.

pub struct BundleSkill {
    pub name: &'static str,
    pub description: &'static str,
    pub tags: &'static [&'static str],
    /// Bump to upgrade vault copies that carry the `managed: qsl-agent` marker.
    pub version: &'static str,
    pub skill_md: &'static str,
    /// Companion files, relative to the skill directory.
    pub files: &'static [(&'static str, &'static str)],
}

pub const BUNDLE_SKILLS: &[BundleSkill] = &[BundleSkill {
    name: "qsl-security-audit",
    description: "Source-first security audit and vulnerability review of a codebase: reconnaissance, coverage-led hunting, adversarial validation, schema-checked findings and a report. Use for security audits, pen-test style reviews, vulnerability research, or focused security questions.",
    tags: &["security", "audit", "vulnerability", "pentest", "review", "appsec", "llm-security"],
    version: "1.0.0",
    skill_md: include_str!("skills_bundles/qsl-security-audit/SKILL.md"),
    files: QSL_SECURITY_AUDIT_FILES,
}];

const QSL_SECURITY_AUDIT_FILES: &[(&str, &str)] = &[
    (
        "AI-AND-LLM.md",
        include_str!("skills_bundles/qsl-security-audit/AI-AND-LLM.md"),
    ),
    (
        "ATTACK-CLASSES.md",
        include_str!("skills_bundles/qsl-security-audit/ATTACK-CLASSES.md"),
    ),
    (
        "CLIENT-SIDE.md",
        include_str!("skills_bundles/qsl-security-audit/CLIENT-SIDE.md"),
    ),
    (
        "CLOUD-AND-DEPLOYMENT.md",
        include_str!("skills_bundles/qsl-security-audit/CLOUD-AND-DEPLOYMENT.md"),
    ),
    (
        "DATA-ISOLATION-AND-LIFECYCLE.md",
        include_str!("skills_bundles/qsl-security-audit/DATA-ISOLATION-AND-LIFECYCLE.md"),
    ),
    (
        "DESKTOP-MOBILE-AND-LOCAL-IPC.md",
        include_str!("skills_bundles/qsl-security-audit/DESKTOP-MOBILE-AND-LOCAL-IPC.md"),
    ),
    (
        "HUNTING.md",
        include_str!("skills_bundles/qsl-security-audit/HUNTING.md"),
    ),
    (
        "LICENSE-MIT.txt",
        include_str!("skills_bundles/qsl-security-audit/LICENSE-MIT.txt"),
    ),
    (
        "MEMORY-SAFETY-AND-BINARY.md",
        include_str!("skills_bundles/qsl-security-audit/MEMORY-SAFETY-AND-BINARY.md"),
    ),
    (
        "PROTOCOLS-RPC-AND-MESSAGING.md",
        include_str!("skills_bundles/qsl-security-audit/PROTOCOLS-RPC-AND-MESSAGING.md"),
    ),
    (
        "RECONNAISSANCE.md",
        include_str!("skills_bundles/qsl-security-audit/RECONNAISSANCE.md"),
    ),
    (
        "RESOURCE-EXHAUSTION-AND-AVAILABILITY.md",
        include_str!("skills_bundles/qsl-security-audit/RESOURCE-EXHAUSTION-AND-AVAILABILITY.md"),
    ),
    (
        "SUPPLY-CHAIN-AND-RELEASE.md",
        include_str!("skills_bundles/qsl-security-audit/SUPPLY-CHAIN-AND-RELEASE.md"),
    ),
    (
        "VALIDATION-AND-REPORTING.md",
        include_str!("skills_bundles/qsl-security-audit/VALIDATION-AND-REPORTING.md"),
    ),
    (
        "WEB-PROTOCOL-AND-AUTH.md",
        include_str!("skills_bundles/qsl-security-audit/WEB-PROTOCOL-AND-AUTH.md"),
    ),
    (
        "report-schema.json",
        include_str!("skills_bundles/qsl-security-audit/report-schema.json"),
    ),
    (
        "validate-coverage-ledger.cjs",
        include_str!("skills_bundles/qsl-security-audit/validate-coverage-ledger.cjs"),
    ),
    (
        "validate-coverage-ledger.test.cjs",
        include_str!("skills_bundles/qsl-security-audit/validate-coverage-ledger.test.cjs"),
    ),
    (
        "validate-findings.cjs",
        include_str!("skills_bundles/qsl-security-audit/validate-findings.cjs"),
    ),
    (
        "validate-findings.test.cjs",
        include_str!("skills_bundles/qsl-security-audit/validate-findings.test.cjs"),
    ),
];
