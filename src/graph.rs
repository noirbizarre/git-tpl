//! The dependency graph over questions, computed values and data sources.
//!
//! Every expression in a manifest is parsed before anything is prompted, the
//! names it references are extracted, and the resulting graph is topologically
//! sorted. Cycles and unknown references are errors *before the first prompt* —
//! answering six questions and then being told the seventh is unresolvable is
//! the worst possible moment to find out.
//!
//! See `docs/adr/007-static-dependency-graph.md`.

use std::collections::{BTreeMap, BTreeSet};

use miette::Diagnostic;
use thiserror::Error;

use crate::context::{Name, Namespace};
use crate::template::{Manifest, is_expression};

/// Errors from building or validating the graph.
#[derive(Debug, Error, Diagnostic)]
pub enum GraphError {
    /// An expression could not be parsed.
    #[error("invalid expression in `{location}`")]
    #[diagnostic(
        code(tpl::graph::invalid_expression),
        help("expression: {expression}\nreason:     {reason}")
    )]
    InvalidExpression {
        /// Which declaration it came from.
        location: String,
        /// The expression itself.
        expression: String,
        /// The parser's message.
        reason: String,
    },

    /// An expression references something that does not exist.
    #[error("unknown reference `{unknown}` in `{location}`")]
    #[diagnostic(
        code(tpl::graph::unknown_reference),
        help("in: {expression}\n{}", match suggestion {
            Some(s) => format!("did you mean `{s}`?"),
            None => "`{unknown}` is not a question, a computed value or a data source in this template".to_string(),
        })
    )]
    UnknownReference {
        /// Which declaration referenced it.
        location: String,
        /// The name that does not exist.
        unknown: String,
        /// The expression it appeared in.
        expression: String,
        /// The closest known name, if there is one.
        suggestion: Option<String>,
    },

    /// The graph contains a cycle.
    #[error("cyclic dependency: {}", path.join(" → "))]
    #[diagnostic(
        code(tpl::graph::cycle),
        help(
            "a question's `when`, `default` or `choices_from` may only reference values resolved before it"
        )
    )]
    Cycle {
        /// The cycle, as a readable chain.
        path: Vec<String>,
    },
}

/// What kind of thing a node resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A question to ask.
    Question,
    /// A value to compute.
    Computed,
    /// A data source to load.
    Data,
}

/// One node in the resolution order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// What it is.
    pub kind: NodeKind,
    /// Its name within its namespace.
    pub key: String,
}

impl Node {
    fn name(&self) -> Name {
        match self.kind {
            NodeKind::Question => Name::answer(&self.key),
            NodeKind::Computed => Name::computed(&self.key),
            NodeKind::Data => Name::data(&self.key),
        }
    }
}

/// A validated, topologically ordered resolution plan.
#[derive(Debug, Clone)]
pub struct Graph {
    order: Vec<Node>,
}

impl Graph {
    /// The nodes, in the order they must be resolved.
    pub fn order(&self) -> &[Node] {
        &self.order
    }

    /// Build and validate the graph for a manifest.
    pub fn build(manifest: &Manifest) -> Result<Self, GraphError> {
        let mut nodes: Vec<Node> = Vec::new();
        let mut edges: BTreeMap<Name, BTreeSet<Name>> = BTreeMap::new();

        // Declaration order is preserved here and used to break ties in the
        // sort below, so it remains the one ordering a template author
        // controls.
        for name in manifest.data.keys() {
            nodes.push(Node {
                kind: NodeKind::Data,
                key: name.clone(),
            });
        }
        for name in manifest.questions.keys() {
            nodes.push(Node {
                kind: NodeKind::Question,
                key: name.clone(),
            });
        }
        for name in manifest.computed.keys() {
            nodes.push(Node {
                kind: NodeKind::Computed,
                key: name.clone(),
            });
        }

        let known: BTreeSet<Name> = nodes.iter().map(Node::name).collect();

        // A data source's `source` may itself be an expression, so a source
        // can depend on an answer. That edge is what makes
        // `data/frameworks/{{ project_type }}.toml` load after `project_type`
        // is known rather than before.
        for (key, decl) in &manifest.data {
            let deps = references(&decl.source, &format!("data.{key}"), &known)?;
            edges.entry(Name::data(key)).or_default().extend(deps);
        }

        for (key, question) in &manifest.questions {
            let node = Name::answer(key);
            let mut deps = edges.entry(node.clone()).or_default().clone();

            if let Some(when) = &question.when {
                deps.extend(references(when, &format!("questions.{key}.when"), &known)?);
            }
            if let Some(default) = question.default_expression() {
                deps.extend(references(
                    default,
                    &format!("questions.{key}.default"),
                    &known,
                )?);
            }
            if let Some(from) = &question.choices_from {
                // `choices_from` is a path, not an expression: `data.licenses.ids`
                // depends on `data.licenses`. Treating it as an expression would
                // miss the dependency entirely, and the data would be loaded
                // after the question that needs it.
                deps.extend(path_reference(
                    from,
                    &format!("questions.{key}.choices_from"),
                    &known,
                )?);
            }

            edges.insert(node, deps);
        }

        for (key, expression) in &manifest.computed {
            let node = Name::computed(key);
            let deps = references(expression, &format!("computed.{key}"), &known)?;
            edges.entry(node).or_default().extend(deps);
        }

        let order = toposort(&nodes, &edges)?;
        Ok(Self { order })
    }
}

/// Extract the names an expression references, rejecting unknown ones.
fn references(
    expression: &str,
    location: &str,
    known: &BTreeSet<Name>,
) -> Result<BTreeSet<Name>, GraphError> {
    if !is_expression(expression) {
        return Ok(BTreeSet::new());
    }

    // The same environment used to evaluate, so an expression that parses here
    // is one that will run there. Without partials, deliberately: this analysis
    // is a parse, and `undeclared_variables` never follows an `{% import %}`.
    // A missing partial is a render-time failure with a render-time diagnostic,
    // not a graph edge.
    let env = crate::eval::environment(crate::eval::no_partials());
    let template =
        env.template_from_str(expression)
            .map_err(|error| GraphError::InvalidExpression {
                location: location.to_string(),
                expression: expression.to_string(),
                reason: error.to_string(),
            })?;

    // `nested: true` yields dotted paths, so `data.licenses.ids` arrives whole
    // and can be resolved to the `data.licenses` node rather than to a
    // non-existent top-level `data`.
    let mut out = BTreeSet::new();
    for reference in template.undeclared_variables(true) {
        if let Some(name) = resolve_reference(&reference, known) {
            out.insert(name);
        } else if let Some(root) = unknown_root(&reference, known) {
            return Err(GraphError::UnknownReference {
                location: location.to_string(),
                unknown: root.clone(),
                expression: expression.to_string(),
                suggestion: closest(&root, known),
            });
        }
    }
    Ok(out)
}

/// Resolve a `choices_from` path to the node it depends on.
fn path_reference(
    path: &str,
    location: &str,
    known: &BTreeSet<Name>,
) -> Result<BTreeSet<Name>, GraphError> {
    match resolve_reference(path, known) {
        Some(name) => Ok(BTreeSet::from([name])),
        None => Err(GraphError::UnknownReference {
            location: location.to_string(),
            // Report the *declaration* that is missing rather than the whole
            // path. `data.licenses.ids` is unresolvable because `[data.licenses]`
            // was never declared, and that is what the user has to go and add —
            // naming the full path would leave them wondering whether the
            // problem was the `ids` part.
            unknown: unknown_root(path, known).unwrap_or_else(|| path.to_string()),
            expression: path.to_string(),
            suggestion: closest(path, known),
        }),
    }
}

/// Map a dotted reference onto a graph node, if it names one.
fn resolve_reference(reference: &str, known: &BTreeSet<Name>) -> Option<Name> {
    let mut parts = reference.split('.');
    let head = parts.next()?;

    match head {
        "data" => {
            let key = parts.next()?;
            let name = Name::data(key);
            known.contains(&name).then_some(name)
        }
        // Template metadata is seeded before anything is resolved, so it
        // creates no edges. Ignoring it here is what keeps `{{ template.name }}`
        // from looking like an unknown reference.
        "template" => None,
        other => {
            for candidate in [Name::answer(other), Name::computed(other)] {
                if known.contains(&candidate) {
                    return Some(candidate);
                }
            }
            None
        }
    }
}

/// The name to report when a reference resolves to nothing.
///
/// Returns `None` for things that are legitimately not graph nodes — loop
/// variables, `template.*`, and MiniJinja's own globals — so that
/// `{% for x in xs %}{{ x }}{% endfor %}` is not reported as an error.
fn unknown_root(reference: &str, known: &BTreeSet<Name>) -> Option<String> {
    let head = reference.split('.').next()?;

    if head == "template" {
        return None;
    }

    if head == "data" {
        // `data.something` where `something` is not declared is a real error:
        // the template asked for a source it never declared.
        return Some(reference.split('.').take(2).collect::<Vec<_>>().join("."));
    }

    // MiniJinja reports loop variables and block-scoped names as undeclared at
    // the point they are used. They are not references to the context, and
    // rejecting them would make `{% for %}` unusable in a default.
    if is_minijinja_builtin(head) {
        return None;
    }

    if known.iter().any(|n| n.key == head) {
        return None;
    }

    Some(head.to_string())
}

/// Names MiniJinja provides that are not context references.
fn is_minijinja_builtin(name: &str) -> bool {
    matches!(
        name,
        "loop" | "self" | "range" | "dict" | "namespace" | "debug" | "true" | "false" | "none"
    )
}

/// The closest known name, for a "did you mean?" suggestion.
///
/// A plain prefix/edit-distance heuristic rather than a dependency: the whole
/// job is to turn a typo into a pointer, and a near-miss suggestion is no worse
/// than none.
fn closest(unknown: &str, known: &BTreeSet<Name>) -> Option<String> {
    let unknown_lower = unknown.to_lowercase();
    known
        .iter()
        .filter(|name| name.namespace != Namespace::Data)
        .map(|name| {
            (
                edit_distance(&unknown_lower, &name.key.to_lowercase()),
                &name.key,
            )
        })
        // A suggestion further away than a third of the name's length is not a
        // typo, it is a different word, and offering it is worse than silence.
        .filter(|(distance, key)| *distance <= (key.len() / 3).max(1))
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, key)| key.clone())
}

/// Levenshtein distance.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];

    for (i, ca) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            current[j + 1] = (previous[j] + cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

/// Topologically sort, breaking ties by declaration order.
///
/// A depth-first sort rather than Kahn's algorithm because the recursion stack
/// *is* the cycle path, which is what makes the error message name the actual
/// chain rather than just reporting that a cycle exists.
fn toposort(
    nodes: &[Node],
    edges: &BTreeMap<Name, BTreeSet<Name>>,
) -> Result<Vec<Node>, GraphError> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Unvisited,
        InProgress,
        Done,
    }

    let by_name: BTreeMap<Name, &Node> = nodes.iter().map(|n| (n.name(), n)).collect();
    let mut marks: BTreeMap<Name, Mark> = by_name
        .keys()
        .map(|n| (n.clone(), Mark::Unvisited))
        .collect();
    let mut order = Vec::with_capacity(nodes.len());

    fn visit(
        name: &Name,
        by_name: &BTreeMap<Name, &Node>,
        edges: &BTreeMap<Name, BTreeSet<Name>>,
        marks: &mut BTreeMap<Name, Mark>,
        stack: &mut Vec<Name>,
        order: &mut Vec<Node>,
    ) -> Result<(), GraphError> {
        match marks.get(name) {
            Some(Mark::Done) => return Ok(()),
            Some(Mark::InProgress) => {
                // The stack from the first occurrence of this name onwards is
                // exactly the cycle, closed by repeating the name.
                let start = stack.iter().position(|n| n == name).unwrap_or(0);
                let mut path: Vec<String> =
                    stack[start..].iter().map(ToString::to_string).collect();
                path.push(name.to_string());
                return Err(GraphError::Cycle { path });
            }
            None => return Ok(()),
            Some(Mark::Unvisited) => {}
        }

        marks.insert(name.clone(), Mark::InProgress);
        stack.push(name.clone());

        if let Some(deps) = edges.get(name) {
            for dep in deps {
                visit(dep, by_name, edges, marks, stack, order)?;
            }
        }

        stack.pop();
        marks.insert(name.clone(), Mark::Done);
        if let Some(node) = by_name.get(name) {
            order.push((*node).clone());
        }
        Ok(())
    }

    // Iterating `nodes` in declaration order means that where the graph permits
    // several orders, the one chosen matches what the author wrote.
    for node in nodes {
        visit(
            &node.name(),
            &by_name,
            edges,
            &mut marks,
            &mut Vec::new(),
            &mut order,
        )?;
    }

    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::MANIFEST_NAME;

    fn graph(toml: &str) -> Result<Graph, GraphError> {
        let manifest = Manifest::parse(toml, MANIFEST_NAME).expect("manifest should parse");
        Graph::build(&manifest)
    }

    fn keys(graph: &Graph) -> Vec<String> {
        graph.order().iter().map(|n| n.key.clone()).collect()
    }

    #[test]
    fn independent_questions_keep_their_declaration_order() {
        let graph = graph(
            r#"
            name = "t"
            [questions.first]
            type = "string"
            [questions.second]
            type = "string"
            [questions.third]
            type = "string"
            "#,
        )
        .unwrap();

        assert_eq!(keys(&graph), ["first", "second", "third"]);
    }

    /// The point of the graph: an author may declare in whatever order reads
    /// best, and still be asked in an order that works.
    #[test]
    fn a_dependency_is_asked_before_the_question_that_needs_it() {
        let graph = graph(
            r#"
            name = "t"
            [questions.package_name]
            type = "string"
            default = "{{ project_name | lower }}"
            [questions.project_name]
            type = "string"
            "#,
        )
        .unwrap();

        assert_eq!(keys(&graph), ["project_name", "package_name"]);
    }

    #[test]
    fn a_conditional_question_follows_the_question_it_tests() {
        let graph = graph(
            r#"
            name = "t"
            [questions.cli]
            type = "boolean"
            when = "{{ project_type == 'application' }}"
            [questions.project_type]
            type = "choice"
            choices = ["library", "application"]
            "#,
        )
        .unwrap();

        assert_eq!(keys(&graph), ["project_type", "cli"]);
    }

    #[test]
    fn a_computed_value_is_resolved_between_the_questions_that_bracket_it() {
        let graph = graph(
            r#"
            name = "t"
            [questions.project_name]
            type = "string"
            [questions.crate_path]
            type = "string"
            default = "crates/{{ package_name }}"
            [computed]
            package_name = "{{ project_name | lower }}"
            "#,
        )
        .unwrap();

        assert_eq!(keys(&graph), ["project_name", "package_name", "crate_path"]);
    }

    #[test]
    fn computed_values_are_ordered_by_their_dependencies_not_declaration() {
        let graph = graph(
            r#"
            name = "t"
            [questions.project_name]
            type = "string"
            [computed]
            module_name = "{{ package_name | replace('-', '_') }}"
            package_name = "{{ project_name | lower }}"
            "#,
        )
        .unwrap();

        assert_eq!(
            keys(&graph),
            ["project_name", "package_name", "module_name"]
        );
    }

    /// A data source must load before the question that draws its choices from
    /// it, or the question has nothing to offer.
    #[test]
    fn a_data_source_loads_before_the_question_that_uses_it() {
        let graph = graph(
            r#"
            name = "t"
            [questions.license]
            type = "choice"
            choices_from = "data.licenses.ids"
            [data.licenses]
            source = "data/licenses.toml"
            "#,
        )
        .unwrap();

        assert_eq!(keys(&graph), ["licenses", "license"]);
    }

    #[test]
    fn a_dynamic_data_source_follows_the_answer_it_interpolates() {
        let graph = graph(
            r#"
            name = "t"
            [data.frameworks]
            source = "data/frameworks/{{ project_type }}.toml"
            [questions.project_type]
            type = "choice"
            choices = ["web", "cli"]
            [questions.framework]
            type = "choice"
            choices_from = "data.frameworks.names"
            "#,
        )
        .unwrap();

        assert_eq!(keys(&graph), ["project_type", "frameworks", "framework"]);
    }

    #[test]
    fn a_direct_cycle_is_reported_with_the_chain() {
        let error = graph(
            r#"
            name = "t"
            [computed]
            a = "{{ b }}"
            b = "{{ a }}"
            "#,
        )
        .unwrap_err();

        match error {
            GraphError::Cycle { path } => {
                assert_eq!(path.first(), path.last(), "the chain must close: {path:?}");
                assert!(path.len() >= 3, "{path:?}");
                assert!(path.iter().all(|s| s.starts_with("computed.")), "{path:?}");
            }
            other => panic!("expected a cycle, got {other:?}"),
        }
    }

    #[test]
    fn an_indirect_cycle_is_reported() {
        let error = graph(
            r#"
            name = "t"
            [questions.a]
            type = "string"
            default = "{{ c }}"
            [computed]
            b = "{{ a }}"
            c = "{{ b }}"
            "#,
        )
        .unwrap_err();

        assert!(matches!(error, GraphError::Cycle { .. }), "{error:?}");
    }

    #[test]
    fn a_question_depending_on_itself_is_a_cycle() {
        let error = graph(
            r#"
            name = "t"
            [questions.a]
            type = "string"
            default = "{{ a }}-suffix"
            "#,
        )
        .unwrap_err();

        assert!(matches!(error, GraphError::Cycle { .. }), "{error:?}");
    }

    #[test]
    fn a_typo_is_reported_with_a_suggestion() {
        let error = graph(
            r#"
            name = "t"
            [questions.project_name]
            type = "string"
            [computed]
            package_name = "{{ projct_name | lower }}"
            "#,
        )
        .unwrap_err();

        match error {
            GraphError::UnknownReference {
                unknown,
                suggestion,
                ..
            } => {
                assert_eq!(unknown, "projct_name");
                assert_eq!(suggestion.as_deref(), Some("project_name"));
            }
            other => panic!("expected an unknown reference, got {other:?}"),
        }
    }

    #[test]
    fn an_undeclared_data_source_is_reported() {
        let error = graph(
            r#"
            name = "t"
            [questions.license]
            type = "choice"
            choices_from = "data.licenses.ids"
            "#,
        )
        .unwrap_err();

        assert!(
            matches!(error, GraphError::UnknownReference { ref unknown, .. } if unknown == "data.licenses"),
            "{error:?}"
        );
    }

    /// `template.name` is seeded before anything is resolved, so it is not a
    /// graph node and must not look like a typo.
    #[test]
    fn template_metadata_is_not_an_unknown_reference() {
        let graph = graph(
            r#"
            name = "t"
            [computed]
            title = "{{ template.name }} — a library"
            "#,
        )
        .unwrap();

        assert_eq!(keys(&graph), ["title"]);
    }

    /// Rejecting a loop variable would make `{% for %}` unusable in a default.
    #[test]
    fn a_loop_variable_is_not_an_unknown_reference() {
        let graph = graph(
            r#"
            name = "t"
            [questions.features]
            type = "multi_choice"
            choices = ["a", "b"]
            [computed]
            listed = "{% for f in features %}{{ f }},{% endfor %}"
            "#,
        )
        .unwrap();

        assert_eq!(keys(&graph), ["features", "listed"]);
    }

    #[test]
    fn a_malformed_expression_is_reported_with_its_location() {
        let error = graph(
            r#"
            name = "t"
            [computed]
            broken = "{{ unclosed "
            "#,
        )
        .unwrap_err();

        match error {
            GraphError::InvalidExpression { location, .. } => {
                assert_eq!(location, "computed.broken");
            }
            other => panic!("expected an invalid expression, got {other:?}"),
        }
    }

    #[test]
    fn a_literal_default_creates_no_edges() {
        let graph = graph(
            r#"
            name = "t"
            [questions.license]
            type = "choice"
            choices = ["MIT"]
            default = "MIT"
            [questions.ci]
            type = "boolean"
            default = true
            "#,
        )
        .unwrap();

        assert_eq!(keys(&graph), ["license", "ci"]);
    }
}
