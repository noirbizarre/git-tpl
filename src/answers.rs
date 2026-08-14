//! Answers read from a file, for `--answers-from`.
//!
//! One flag rather than a tool-specific importer: the same file supplies a
//! migration from another generator, a set of house values shared by many
//! projects, a non-interactive render in CI, and a template's own test
//! fixtures. A `--from-copier` would have bought only the first, and would have
//! put someone else's file format in the CLI surface permanently.
//!
//! Names map to names. There is no renaming and no mapping language: a template
//! whose questions were renamed between tools needs its answers edited.

use std::collections::BTreeMap;
use std::path::Path;

use miette::Diagnostic;
use thiserror::Error;

use crate::data::format::{self, Format};
use crate::template::Value;

/// The key whose table, when present, holds the answers.
const ANSWERS_KEY: &str = "answers";

/// Errors from reading an answers file.
#[derive(Debug, Error, Diagnostic)]
pub enum AnswersError {
    /// The file could not be read.
    #[error("could not read the answers file")]
    #[diagnostic(code(tpl::answers::read), help("path:   {path}\nreason: {reason}"))]
    Read {
        /// The path as it was given on the command line.
        path: String,
        /// Why the read failed.
        reason: String,
    },

    /// The file is not valid in its format.
    #[error("could not parse the answers file")]
    #[diagnostic(
        code(tpl::answers::parse),
        help("path:   {path}\nformat: {format}\nreason: {reason}")
    )]
    Parse {
        /// The path as it was given on the command line.
        path: String,
        /// The format it was read as, inferred from the extension.
        format: String,
        /// Why the parse failed.
        reason: String,
    },

    /// The document is not a table of answers.
    #[error("the answers file is not a table of answers")]
    #[diagnostic(
        code(tpl::answers::shape),
        help(
            "path: {path}\nexpected either a table of `name = value` pairs, or a table under an `answers` key"
        )
    )]
    Shape {
        /// The path as it was given on the command line.
        path: String,
    },
}

/// Read answers from a TOML, JSON or YAML file.
///
/// The path is used as given, relative to the working directory, and is
/// deliberately *not* checked for containment the way a local data source's
/// path is (`data::within`). The difference is who chose it: a data source path
/// comes out of a template repository, which is untrusted input, and this one
/// was typed by the person running the command.
///
/// Keys naming no question are not rejected here — this layer has never seen
/// the manifest. They are reported by the caller once the template is resolved,
/// because a file carried over from another tool legitimately carries
/// `_src_path` and questions the template has since dropped.
pub fn load(path: &Path) -> Result<BTreeMap<String, Value>, AnswersError> {
    let shown = path.display().to_string();

    let bytes = std::fs::read(path).map_err(|e| AnswersError::Read {
        path: shown.clone(),
        reason: e.to_string(),
    })?;

    // From the extension alone, and unknown extensions are TOML — the same rule
    // data sources use. Answers files are named by the user, so a `.yml` file
    // read as YAML and a `.answers` file read as TOML are both what was asked
    // for.
    let format = Format::infer(&shown);

    let value = format::parse_value(format, &bytes).map_err(|reason| AnswersError::Parse {
        path: shown.clone(),
        format: format!("{format:?}").to_lowercase(),
        reason,
    })?;

    let Value::Table(table) = value else {
        return Err(AnswersError::Shape { path: shown });
    };

    // Two accepted shapes and no more. Flat is what `.copier-answers.yml` and a
    // hand-written defaults file look like; the nested form exists so a
    // template's test fixture can carry `[expect]` alongside its `[answers]`
    // without the two colliding.
    match table.get(ANSWERS_KEY) {
        Some(Value::Table(answers)) => Ok(answers.clone()),
        _ => Ok(table),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write `content` to a uniquely named temporary file and read it back.
    fn read(name: &str, content: &str) -> Result<BTreeMap<String, Value>, AnswersError> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, content).unwrap();
        load(&path)
    }

    #[test]
    fn a_flat_table_is_read_as_answers() {
        let answers = read("a.toml", "project_name = \"thing\"\nlicense = \"MIT\"\n").unwrap();

        assert_eq!(answers["project_name"], Value::String("thing".into()));
        assert_eq!(answers["license"], Value::String("MIT".into()));
    }

    /// The nested form is what a template's own fixtures use, where `[answers]`
    /// sits next to other tables.
    #[test]
    fn a_top_level_answers_table_wins_over_the_flat_form() {
        let answers = read(
            "a.toml",
            "other = \"ignored\"\n\n[answers]\nproject_name = \"thing\"\n",
        )
        .unwrap();

        assert_eq!(answers["project_name"], Value::String("thing".into()));
        assert!(!answers.contains_key("other"), "{answers:?}");
    }

    /// A key that happens to be called `answers` but is not a table is an
    /// answer like any other, not a broken nested form.
    #[test]
    fn a_scalar_answers_key_is_an_answer_not_a_wrapper() {
        let answers = read("a.toml", "answers = \"42\"\n").unwrap();

        assert_eq!(answers["answers"], Value::String("42".into()));
    }

    /// The whole reason the file form exists alongside `--answer`: a flag can
    /// only carry text, a file carries types.
    #[test]
    fn types_are_preserved_from_json() {
        let answers = read("a.json", r#"{"port": 8080, "ci": true, "tags": ["a"]}"#).unwrap();

        assert_eq!(answers["port"], Value::Integer(8080));
        assert_eq!(answers["ci"], Value::Bool(true));
        assert_eq!(
            answers["tags"],
            Value::Array(vec![Value::String("a".into())])
        );
    }

    #[test]
    fn types_are_preserved_from_yaml() {
        let answers = read("a.yaml", "port: 8080\nci: true\nname: no\n").unwrap();

        assert_eq!(answers["port"], Value::Integer(8080));
        assert_eq!(answers["ci"], Value::Bool(true));
        // YAML 1.2: `no` is a string. See `docs/data/index.md#about-yaml`.
        assert_eq!(answers["name"], Value::String("no".into()));
    }

    #[test]
    fn the_format_comes_from_the_extension() {
        assert!(read("a.yml", "name: thing\n").is_ok());
        assert!(read("a.json", r#"{"name": "thing"}"#).is_ok());
        // No extension, so TOML — the same default data sources use.
        assert!(read("answers", "name = \"thing\"\n").is_ok());
    }

    #[test]
    fn a_document_that_is_not_a_table_is_refused() {
        let error = read("a.json", "[1, 2, 3]").unwrap_err();

        assert!(matches!(error, AnswersError::Shape { .. }), "{error:?}");
    }

    #[test]
    fn a_malformed_document_names_the_format() {
        let error = read("a.json", "{not json").unwrap_err();

        match error {
            AnswersError::Parse { format, .. } => assert_eq!(format, "json"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_missing_file_names_the_path() {
        let error = load(Path::new("/nonexistent/answers.toml")).unwrap_err();

        match error {
            AnswersError::Read { path, .. } => assert_eq!(path, "/nonexistent/answers.toml"),
            other => panic!("{other:?}"),
        }
    }
}
