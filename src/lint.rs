//! Static analysis of a template, without rendering it.
//!
//! Everything here is a parse or a string inspection. Nothing is evaluated,
//! nothing is fetched, and no project is required — which is the point: the
//! failures this catches are the ones that otherwise surface in someone else's
//! generated repository.
//!
//! Two of the checks exist because of specific, silent failures:
//!
//! - A conditional path segment that leaves its suffix outside the block. With
//!   two such files you get a collision, named and diagnosed. With one you get
//!   a file called `.yaml`, and nothing says so.
//! - A `.jinja` file emitting another templating language. `${{ github.ref }}`
//!   is inside MiniJinja's syntax, so it renders to `$` and leaves valid YAML
//!   behind.

use miette::Diagnostic;
use thiserror::Error;

use crate::eval::{Partials, environment};
use crate::git::{GitBackend, TreeEntry};
use crate::graph::{Graph, GraphError};
use crate::render::TEMPLATE_SUFFIX;
use crate::template::{MANIFEST_NAME, Manifest, TOP_LEVEL_KEYS};

/// Every rule this module can report, sorted.
///
/// The list a `--deny` or `--allow` argument is checked against, so that a
/// typo in CI fails loudly instead of quietly denying nothing. A new rule
/// registers itself here; `tests/diagnostics.rs` enforces that the set and the
/// reference page agree.
pub const CODES: &[&str] = &[
    "tpl::lint::absorbed_key",
    "tpl::lint::collision",
    "tpl::lint::degenerate_path",
    "tpl::lint::foreign_expression",
    "tpl::lint::missing_note_file",
    "tpl::lint::shadowed_name",
    "tpl::lint::syntax",
    "tpl::lint::undeclared",
];

/// The word that stands for the whole warning severity in `--deny`/`--allow`.
///
/// Spelled as clippy spells it, because the audience types
/// `cargo clippy -- -D warnings` daily.
pub const WARNINGS: &str = "warnings";

/// How much a finding matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The template will fail, or produce something nobody asked for.
    Error,
    /// Suspicious, but a template may legitimately mean it.
    Warning,
}

impl Severity {
    /// The machine-readable name.
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// One thing wrong with a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// How much it matters.
    pub severity: Severity,
    /// The stable diagnostic code.
    pub code: &'static str,
    /// What is wrong.
    pub message: String,
    /// What to do about it.
    pub help: String,
    /// The template path it concerns, if it concerns one.
    pub path: Option<String>,
}

impl Finding {
    fn error(code: &'static str, path: &str, message: String, help: String) -> Self {
        Self {
            severity: Severity::Error,
            code,
            message,
            help,
            path: Some(path.to_string()),
        }
    }

    fn warning(code: &'static str, path: &str, message: String, help: String) -> Self {
        Self {
            severity: Severity::Warning,
            code,
            message,
            help,
            path: Some(path.to_string()),
        }
    }
}

/// Errors that stop a lint before it can produce findings.
#[derive(Debug, Error, Diagnostic)]
pub enum LintError {
    /// The manifest is invalid, or its dependency graph is.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Graph(#[from] GraphError),

    /// The template tree could not be read.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Git(#[from] crate::git::GitError),

    /// A `--deny` or `--allow` names something that is not a rule.
    ///
    /// An error rather than a shrug: the whole point of the flag is to make a
    /// build fail, and a misspelled code that is accepted silently denies
    /// nothing. The failure would be a green CI run, which is the one outcome
    /// nobody checks.
    #[error("`{spelling}` is not a lint code")]
    #[diagnostic(code(tpl::lint::unknown_code), help("{help}"))]
    UnknownCode {
        /// What was written on the command line.
        spelling: String,
        /// The valid vocabulary, and the nearest match if there is one.
        help: String,
    },

    /// The same code was both denied and allowed.
    ///
    /// Neither answer is defensible, and picking one by argument order would
    /// make the meaning depend on how a CI fragment was assembled.
    #[error("`{spelling}` is both denied and allowed")]
    #[diagnostic(
        code(tpl::lint::conflicting_level),
        help(
            "`--deny` and `--allow` disagree about the same thing, so there is no verdict \
             to reach. Drop one of them. A named code overrides `warnings`, so \
             `--deny warnings --allow tpl::lint::undeclared` is how an exception is spelled."
        )
    )]
    ConflictingLevel {
        /// The code, or `warnings`, named by both flags.
        spelling: String,
    },
}

/// What `--deny` and `--allow` were asked for.
///
/// Resolution is by *specificity*, not by position: a named code always beats
/// the `warnings` bucket, whichever flag came first. Clippy resolves by
/// position, but CI arguments get reordered by shell fragments and composed
/// configs, and a rule whose meaning depends on argument order is a rule that
/// changes meaning silently.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Levels {
    /// `--deny warnings`.
    deny_warnings: bool,
    /// `--allow warnings`.
    allow_warnings: bool,
    /// Codes named by `--deny`.
    deny: std::collections::BTreeSet<String>,
    /// Codes named by `--allow`.
    allow: std::collections::BTreeSet<String>,
}

/// A finding, with the verdict the levels reached about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// The finding itself, with its native severity intact.
    pub finding: Finding,
    /// Whether a `--deny` made it fatal.
    ///
    /// Kept beside the severity rather than rewriting it, so a caller can tell
    /// a rule the template broke from a policy this run applied.
    pub denied: bool,
}

impl Levels {
    /// Validate the two argument lists.
    ///
    /// Fails before the tree is walked, so a typo costs no work.
    pub fn parse(deny: &[String], allow: &[String]) -> Result<Self, LintError> {
        let mut levels = Self::default();
        for spelling in deny {
            match check(spelling)? {
                Selector::Warnings => levels.deny_warnings = true,
                Selector::Code(code) => {
                    levels.deny.insert(code.to_string());
                }
            }
        }
        for spelling in allow {
            match check(spelling)? {
                Selector::Warnings => levels.allow_warnings = true,
                Selector::Code(code) => {
                    levels.allow.insert(code.to_string());
                }
            }
        }

        // Only a conflict at the *same* specificity is ambiguous. A named code
        // against `warnings` is the exception mechanism, not a mistake.
        if levels.deny_warnings && levels.allow_warnings {
            return Err(LintError::ConflictingLevel {
                spelling: WARNINGS.to_string(),
            });
        }
        if let Some(both) = levels.deny.intersection(&levels.allow).next() {
            return Err(LintError::ConflictingLevel {
                spelling: both.clone(),
            });
        }

        Ok(levels)
    }

    /// Whether anything was asked for at all.
    pub fn is_empty(&self) -> bool {
        !self.deny_warnings && !self.allow_warnings && self.deny.is_empty() && self.allow.is_empty()
    }

    /// Drop the allowed findings, and mark the denied ones.
    pub fn apply(&self, findings: Vec<Finding>) -> Vec<Verdict> {
        findings
            .into_iter()
            .filter(|finding| !self.allows(finding))
            .map(|finding| Verdict {
                // An error is already fatal; `denied` records a promotion, and
                // saying an error was denied would make the JSON count wrong.
                denied: finding.severity == Severity::Warning && self.denies(&finding),
                finding,
            })
            .collect()
    }

    /// Whether this finding should not be reported at all.
    fn allows(&self, finding: &Finding) -> bool {
        if self.deny.contains(finding.code) {
            return false; // The named code wins over `--allow warnings`.
        }
        self.allow.contains(finding.code)
            || (self.allow_warnings && finding.severity == Severity::Warning)
    }

    /// Whether this finding should fail the command.
    fn denies(&self, finding: &Finding) -> bool {
        if self.allow.contains(finding.code) {
            return false; // The named code wins over `--deny warnings`.
        }
        self.deny.contains(finding.code) || self.deny_warnings
    }
}

/// What one `--deny`/`--allow` value selects.
enum Selector<'a> {
    /// Every warning.
    Warnings,
    /// One rule.
    Code(&'a str),
}

/// Reject anything that is not `warnings` or a known code.
fn check(spelling: &str) -> Result<Selector<'_>, LintError> {
    if spelling == WARNINGS {
        return Ok(Selector::Warnings);
    }
    if let Some(code) = CODES.iter().find(|code| **code == spelling) {
        return Ok(Selector::Code(code));
    }

    // The nearest match first: the common mistake is a remembered code, not an
    // invented one, and reading a list of six to find a typo is work.
    let suggestion = crate::suggest::closest(spelling, CODES.iter().copied().chain([WARNINGS]))
        .map(|close| format!("Did you mean `{close}`? "))
        .unwrap_or_default();
    Err(LintError::UnknownCode {
        spelling: spelling.to_string(),
        help: format!(
            "{suggestion}Valid values are `{WARNINGS}` and: {}",
            CODES.join(", ")
        ),
    })
}

/// Lint a resolved template.
///
/// `entries` is the render root, flattened. `repo_entries` is the whole
/// repository, flattened — a `note_file` and a partial live there rather than
/// in the render root, and the two are different path namespaces. `partials`
/// are the importable `.jinja` blobs from outside the root.
///
/// `manifest_text` is the raw `template.toml`. It is not redundant with
/// `manifest`: a key absorbed by a preceding table header is gone by the time
/// the manifest is deserialised, and only the source says it was ever written.
pub fn lint(
    template: &dyn GitBackend,
    manifest: &Manifest,
    manifest_text: &str,
    entries: &[TreeEntry],
    repo_entries: &[TreeEntry],
    partials: &std::sync::Arc<Partials>,
) -> Result<Vec<Finding>, LintError> {
    // The manifest and its graph first: a cycle or an unknown reference makes
    // every later finding untrustworthy, so it is an error rather than a
    // finding. `Manifest::parse` has already run by the time we are here.
    Graph::build(manifest)?;

    let mut findings = Vec::new();

    for entry in entries {
        if !entry.mode.is_blob() {
            continue;
        }
        findings.extend(check_path(&entry.path));

        if !entry.path.ends_with(TEMPLATE_SUFFIX) {
            continue;
        }
        let source = template.read_blob(entry.oid)?;
        // A binary `.jinja` is copied, not rendered, so parsing it would be
        // reporting on something that never happens.
        if source.iter().take(8192).any(|byte| *byte == 0) {
            continue;
        }
        let text = String::from_utf8_lossy(&source);
        findings.extend(check_syntax(&entry.path, &text, partials));

        // The roots of every `${{ ... }}` MiniJinja would eat. They are
        // undeclared *by construction* — `matrix` and `github` belong to
        // GitHub, not to the template — so reporting them again under
        // `undeclared` would be the same bug twice, and the second report
        // would advise declaring a name the author must not declare.
        let foreign = foreign_expression_roots(&text);
        findings.extend(check_foreign_expressions(&entry.path, &text));
        findings.extend(check_undeclared(
            &entry.path,
            &text,
            manifest,
            partials,
            &foreign,
        ));
        findings.extend(check_shadowed_names(&entry.path, &text, manifest));
    }

    findings.extend(check_path_collisions(entries));
    findings.extend(check_note_file(manifest, repo_entries));
    findings.extend(check_absorbed_keys(manifest_text));

    Ok(findings)
}

/// A top-level key written after a table header, and so absorbed by it.
///
/// In TOML a bare key belongs to the table that most recently opened, so a
/// `note_file = "NOTE.md"` written below `[computed]` is a computed value and
/// below `[remotes]` is a Git remote. Both are valid TOML, both load without
/// complaint, and both do something other than what was written — the note
/// simply never appears, or a remote nobody asked for is added at `init`.
///
/// Invisible after deserialisation: `Manifest::note_file` is `None`, which is
/// exactly what a template declaring no note looks like, so `missing_note_file`
/// short-circuits. The raw source is the only place the mistake still exists.
///
/// A warning rather than an error, because a computed value or a remote
/// genuinely named `name` is conceivable, if unlikely.
fn check_absorbed_keys(text: &str) -> Vec<Finding> {
    // A manifest that does not parse never reaches `lint`; if one somehow did,
    // the parse error is the finding to report, not a guess about its shape.
    let Ok(document) = text.parse::<toml::Table>() else {
        return Vec::new();
    };

    let mut findings = Vec::new();
    for (name, value) in &document {
        // The top level itself is exempt: that is where these keys belong.
        collect_absorbed_keys(&mut findings, name, value);
    }
    findings
}

/// Report every top-level key name used inside `table`, depth-first.
///
/// Only non-table values are reported. An absorbed key is always a scalar —
/// `name`, `note`, `note_file`, `root` and `description` are strings and
/// `strict` is a boolean — so skipping table values is what lets `[data.note_file]`,
/// a data source legitimately named after a top-level key, go unreported
/// without a per-table list of exceptions to maintain.
fn collect_absorbed_keys(findings: &mut Vec<Finding>, path: &str, value: &toml::Value) {
    match value {
        toml::Value::Table(table) => {
            for (key, nested) in table {
                if !nested.is_table() && TOP_LEVEL_KEYS.contains(&key.as_str()) {
                    findings.push(Finding::warning(
                        "tpl::lint::absorbed_key",
                        MANIFEST_NAME,
                        format!("`{key}` is a top-level key, but it is written inside `[{path}]`"),
                        format!(
                            "in TOML a bare key belongs to the table that most recently \
                             opened, so this declares `{path}.{key}` and the top-level \
                             `{key}` is never set. Move it above the first table header. \
                             Pass `--allow tpl::lint::absorbed_key` if `{path}` really \
                             does have an entry of its own by that name."
                        ),
                    ));
                }
                collect_absorbed_keys(findings, &format!("{path}.{key}"), nested);
            }
        }
        // An array of tables — `[[questions.x.choices]]` — opens a table too,
        // and absorbs whatever bare key follows it just the same.
        toml::Value::Array(items) => {
            for item in items {
                collect_absorbed_keys(findings, path, item);
            }
        }
        _ => {}
    }
}

/// A `note_file` that names nothing in the template repository.
///
/// A render will not tell the author: the path is checked at `init`, which is
/// the one place they are unlikely to be. This reports it without a repository
/// and without answers, which is what `lint` is for.
///
/// Matched exactly. Unlike a rendered file, no `.jinja` suffix is stripped —
/// `note_file = "N.md.jinja"` is how an author asks for a rendered note, and
/// treating `N.md` as a match for it would make the two indistinguishable.
///
/// Only a literal path can be checked. One containing an expression depends on
/// the answers, and a lint has none.
fn check_note_file(manifest: &Manifest, repo_entries: &[TreeEntry]) -> Vec<Finding> {
    let Some(declared) = &manifest.note_file else {
        return Vec::new();
    };

    if declared.contains("{{") || declared.contains("{%") {
        return Vec::new();
    }

    let wanted = declared.trim();
    // Against the *repository* tree, not the render root: a note is read from
    // the template and never rendered into the project, so it is in the same
    // path namespace as a partial.
    let exists = repo_entries
        .iter()
        .any(|entry| entry.mode.is_blob() && entry.path == wanted);

    if exists {
        return Vec::new();
    }

    vec![Finding::error(
        "tpl::lint::missing_note_file",
        wanted,
        format!("`note_file` names `{wanted}`, which the template repository does not contain"),
        "the path is relative to the repository root, not to the render root — \
         a note beside the manifest is `NEXT-STEPS.md`, not \
         `template/NEXT-STEPS.md`. An `init` will refuse rather than show \
         nothing."
            .into(),
    )]
}

/// The trap: a conditional segment whose suffix sits outside the block.
///
/// `{% if msrv %}msrv{% endif %}.yaml` renders to `.yaml` when `msrv` is false
/// — a real file, with a real name, that passes every check the renderer makes.
/// The whole filename has to be inside the conditional.
///
/// A `.jinja` file is the exception, and the reason this cannot be a naive
/// string match: the suffix is stripped from the path *before* the segments are
/// rendered, so `{% if docs %}zensical.toml{% endif %}.jinja` is correct and
/// collapses to nothing. Flagging it would make the check unusable on precisely
/// the templates that need it most.
fn check_path(path: &str) -> Vec<Finding> {
    let stripped = path.strip_suffix(TEMPLATE_SUFFIX).unwrap_or(path);

    let mut findings = Vec::new();
    for segment in stripped.split('/') {
        if !segment.contains("{%") {
            continue;
        }
        let Some(residue) = literal_residue(segment) else {
            continue;
        };
        if residue.is_empty() {
            continue;
        }
        findings.push(Finding::error(
            "tpl::lint::degenerate_path",
            path,
            format!(
                "`{segment}` renders to `{residue}` when its condition is false, \
                 rather than being skipped"
            ),
            format!(
                "a path segment is skipped only when it renders *empty*. Move the whole \
                 name inside the block: `{{% if ... %}}{}{{% endif %}}`",
                segment_name(segment)
            ),
        ));
    }
    findings
}

/// What a segment renders to when every `{% if %}` takes its false branch.
///
/// Returns `None` when the segment has no `{% if %}`, or when it also
/// interpolates a `{{ ... }}` — an expression can render to anything, so the
/// false case is not statically knowable and a guess would be a false positive.
fn literal_residue(segment: &str) -> Option<String> {
    if segment.contains("{{") || !segment.contains("{%") {
        return None;
    }
    split_conditional(segment).map(|(_, outside)| outside)
}

/// Split a segment into what a taken branch would contribute, and what sits
/// outside every block.
///
/// The second is the residue: the text that survives when no branch is taken.
/// The first is the name the author is trying to build, which the help text
/// needs in order to say what the fix looks like.
///
/// `None` when the tags are unbalanced — `check_syntax` reports that properly,
/// and guessing here would produce a second, worse diagnostic for one mistake.
fn split_conditional(segment: &str) -> Option<(String, String)> {
    let mut inside = String::new();
    let mut outside = String::new();
    let mut depth = 0usize;
    let mut saw_if = false;
    let mut rest = segment;

    while let Some(at) = rest.find("{%") {
        let text = &rest[..at];
        if depth == 0 {
            outside.push_str(text);
        } else {
            inside.push_str(text);
        }

        let after = &rest[at + 2..];
        let end = after.find("%}")?;
        let tag = after[..end].trim().trim_matches('-').trim();

        if tag.starts_with("if ") || tag == "if" {
            saw_if = true;
            depth += 1;
        } else if tag == "endif" {
            depth = depth.checked_sub(1)?;
        }

        rest = &after[end + 2..];
    }

    if depth != 0 {
        return None;
    }
    outside.push_str(rest);

    saw_if.then_some((inside, outside))
}

/// The name a segment is trying to produce, for the help text.
fn segment_name(segment: &str) -> String {
    match split_conditional(segment) {
        Some((inside, outside)) if !inside.is_empty() || !outside.is_empty() => {
            format!("{inside}{outside}")
        }
        _ => segment.to_string(),
    }
}

/// Every `.jinja` file must parse, on every branch.
///
/// Otherwise a syntax error in a conditional nobody has answered their way into
/// is found by the first person who does.
fn check_syntax(path: &str, text: &str, partials: &std::sync::Arc<Partials>) -> Vec<Finding> {
    let env = environment(partials);
    match env.template_from_str(text) {
        Ok(_) => Vec::new(),
        Err(error) => vec![Finding::error(
            "tpl::lint::syntax",
            path,
            format!("`{path}` is not a valid template: {error}"),
            "fix the template syntax; the whole file is parsed, including branches \
             no answer set reaches"
                .to_string(),
        )],
    }
}

/// A `.jinja` file emitting another `{{ }}` language.
///
/// GitHub Actions is the common case and the reason this exists: `${{ }}`
/// contains `{{`, so MiniJinja consumes it, `${{ github.ref }}` renders to `$`,
/// and the result is still valid YAML. Nothing fails until a workflow run does.
///
/// `${{ '{{' }}` is *not* flagged: that is the escape idiom, a MiniJinja
/// expression whose whole body is a string literal, and an author who wrote it
/// knew exactly what they were doing. Flagging it would make the check
/// unusable on the files that most need it — a workflow that interpolates
/// anything at all has to escape this way on every line.
fn check_foreign_expressions(path: &str, text: &str) -> Vec<Finding> {
    let leaked = foreign_expressions(text);
    if leaked.is_empty() {
        return Vec::new();
    }
    vec![Finding::warning(
        "tpl::lint::foreign_expression",
        path,
        format!(
            "`{path}` has {} expression(s) MiniJinja will consume: {}",
            leaked.len(),
            leaked.join(", ")
        ),
        "`${{ github.ref }}` renders to `$` and leaves valid YAML behind, so nothing \
         fails until the workflow runs. Wrap the region in `{% raw %}...{% endraw %}`, \
         drop the `.jinja` suffix so the file is copied byte-for-byte, or escape it as \
         `${{ '{{' }} github.ref {{ '}}' }}`."
            .to_string(),
    )]
}

/// Every `${{ ... }}` that MiniJinja would eat, outside any raw block.
fn foreign_expressions(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut cursor = 0usize;

    while cursor < text.len() {
        let raw_open = find_tag(&text[cursor..], "raw").map(|at| cursor + at);
        let raw_close = find_tag(&text[cursor..], "endraw").map(|at| cursor + at);
        let hit = text[cursor..].find("${{").map(|at| cursor + at);

        let Some(at) = [raw_open, raw_close, hit].into_iter().flatten().min() else {
            break;
        };

        if Some(at) == hit && depth == 0 {
            // The MiniJinja tag starts one byte after the `$`.
            let body = &text[at + 3..];
            let inner = body.find("}}").map(|end| body[..end].trim()).unwrap_or("");
            // A body that is only a string literal is the escape idiom, and
            // renders to the literal — which is the author's intent, not a leak.
            if !is_string_literal(inner) {
                out.push(format!("${{{{ {inner} }}}}"));
            }
        }

        if Some(at) == raw_open && Some(at) != hit {
            depth += 1;
        } else if Some(at) == raw_close {
            depth = depth.saturating_sub(1);
        }

        cursor = at + text[at..].chars().next().map_or(1, char::len_utf8);
    }

    out
}

/// The root name of every `${{ ... }}` MiniJinja would consume.
///
/// `${{ matrix.os }}` yields `matrix`. Used to suppress the `undeclared`
/// finding for the same text, which would otherwise advise declaring a name
/// that belongs to GitHub Actions.
fn foreign_expression_roots(text: &str) -> std::collections::BTreeSet<String> {
    foreign_expressions(text)
        .into_iter()
        .filter_map(|expression| {
            let inner = expression
                .trim_start_matches("${{")
                .trim_end_matches("}}")
                .trim();
            let root = inner
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .next()?;
            (!root.is_empty()).then(|| root.to_string())
        })
        .collect()
}

/// Whether an expression body is a bare quoted string.
fn is_string_literal(inner: &str) -> bool {
    let inner = inner.trim();
    if inner.len() < 2 {
        return false;
    }
    let quote = match inner.chars().next() {
        Some(c @ ('\'' | '"')) => c,
        _ => return false,
    };
    // Only the outermost pair matters; an embedded quote would make this a
    // concatenation, which is no longer a plain literal.
    inner.ends_with(quote) && !inner[1..inner.len() - 1].contains(quote)
}

/// Find `{% <tag> %}`, tolerating whitespace control and inner spaces.
fn find_tag(text: &str, tag: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(at) = text[from..].find("{%") {
        let start = from + at;
        let after = &text[start + 2..];
        let end = after.find("%}")?;
        let inner = after[..end].trim().trim_matches('-').trim();
        if inner == tag {
            return Some(start);
        }
        from = start + 2 + end + 2;
    }
    None
}

/// A name a file body uses that the manifest never declares.
///
/// This is the asymmetry the lint exists to close. In a manifest expression an
/// unknown name is a hard error before the first prompt, with a suggestion
/// attached. In a file body MiniJinja is lenient, so `{{ typo }}` renders to
/// the empty string and the command exits 0 — leaving a `Cargo.toml` with
/// `name = ""`, which parses, or a workflow with `runs-on: `, which is valid
/// YAML. Nothing fails until a human reads it.
///
/// A warning rather than an error for now, so that flipping the renderer to
/// strict later is a change people have already been told about. See ADR-014.
fn check_undeclared(
    path: &str,
    text: &str,
    manifest: &Manifest,
    partials: &std::sync::Arc<Partials>,
    foreign: &std::collections::BTreeSet<String>,
) -> Vec<Finding> {
    let known = declared_names(manifest);

    let env = environment(partials);
    let Ok(template) = env.template_from_str(text) else {
        // `check_syntax` already reported it, and a file that does not parse has no
        // meaningful variable set.
        return Vec::new();
    };

    // `nested: false`: root names only. A dotted path like `data.licenses.ids`
    // arrives as `data`, which is what the manifest declares.
    let mut unknown: Vec<String> = template
        .undeclared_variables(false)
        .into_iter()
        .filter(|name| {
            !known.contains(name.as_str()) && !is_builtin(name) && !foreign.contains(name)
        })
        .collect();
    unknown.sort();
    unknown.dedup();

    unknown
        .into_iter()
        .map(|name| {
            let suggestion = crate::suggest::closest(&name, known.iter().copied())
                .map(|close| format!(" Did you mean `{close}`?"))
                .unwrap_or_default();
            Finding::warning(
                "tpl::lint::undeclared",
                path,
                format!("`{path}` uses `{name}`, which the template does not declare"),
                format!(
                    "MiniJinja is lenient, so this renders to an empty string and nothing \
                     fails.{suggestion} Declare it as a question or a computed value, or \
                     write `{{{{ {name} | default('') }}}}` if it is meant to be optional."
                ),
            )
        })
        .collect()
}

/// Every name a template body may legitimately use.
fn declared_names(manifest: &Manifest) -> std::collections::BTreeSet<&str> {
    manifest
        .questions
        .keys()
        .map(String::as_str)
        .chain(manifest.computed.keys().map(String::as_str))
        // Namespaces, always present in the context.
        .chain(["data", "template"])
        .collect()
}

/// Names MiniJinja itself provides, which no manifest declares.
fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "loop" | "self" | "range" | "dict" | "namespace" | "debug" | "true" | "false" | "none"
    )
}

/// A binding in a file body that hides an answer or a computed value.
///
/// `{% import "macros/m.jinja" as stack %}` binds `stack` in the same namespace
/// the answers arrive in, so a later `{% if stack == "b" %}` compares a module
/// with a string. That is never true and never an error, and `strict = true`
/// does not help because nothing is undefined. The file renders, the branch is
/// simply gone.
///
/// The manifest already refuses a question and a computed value that share a
/// name, for the same reason and with the same help. This is that collision
/// arriving from a third direction, and it was the only one nothing reported.
///
/// A warning rather than an error: the binding is legal MiniJinja, and an
/// author who means it can `--allow` the code. `-D warnings` catches it in CI,
/// which is where the reported case went unnoticed.
fn check_shadowed_names(path: &str, text: &str, manifest: &Manifest) -> Vec<Finding> {
    let known = declared_names(manifest);
    let mut shadowed: Vec<&str> = tag_bodies(text)
        .iter()
        .flat_map(|body| bound_names(body))
        // Borrow the manifest's own spelling, so the finding outlives the
        // owned name the scanner produced.
        .filter_map(|name| known.get(name.as_str()).copied())
        .collect();
    // A name bound twice in one file is one mistake, and the second report
    // would say nothing the first did not.
    shadowed.sort_unstable();
    shadowed.dedup();

    shadowed
        .into_iter()
        .map(|name| {
            let kind = if manifest.questions.contains_key(name) {
                "a question"
            } else if manifest.computed.contains_key(name) {
                "a computed value"
            } else {
                "a context namespace"
            };
            Finding::warning(
                "tpl::lint::shadowed_name",
                path,
                format!("`{path}` binds `{name}`, which is already {kind}"),
                format!(
                    "answers, computed values and template bindings share one namespace, so \
                     the rest of the file sees the binding and not {kind}. A comparison like \
                     `{{% if {name} == \"...\" %}}` then silently never matches. Rename the \
                     binding."
                ),
            )
        })
        .collect()
}

/// Every `{% ... %}` body outside a `{% raw %}` block, trimmed.
///
/// `find_tag` answers "where is this exact tag"; the shadowing check needs
/// every tag instead. Text inside a raw block is emitted verbatim, so a
/// binding written there binds nothing and must not be reported.
fn tag_bodies(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut from = 0usize;

    while let Some(at) = text[from..].find("{%") {
        let start = from + at;
        let after = &text[start + 2..];
        // Unbalanced: `check_syntax` reports it properly, and guessing past it
        // here would produce a second, worse diagnostic for one mistake.
        let Some(end) = after.find("%}") else { break };
        let body = after[..end].trim().trim_matches('-').trim();

        if body == "raw" {
            depth += 1;
        } else if body == "endraw" {
            depth = depth.saturating_sub(1);
        } else if depth == 0 {
            out.push(body.to_string());
        }

        from = start + 2 + end + 2;
    }

    out
}

/// The names a single tag body binds in the file's namespace.
///
/// Deliberately not covered:
///
/// - A macro's parameters. `{% macro badge(name) %}` shadowing a `name`
///   question inside the macro body is exactly what the author means.
/// - A target its own right-hand side mentions. `{% set x = x | default('a') %}`
///   is the self-defaulting idiom; reporting it would make the rule unusable on
///   the templates most likely to use it.
///
/// Anything that is not identifier-shaped yields nothing — `{% set ns.x = 1 %}`
/// assigns through a namespace and binds no root name, and a malformed body is
/// `check_syntax`'s to report.
fn bound_names(body: &str) -> Vec<String> {
    let (keyword, rest) = body.split_once(char::is_whitespace).unwrap_or((body, ""));
    let rest = rest.trim();

    match keyword {
        // `import "m.jinja" as alias [with context]`
        "import" => strip_context(rest)
            .rsplit_once(" as ")
            .and_then(|(_, alias)| identifier(alias.trim()))
            .into_iter()
            .collect(),
        // `from "m.jinja" import a, b as c [with context]`
        "from" => strip_context(rest)
            .split_once(" import ")
            .map(|(_, names)| {
                names
                    .split(',')
                    .filter_map(|item| {
                        let item = item.trim();
                        identifier(
                            item.rsplit_once(" as ")
                                .map_or(item, |(_, alias)| alias)
                                .trim(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
        // `set a, b = expr`, the block form `set content`, or `with` — which
        // additionally allows `a = 1, (b, c) = [2, 3]`: several assignments,
        // each possibly unpacking a tuple, comma-separated. The comma between
        // assignments must not be confused with a comma inside a literal on
        // either side, e.g. `set a = [1, 2], b = 3` — hence the depth-aware
        // split rather than a plain `rest.split(',')`.
        "set" | "with" => split_top_level(rest, ',')
            .into_iter()
            .flat_map(|clause| match clause.split_once('=') {
                Some((target, expression)) => target_names(target)
                    .into_iter()
                    .filter(|name| !mentions(expression, name))
                    .collect(),
                // The block form, `{% set content %}...{% endset %}`: no
                // right-hand side on this line to check against.
                None => target_names(clause),
            })
            .collect(),
        // `for k, v in expr [if ...] [recursive]`, target possibly `(k, v)`.
        "for" => rest
            .split_once(" in ")
            .map(|(targets, _)| target_names(targets))
            .unwrap_or_default(),
        // The name only; see the note above about parameters.
        "macro" => identifier(rest.split('(').next().unwrap_or("").trim())
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

/// Drop a trailing `with context` or `without context` from an import body.
fn strip_context(rest: &str) -> &str {
    for suffix in ["with context", "without context"] {
        if let Some(head) = rest.strip_suffix(suffix) {
            return head.trim_end();
        }
    }
    rest
}

/// Split `text` on `sep`, but not inside `()`, `[]` or `{}`.
///
/// `set a = [1, 2], b = 3` has one assignment-separating comma and one that
/// belongs to the list literal; only depth tracking tells them apart.
fn split_top_level(text: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            c if c == sep && depth == 0 => {
                out.push(text[start..i].trim());
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(text[start..].trim());
    out
}

/// The identifiers a `set`/`with`/`for` target names, tuple unpacking included.
///
/// `(a, b, (c,))` and `a, b` both yield `["a", "b", "c"]`: parentheses only
/// group, so stripping them and splitting on comma finds every leaf name
/// without needing to track nesting.
fn target_names(target: &str) -> Vec<String> {
    target
        .chars()
        .filter(|c| !matches!(c, '(' | ')' | '[' | ']'))
        .collect::<String>()
        .split(',')
        .filter_map(|token| identifier(token.trim()))
        .collect()
}

/// A token usable as a variable name, or `None`.
fn identifier(token: &str) -> Option<String> {
    let mut chars = token.chars();
    let first = chars.next()?;
    (first.is_alphabetic() || first == '_')
        .then(|| token.to_string())
        .filter(|_| chars.all(|c| c.is_alphanumeric() || c == '_'))
}

/// Whether an expression uses `name` as a whole word.
///
/// A substring match would let `{% set stack = stackless %}` pass as the
/// self-defaulting idiom, which it is not.
fn mentions(expression: &str, name: &str) -> bool {
    expression
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|token| token == name)
}

/// Two template files that can render to one path.
///
/// The renderer catches this, but only for the answer set it was given. Here it
/// is checked structurally: two paths whose conditional segments can both
/// collapse to the same literal will collide for *some* answer set, and finding
/// out which one is not the author's job.
fn check_path_collisions(entries: &[TreeEntry]) -> Vec<Finding> {
    use std::collections::BTreeMap;

    let mut degenerate: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in entries {
        if !entry.mode.is_blob() {
            continue;
        }
        let stripped = entry
            .path
            .strip_suffix(TEMPLATE_SUFFIX)
            .unwrap_or(&entry.path);
        let mut collapsed = Vec::new();
        let mut degenerates = false;
        for segment in stripped.split('/') {
            match literal_residue(segment) {
                Some(residue) if !residue.is_empty() => {
                    degenerates = true;
                    collapsed.push(residue);
                }
                Some(_) => {
                    // Renders empty, so the entry is skipped entirely and
                    // cannot collide with anything.
                    collapsed.clear();
                    degenerates = false;
                    break;
                }
                None => collapsed.push(segment.to_string()),
            }
        }
        if degenerates && !collapsed.is_empty() {
            degenerate
                .entry(collapsed.join("/"))
                .or_default()
                .push(entry.path.clone());
        }
    }

    degenerate
        .into_iter()
        .filter(|(_, sources)| sources.len() > 1)
        .map(|(rendered, sources)| Finding {
            severity: Severity::Error,
            code: "tpl::lint::collision",
            message: format!(
                "{} template paths all render to `{rendered}`: {}",
                sources.len(),
                sources.join(", ")
            ),
            help: "two template files cannot produce the same output file. This happens \
                   when several conditional segments collapse to the same literal suffix."
                .to_string(),
            path: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn codes(findings: &[Finding]) -> Vec<&str> {
        findings.iter().map(|f| f.code).collect()
    }

    // The exact shape that cost an afternoon: with one such file there is no
    // collision to catch it, and `.yaml` is written silently.
    #[test]
    fn a_conditional_leaving_its_suffix_outside_is_an_error() {
        let findings = check_path(".github/workflows/{% if msrv %}msrv{% endif %}.yaml");
        assert_eq!(codes(&findings), ["tpl::lint::degenerate_path"]);
        assert!(findings[0].message.contains("`.yaml`"));
        assert!(findings[0].help.contains("msrv.yaml"));
    }

    // The correct form, and the one a naive check would flag: for a `.jinja`
    // file the suffix is stripped before the segments render.
    #[rstest]
    #[case(".github/workflows/{% if msrv %}msrv.yaml{% endif %}")]
    #[case("{% if docs %}zensical.toml{% endif %}.jinja")]
    #[case("{% if docs %}docs{% endif %}/index.md.jinja")]
    #[case("plain/path/file.txt")]
    #[case("{{ name }}.rs.jinja")]
    fn a_correct_path_is_not_flagged(#[case] path: &str) {
        assert!(check_path(path).is_empty(), "{path} was flagged");
    }

    // An interpolation can render to anything, so the false branch is not
    // statically knowable and a guess would be a false positive.
    #[test]
    fn a_segment_with_an_interpolation_is_not_guessed_at() {
        assert!(check_path("{% if x %}a{% endif %}{{ suffix }}.yaml").is_empty());
    }

    #[test]
    fn two_segments_collapsing_to_one_name_collide() {
        let entries = [
            entry(".github/workflows/{% if msrv %}msrv{% endif %}.yaml"),
            entry(".github/workflows/{% if docs %}docs{% endif %}.yaml"),
        ];
        let findings = check_path_collisions(&entries);
        assert_eq!(codes(&findings), ["tpl::lint::collision"]);
        assert!(findings[0].message.contains(".github/workflows/.yaml"));
    }

    #[test]
    fn a_github_expression_outside_a_raw_block_is_reported() {
        let findings = check_foreign_expressions("ci.yaml.jinja", "runs-on: ${{ matrix.os }}\n");
        assert_eq!(codes(&findings), ["tpl::lint::foreign_expression"]);
    }

    #[test]
    fn a_github_expression_inside_a_raw_block_is_fine() {
        let findings = check_foreign_expressions(
            "ci.yaml.jinja",
            "{% raw %}runs-on: ${{ matrix.os }}{% endraw %}\n",
        );
        assert!(findings.is_empty());
    }

    // Whitespace control at the top of a file is normal, and a check that
    // missed it would pass by not looking.
    #[test]
    fn whitespace_control_still_counts_as_a_raw_block() {
        let findings = check_foreign_expressions(
            "ci.yaml.jinja",
            "{%- raw %}\nruns-on: ${{ matrix.os }}\n{%- endraw %}\n",
        );
        assert!(findings.is_empty());
    }

    // The real shape: a raw block, an interpolation, then another raw block.
    #[test]
    fn an_expression_after_a_closed_raw_block_is_reported() {
        let findings = check_foreign_expressions(
            "ci.yaml.jinja",
            "{% raw %}a: ${{ ok }}{% endraw %}\nb: {{ name }}\nc: ${{ leaked }}\n",
        );
        assert_eq!(codes(&findings), ["tpl::lint::foreign_expression"]);
        assert!(findings[0].message.contains("leaked"));
        assert!(
            !findings[0].message.contains("ok"),
            "the raw one was reported"
        );
    }

    // #81: trailing whitespace control (`-%}`) puts the `-` inside the
    // captured tag body. A scanner that only trims a leading `-` reads
    // `raw -` instead of `raw`, misses the tag, and reports every expression
    // in a block that in fact renders fine. This is the issue's own repro.
    #[test]
    fn trailing_whitespace_control_on_both_tags_is_a_raw_block() {
        let findings = check_foreign_expressions(
            "w.yaml.jinja",
            "{%- raw -%}\nx: ${{ github.ref }}\n{%- endraw %}\n",
        );
        assert!(findings.is_empty());
    }

    // Trailing dash on the opening tag only.
    #[test]
    fn trailing_whitespace_control_on_the_open_tag_is_a_raw_block() {
        let findings = check_foreign_expressions(
            "ci.yaml.jinja",
            "{% raw -%}\nruns-on: ${{ matrix.os }}\n{% endraw %}\n",
        );
        assert!(findings.is_empty());
    }

    // Trailing dash on the closing tag only.
    #[test]
    fn trailing_whitespace_control_on_the_close_tag_is_a_raw_block() {
        let findings = check_foreign_expressions(
            "ci.yaml.jinja",
            "{%- raw %}\nruns-on: ${{ matrix.os }}\n{%- endraw -%}\n",
        );
        assert!(findings.is_empty());
    }

    // With whitespace control on every tag, `endraw` still has to be
    // recognised as closing the block — otherwise `depth` never drops back
    // to zero and everything after is (wrongly) treated as still raw.
    #[test]
    fn an_expression_after_a_fully_trimmed_raw_block_is_reported() {
        let findings = check_foreign_expressions(
            "ci.yaml.jinja",
            "{%- raw -%}a: ${{ ok }}{%- endraw -%}\nb: {{ name }}\nc: ${{ leaked }}\n",
        );
        assert_eq!(codes(&findings), ["tpl::lint::foreign_expression"]);
        assert!(findings[0].message.contains("leaked"));
        assert!(
            !findings[0].message.contains("ok"),
            "the raw one was reported"
        );
    }

    // The escape idiom. A workflow that interpolates anything has to write this
    // on every line, so flagging it would make the check unusable on exactly
    // the files it exists for.
    #[rstest]
    #[case("cp target/${{ '{{' }} matrix.target {{ '}}' }}/release/x\n")]
    #[case(
        r#"url="${{ "{{" }} inputs.tag {{ "}}" }}"
"#
    )]
    fn the_escape_idiom_is_not_flagged(#[case] text: &str) {
        assert!(
            check_foreign_expressions("ci.yaml.jinja", text).is_empty(),
            "escaped expression was flagged: {text}"
        );
    }

    #[rstest]
    #[case("'{{'", true)]
    #[case("\"}}\"", true)]
    #[case("github.ref", false)]
    #[case("matrix.os", false)]
    #[case("", false)]
    #[case("'a' ~ b", false)]
    fn a_string_literal_body_is_the_escape_idiom(#[case] inner: &str, #[case] expected: bool) {
        assert_eq!(is_string_literal(inner), expected, "for {inner:?}");
    }

    #[test]
    fn a_syntax_error_is_reported_without_rendering() {
        let partials = crate::eval::no_partials();
        let findings = check_syntax("broken.jinja", "{% if x %}unterminated\n", partials);
        assert_eq!(codes(&findings), ["tpl::lint::syntax"]);
    }

    // The bug report itself: an import alias silently replaces a question, and
    // every comparison against it stops meaning anything.
    #[test]
    fn an_import_alias_matching_a_question_is_shadowed() {
        let manifest = manifest_with(
            "[questions.stack]\ntype = \"choice\"\nchoices = [\"a\", \"b\"]\ndefault = \"b\"\n",
        );
        let text = r#"{%- import "macros/m.jinja" as stack -%}"#;
        let findings = check_shadowed_names("a.txt.jinja", text, &manifest);
        assert_eq!(codes(&findings), ["tpl::lint::shadowed_name"]);
        assert!(findings[0].message.contains("stack"));
        assert!(findings[0].message.contains("a question"));
    }

    #[test]
    fn a_from_import_alias_matching_a_computed_value_is_shadowed() {
        let manifest = manifest_with("[computed]\nslug = \"'x'\"\n");
        let text = r#"{% from "m.jinja" import name as slug %}"#;
        let findings = check_shadowed_names("a.jinja", text, &manifest);
        assert_eq!(codes(&findings), ["tpl::lint::shadowed_name"]);
        assert!(findings[0].message.contains("a computed value"));
    }

    #[test]
    fn an_alias_that_names_nothing_declared_is_not_flagged() {
        let manifest = manifest_with(
            "[questions.stack]\ntype = \"choice\"\nchoices = [\"a\", \"b\"]\ndefault = \"b\"\n",
        );
        let text = r#"{% import "macros/m.jinja" as helpers %}"#;
        assert!(check_shadowed_names("a.jinja", text, &manifest).is_empty());
    }

    // A macro's own parameters are meant to be used as local names inside its
    // body; that is what a parameter is for, not a collision.
    #[test]
    fn a_macro_parameter_matching_a_question_is_not_flagged() {
        let manifest = manifest_with("[questions.name]\ntype = \"string\"\ndefault = \"x\"\n");
        let text = "{% macro badge(name) %}{{ name }}{% endmacro %}";
        assert!(check_shadowed_names("a.jinja", text, &manifest).is_empty());
    }

    // The macro's own name is still a binding in the file's namespace, and can
    // still collide.
    #[test]
    fn a_macro_name_matching_a_question_is_shadowed() {
        let manifest = manifest_with("[questions.badge]\ntype = \"string\"\ndefault = \"x\"\n");
        let text = "{% macro badge() %}{% endmacro %}";
        assert_eq!(
            codes(&check_shadowed_names("a.jinja", text, &manifest)),
            ["tpl::lint::shadowed_name"]
        );
    }

    // The self-defaulting idiom: a target that reuses its own name on the
    // right-hand side is deliberate, and flagging it would make the rule
    // unusable on the templates that rely on it.
    #[test]
    fn a_set_that_defaults_from_itself_is_not_flagged() {
        let manifest = manifest_with("[questions.stack]\ntype = \"string\"\ndefault = \"x\"\n");
        let text = "{% set stack = stack | default('x') %}";
        assert!(check_shadowed_names("a.jinja", text, &manifest).is_empty());
    }

    #[test]
    fn a_set_that_does_not_reuse_its_own_name_is_flagged() {
        let manifest = manifest_with("[questions.stack]\ntype = \"string\"\ndefault = \"x\"\n");
        let text = "{% set stack = 'b' %}";
        assert_eq!(
            codes(&check_shadowed_names("a.jinja", text, &manifest)),
            ["tpl::lint::shadowed_name"]
        );
    }

    #[test]
    fn a_for_loop_target_matching_a_question_is_shadowed() {
        let manifest = manifest_with("[questions.stack]\ntype = \"string\"\ndefault = \"x\"\n");
        let text = "{% for stack in items %}{{ stack }}{% endfor %}";
        assert_eq!(
            codes(&check_shadowed_names("a.jinja", text, &manifest)),
            ["tpl::lint::shadowed_name"]
        );
    }

    // Bound inside a raw block, which is emitted verbatim: it binds nothing.
    #[test]
    fn a_binding_inside_a_raw_block_is_not_flagged() {
        let manifest = manifest_with(
            "[questions.stack]\ntype = \"choice\"\nchoices = [\"a\", \"b\"]\ndefault = \"b\"\n",
        );
        let text = r#"{% raw %}{% import "m.jinja" as stack %}{% endraw %}"#;
        assert!(check_shadowed_names("a.jinja", text, &manifest).is_empty());
    }

    // A name bound twice is one mistake; the second report would say nothing
    // the first did not.
    #[test]
    fn the_same_name_shadowed_twice_is_reported_once() {
        let manifest = manifest_with("[questions.stack]\ntype = \"string\"\ndefault = \"x\"\n");
        let text = "{% set stack = 'a' %}{% set stack = 'b' %}";
        assert_eq!(
            codes(&check_shadowed_names("a.jinja", text, &manifest)),
            ["tpl::lint::shadowed_name"]
        );
    }

    #[rstest]
    // A plain alias.
    #[case("import \"m.jinja\" as stack", &["stack"])]
    // `with context` is not part of the alias.
    #[case("import \"m.jinja\" as stack with context", &["stack"])]
    #[case("from \"m.jinja\" import a, b as c", &["a", "c"])]
    // Multiple assignments in one `with`, comma-separated.
    #[case("with a = foo, b = bar", &["a", "b"])]
    // A list literal's comma must not be read as a second assignment.
    #[case("set a = [1, 2], b = 3", &["a", "b"])]
    // Tuple unpacking.
    #[case("with (a, b) = pair", &["a", "b"])]
    #[case("for k, v in items.items()", &["k", "v"])]
    // The block form: no right-hand side on this line at all.
    #[case("set content", &["content"])]
    #[case("macro badge(name)", &["badge"])]
    // A macro's parameters are not bindings this rule reports.
    #[case("macro badge(name, size)", &["badge"])]
    // Assigning through a namespace binds no root name.
    #[case("set ns.x = 1", &[] as &[&str])]
    fn bound_names_reads_every_construct(#[case] tag: &str, #[case] expected: &[&str]) {
        assert_eq!(bound_names(tag), expected);
    }

    #[rstest]
    #[case("set x = x | default('a')", &[] as &[&str])]
    // Only the reused target is excluded; an unrelated second target is not.
    #[case("with x = x, y = 1", &["y"])]
    fn a_target_reusing_its_own_name_is_excluded(#[case] tag: &str, #[case] expected: &[&str]) {
        assert_eq!(bound_names(tag), expected);
    }

    #[test]
    fn tag_bodies_skips_a_raw_block_and_finds_what_follows() {
        let text = "{% raw %}{% import \"m.jinja\" as x %}{% endraw %}{% set y = 1 %}";
        assert_eq!(tag_bodies(text), vec!["set y = 1"]);
    }

    fn entry(path: &str) -> TreeEntry {
        TreeEntry {
            path: path.to_string(),
            oid: crate::git::Oid::from_bytes([0; 20]),
            mode: crate::git::FileMode::Blob,
        }
    }

    fn levels(deny: &[&str], allow: &[&str]) -> Result<Levels, LintError> {
        let deny: Vec<String> = deny.iter().map(|s| s.to_string()).collect();
        let allow: Vec<String> = allow.iter().map(|s| s.to_string()).collect();
        Levels::parse(&deny, &allow)
    }

    /// A manifest with just enough in it to reach one rule.
    fn manifest_with(body: &str) -> Manifest {
        Manifest::parse(&format!("name = \"x\"\n{body}"), "template.toml").expect("valid manifest")
    }

    /// The repository tree, not the render root: a note is read from the
    /// template and never rendered into the project.
    #[rstest]
    #[case("NEXT-STEPS.md", "NEXT-STEPS.md")]
    #[case("docs/NEXT.md", "docs/NEXT.md")]
    // `.jinja` is how an author asks for a rendered note, so it is matched
    // literally rather than stripped.
    #[case("NEXT.md.jinja", "NEXT.md.jinja")]
    // Allowed, and pointless but harmless: the file is also rendered into the
    // project. Policing it would cost more to explain than it saves.
    #[case("template/NEXT.md", "template/NEXT.md")]
    fn a_note_file_the_template_contains_is_not_flagged(
        #[case] declared: &str,
        #[case] present: &str,
    ) {
        let manifest = manifest_with(&format!("note_file = \"{declared}\""));
        assert!(check_note_file(&manifest, &[entry(present)]).is_empty());
    }

    /// The mistake this rule exists for: `note_file` is repository-root
    /// relative, so an author who writes the render-root path gets nothing.
    #[test]
    fn a_note_file_naming_nothing_is_an_error() {
        let manifest = manifest_with("note_file = \"NEXT-STEPS.md\"");
        let findings = check_note_file(&manifest, &[entry("template/NEXT-STEPS.md")]);

        assert_eq!(codes(&findings), ["tpl::lint::missing_note_file"]);
        assert!(findings[0].message.contains("NEXT-STEPS.md"));
        // The help has to name the trap, or the finding restates the message.
        assert!(
            findings[0].help.contains("repository root"),
            "{:?}",
            findings[0].help
        );
    }

    /// A `.jinja` note is a different file from the same path without it, and
    /// treating one as the other would make them indistinguishable.
    #[test]
    fn a_jinja_note_file_is_not_satisfied_by_the_stripped_path() {
        let manifest = manifest_with("note_file = \"NEXT.md.jinja\"");
        assert_eq!(
            codes(&check_note_file(&manifest, &[entry("NEXT.md")])),
            ["tpl::lint::missing_note_file"]
        );
    }

    /// A lint has no answers, so a path that depends on them cannot be
    /// resolved. Skipped rather than guessed — a false error here would make
    /// the rule unusable on exactly the templates that need it.
    #[rstest]
    #[case("notes/{{ language }}.md")]
    #[case("{% if ci %}notes/ci.md{% endif %}")]
    fn a_note_file_path_that_is_an_expression_is_not_checked(#[case] declared: &str) {
        let manifest = manifest_with(&format!("note_file = \"{declared}\""));
        assert!(check_note_file(&manifest, &[]).is_empty());
    }

    #[test]
    fn a_template_without_a_note_file_is_not_flagged() {
        assert!(check_note_file(&manifest_with(""), &[]).is_empty());
        // An inline `note` names no path and so has nothing to check.
        assert!(check_note_file(&manifest_with("note = \"hi\""), &[]).is_empty());
    }

    /// A tree entry is not necessarily a blob, and a directory named
    /// `NEXT-STEPS.md` is not a note.
    #[test]
    fn a_directory_does_not_satisfy_a_note_file() {
        let manifest = manifest_with("note_file = \"NEXT-STEPS.md\"");
        let tree = TreeEntry {
            path: "NEXT-STEPS.md".to_string(),
            oid: crate::git::Oid::from_bytes([0; 20]),
            mode: crate::git::FileMode::Tree,
        };
        assert_eq!(
            codes(&check_note_file(&manifest, &[tree])),
            ["tpl::lint::missing_note_file"]
        );
    }

    /// The two shapes from the field report. Both are valid TOML, both load,
    /// and both do something other than what the author wrote — the note never
    /// appears, or a remote called `note_file` is added at `init`.
    #[rstest]
    #[case("[computed]\nx = \"{{ 'y' }}\"\nnote_file = \"NOTE.md\"", "computed")]
    #[case(
        "[remotes]\norigin = \"git@example.com:a/b.git\"\nnote_file = \"NOTE.md\"",
        "remotes"
    )]
    #[case(
        "[data.things]\nsource = \"data/things.toml\"\nnote_file = \"NOTE.md\"",
        "data.things"
    )]
    #[case("[questions.a]\ntype = \"string\"\nnote = \"hi\"", "questions.a")]
    fn a_top_level_key_written_after_a_table_is_absorbed(#[case] body: &str, #[case] table: &str) {
        let findings = check_absorbed_keys(&format!("name = \"x\"\n{body}\n"));

        assert_eq!(codes(&findings), ["tpl::lint::absorbed_key"]);
        assert!(
            findings[0].message.contains(&format!("[{table}]")),
            "{:?}",
            findings[0].message
        );
        // The help must name the destination, or the author cannot tell which
        // of several tables above the line swallowed it.
        assert!(
            findings[0].help.contains(&format!("{table}.")),
            "{:?}",
            findings[0].help
        );
    }

    /// The keys are the schema's, not a second list: a `data` source or a
    /// question named after any of them absorbs just the same.
    #[rstest]
    #[case("name")]
    #[case("description")]
    #[case("root")]
    #[case("note")]
    #[case("note_file")]
    fn every_top_level_key_is_reported_when_absorbed(#[case] key: &str) {
        let findings = check_absorbed_keys(&format!("name = \"x\"\n[computed]\n{key} = \"v\"\n"));
        assert_eq!(codes(&findings), ["tpl::lint::absorbed_key"]);
    }

    /// The whole point of the rule: written above the first table header, the
    /// key means what it says.
    #[test]
    fn a_top_level_key_at_the_top_level_is_not_flagged() {
        let text = "name = \"x\"\nnote_file = \"NOTE.md\"\nstrict = true\n\n\
                    [computed]\ny = \"{{ 1 }}\"\n";
        assert!(check_absorbed_keys(text).is_empty());
    }

    /// A table value is not an absorbed key. `[data.note_file]` is a data
    /// source that happens to be called `note_file`, and refusing it would
    /// make the rule police names it has no business policing.
    #[rstest]
    #[case("[data.note_file]\nsource = \"data/n.toml\"")]
    #[case("[questions.name]\ntype = \"string\"")]
    fn a_table_named_after_a_top_level_key_is_not_flagged(#[case] body: &str) {
        assert!(check_absorbed_keys(&format!("name = \"x\"\n{body}\n")).is_empty());
    }

    /// An array of tables opens a table too, so a key after `[[...]]` is
    /// absorbed by that element rather than by the manifest.
    #[test]
    fn an_array_of_tables_absorbs_a_key_as_well() {
        let text = "name = \"x\"\n\n[[questions.a.choices]]\nvalue = \"v\"\nnote = \"hi\"\n";
        assert_eq!(
            codes(&check_absorbed_keys(text)),
            ["tpl::lint::absorbed_key"]
        );
    }

    /// Reported once per occurrence, in document order, so a manifest with two
    /// mistakes does not have to be fixed twice to see the second.
    #[test]
    fn every_absorbed_key_is_reported() {
        let text = "name = \"x\"\n\n[computed]\nnote_file = \"a\"\nroot = \"b\"\n";
        assert_eq!(
            codes(&check_absorbed_keys(text)),
            ["tpl::lint::absorbed_key", "tpl::lint::absorbed_key"]
        );
    }

    fn sample() -> Vec<Finding> {
        vec![
            Finding::error("tpl::lint::syntax", "a.jinja", "boom".into(), "fix".into()),
            Finding::warning(
                "tpl::lint::foreign_expression",
                "b.jinja",
                "boom".into(),
                "fix".into(),
            ),
            Finding::warning(
                "tpl::lint::undeclared",
                "c.jinja",
                "boom".into(),
                "fix".into(),
            ),
        ]
    }

    fn denied(verdicts: &[Verdict]) -> Vec<&str> {
        verdicts
            .iter()
            .filter(|v| v.denied)
            .map(|v| v.finding.code)
            .collect()
    }

    fn reported(verdicts: &[Verdict]) -> Vec<&str> {
        verdicts.iter().map(|v| v.finding.code).collect()
    }

    #[test]
    fn no_levels_deny_nothing() {
        let verdicts = levels(&[], &[]).unwrap().apply(sample());
        assert_eq!(reported(&verdicts).len(), 3);
        assert!(denied(&verdicts).is_empty());
    }

    #[test]
    fn denying_warnings_promotes_every_warning_but_not_the_errors() {
        let verdicts = levels(&["warnings"], &[]).unwrap().apply(sample());
        assert_eq!(
            denied(&verdicts),
            ["tpl::lint::foreign_expression", "tpl::lint::undeclared"]
        );
    }

    #[test]
    fn denying_one_code_leaves_the_other_warnings_alone() {
        let verdicts = levels(&["tpl::lint::foreign_expression"], &[])
            .unwrap()
            .apply(sample());
        assert_eq!(denied(&verdicts), ["tpl::lint::foreign_expression"]);
    }

    #[test]
    fn an_allowed_finding_is_not_reported_at_all() {
        let verdicts = levels(&[], &["tpl::lint::undeclared"])
            .unwrap()
            .apply(sample());
        assert_eq!(
            reported(&verdicts),
            ["tpl::lint::syntax", "tpl::lint::foreign_expression"]
        );
    }

    #[test]
    fn allowing_an_error_silences_it_too() {
        let verdicts = levels(&[], &["tpl::lint::syntax"]).unwrap().apply(sample());
        assert!(!reported(&verdicts).contains(&"tpl::lint::syntax"));
    }

    #[test]
    fn denying_a_code_that_is_already_an_error_changes_nothing() {
        let verdicts = levels(&["tpl::lint::syntax"], &[]).unwrap().apply(sample());
        // Already fatal; counting it as denied would double-count it.
        assert!(denied(&verdicts).is_empty());
        assert_eq!(reported(&verdicts).len(), 3);
    }

    // The composition the flag exists for: everything fatal except the one
    // code a template is still migrating away from.
    #[test]
    fn a_named_allow_beats_the_warnings_bucket() {
        let verdicts = levels(&["warnings"], &["tpl::lint::undeclared"])
            .unwrap()
            .apply(sample());
        assert_eq!(denied(&verdicts), ["tpl::lint::foreign_expression"]);
        assert!(!reported(&verdicts).contains(&"tpl::lint::undeclared"));
    }

    // Precedence is by specificity, so the levels a pair of argument lists
    // reach cannot depend on how a CI fragment happened to order them.
    #[test]
    fn the_levels_do_not_depend_on_the_order_of_the_values() {
        let one = levels(
            &["warnings", "tpl::lint::syntax"],
            &["tpl::lint::undeclared", "tpl::lint::collision"],
        )
        .unwrap();
        let other = levels(
            &["tpl::lint::syntax", "warnings"],
            &["tpl::lint::collision", "tpl::lint::undeclared"],
        )
        .unwrap();
        assert_eq!(one, other);
    }

    #[test]
    fn a_named_deny_beats_allow_warnings() {
        let verdicts = levels(&["tpl::lint::undeclared"], &["warnings"])
            .unwrap()
            .apply(sample());
        assert_eq!(
            reported(&verdicts),
            ["tpl::lint::syntax", "tpl::lint::undeclared"]
        );
        assert_eq!(denied(&verdicts), ["tpl::lint::undeclared"]);
    }

    #[test]
    fn an_unknown_code_is_rejected_with_the_valid_ones() {
        // Assembled rather than written out: `tests/diagnostics.rs` harvests
        // bare `"tpl::…"` literals from `src/`, and a fake one would be read
        // as an undocumented code.
        let unknown = format!("tpl::lint::{}", "nope");
        let error = levels(&[&unknown], &[]).unwrap_err();
        let LintError::UnknownCode { spelling, help } = error else {
            panic!("expected an unknown code, got {error:?}");
        };
        assert_eq!(spelling, unknown);
        assert!(help.contains("undeclared"), "{help}");
        assert!(help.contains(WARNINGS), "{help}");
    }

    #[test]
    fn a_near_miss_is_suggested() {
        let typo = "tpl::lint::undeclared".strip_suffix('d').expect("a typo");
        let error = levels(&[], &[typo]).unwrap_err();
        let LintError::UnknownCode { help, .. } = error else {
            panic!("expected an unknown code");
        };
        assert!(
            help.starts_with("Did you mean `tpl::lint::undeclared`?"),
            "{help}"
        );
    }

    #[test]
    fn denying_and_allowing_the_same_code_is_rejected() {
        let error = levels(&["tpl::lint::undeclared"], &["tpl::lint::undeclared"]).unwrap_err();
        assert!(matches!(error, LintError::ConflictingLevel { .. }));
    }

    #[test]
    fn denying_and_allowing_warnings_wholesale_is_rejected() {
        let error = levels(&["warnings"], &["warnings"]).unwrap_err();
        assert!(matches!(error, LintError::ConflictingLevel { .. }));
    }

    // The list `--deny` validates against has to be the list the rules use, or
    // a code becomes undeniable the day it is added.
    #[test]
    fn every_rule_code_is_denyable() {
        let mut sorted = CODES.to_vec();
        sorted.sort_unstable();
        assert_eq!(CODES, sorted.as_slice(), "CODES must stay sorted");
        for code in CODES {
            assert!(levels(&[code], &[]).is_ok(), "{code} is not accepted");
        }
    }
}
