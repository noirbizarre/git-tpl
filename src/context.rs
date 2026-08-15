//! The evaluation context shared by prompting and rendering.
//!
//! One context serves both, so a value resolved while asking a question is the
//! same value the template sees. There is no second pass and no second context.
//! See `docs/adr/007-static-dependency-graph.md`.

use std::collections::BTreeMap;

use crate::template::Value;

/// The namespace a name belongs to.
///
/// Answers and computed values share the top level of the context, because
/// `{{ answers.project_name }}` is noise and templates are read far more often
/// than they are written. That is why a collision between the two is an error
/// rather than a shadow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Namespace {
    /// A question's answer.
    Answer,
    /// A computed value.
    Computed,
    /// A loaded data source.
    Data,
    /// Template metadata.
    Template,
}

/// A fully-qualified name in the context.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name {
    /// Which namespace it lives in.
    pub namespace: Namespace,
    /// The name within that namespace.
    pub key: String,
}

impl Name {
    /// A question's answer.
    pub fn answer(key: impl Into<String>) -> Self {
        Self {
            namespace: Namespace::Answer,
            key: key.into(),
        }
    }

    /// A computed value.
    pub fn computed(key: impl Into<String>) -> Self {
        Self {
            namespace: Namespace::Computed,
            key: key.into(),
        }
    }

    /// A data source.
    pub fn data(key: impl Into<String>) -> Self {
        Self {
            namespace: Namespace::Data,
            key: key.into(),
        }
    }
}

impl std::fmt::Display for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.namespace {
            // Answers and computed values are shown with their namespace in
            // diagnostics — a cycle reading `a -> b -> a` is far less useful
            // than `answers.a -> computed.b -> answers.a`, which says which
            // declaration to go and look at.
            Namespace::Answer => write!(f, "answers.{}", self.key),
            Namespace::Computed => write!(f, "computed.{}", self.key),
            Namespace::Data => write!(f, "data.{}", self.key),
            Namespace::Template => write!(f, "template.{}", self.key),
        }
    }
}

/// The resolved context.
#[derive(Debug, Clone, Default)]
pub struct Context {
    answers: BTreeMap<String, Value>,
    computed: BTreeMap<String, Value>,
    data: BTreeMap<String, Value>,
    template: BTreeMap<String, Value>,
}

impl Context {
    /// An empty context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed the template metadata.
    pub fn with_template(mut self, metadata: Value) -> Self {
        if let Value::Table(map) = metadata {
            self.template = map;
        }
        self
    }

    /// Record an answer.
    pub fn set_answer(&mut self, key: impl Into<String>, value: Value) {
        self.answers.insert(key.into(), value);
    }

    /// Record a computed value.
    pub fn set_computed(&mut self, key: impl Into<String>, value: Value) {
        self.computed.insert(key.into(), value);
    }

    /// Record loaded data.
    pub fn set_data(&mut self, key: impl Into<String>, value: Value) {
        self.data.insert(key.into(), value);
    }

    /// The answers, for writing back to `.config/git.tpl.toml`.
    ///
    /// Only answers are recorded. Computed values are a function of the answers
    /// and the template, so a template that changes how one is derived should
    /// change it for existing projects too.
    pub fn answers(&self) -> &BTreeMap<String, Value> {
        &self.answers
    }

    /// The loaded data.
    pub fn data(&self) -> &BTreeMap<String, Value> {
        &self.data
    }

    /// Whether a name has been resolved.
    pub fn has(&self, name: &Name) -> bool {
        match name.namespace {
            Namespace::Answer => self.answers.contains_key(&name.key),
            Namespace::Computed => self.computed.contains_key(&name.key),
            Namespace::Data => self.data.contains_key(&name.key),
            Namespace::Template => self.template.contains_key(&name.key),
        }
    }

    /// Look up a dotted path, as an expression would see it.
    pub fn get_path(&self, path: &str) -> Option<&Value> {
        let (head, rest) = match path.split_once('.') {
            Some((head, rest)) => (head, Some(rest)),
            None => (path, None),
        };

        let root = match head {
            // `data.<name>` is a namespace whose values are themselves
            // structured, so the name is consumed here and the remainder is
            // walked inside the value: `data.licenses.names.MIT` is
            // source `licenses`, then `names.MIT` within it.
            "data" => {
                let rest = rest?;
                let (key, tail) = match rest.split_once('.') {
                    Some((key, tail)) => (key, Some(tail)),
                    None => (rest, None),
                };
                let value = self.data.get(key)?;
                return match tail {
                    Some(tail) => value.get_path(tail),
                    None => Some(value),
                };
            }
            // `template` is flat by contrast — `name` and `description`, and
            // nothing nested — so there is no walk to do. Asymmetric with
            // `data` on purpose: a template author cannot add keys here.
            "template" => {
                let rest = rest?;
                return self.template.get(rest);
            }
            // Answers and computed values are flat at the top level. Answers
            // are checked first only because a collision is rejected at load
            // time, so the order can never actually matter.
            other => self
                .answers
                .get(other)
                .or_else(|| self.computed.get(other))?,
        };

        match rest {
            Some(rest) => root.get_path(rest),
            None => Some(root),
        }
    }

    /// The context as MiniJinja sees it.
    ///
    /// Deliberately contains no environment, no clock, no Git user and no
    /// process state. See `docs/adr/006-no-runtime-context.md`.
    pub fn to_minijinja(&self) -> minijinja::Value {
        let mut root: BTreeMap<String, minijinja::Value> = BTreeMap::new();

        for (key, value) in &self.answers {
            root.insert(key.clone(), value.clone().into());
        }
        for (key, value) in &self.computed {
            root.insert(key.clone(), value.clone().into());
        }

        root.insert("data".to_string(), Value::Table(self.data.clone()).into());
        root.insert(
            "template".to_string(),
            Value::Table(self.template.clone()).into(),
        );

        minijinja::Value::from_iter(root)
    }

    /// Computed values, by name.
    ///
    /// The counterpart of [`answers`](Self::answers), which existed because
    /// only answers are recorded. This exists because `git tpl context` has to
    /// show the whole of what a template sees, and a computed value is the
    /// part an author most often gets wrong.
    pub fn computed(&self) -> &BTreeMap<String, Value> {
        &self.computed
    }

    /// Template metadata, by name.
    pub fn template(&self) -> &BTreeMap<String, Value> {
        &self.template
    }

    /// Everything a template sees, as JSON.
    ///
    /// `flat` mirrors [`to_minijinja`](Self::to_minijinja): answers and
    /// computed values at the top level, `data` and `template` namespaced. A
    /// dump that did not match what the renderer sees would be worse than
    /// none, because it would be believed.
    pub fn to_json(&self) -> serde_json::Value {
        let table = |map: &BTreeMap<String, Value>| {
            serde_json::to_value(Value::Table(map.clone())).unwrap_or(serde_json::Value::Null)
        };

        let mut flat = self.answers.clone();
        flat.extend(self.computed.clone());

        serde_json::json!({
            "answers": table(&self.answers),
            "computed": table(&self.computed),
            "data": table(&self.data),
            "template": table(&self.template),
            "flat": table(&flat),
        })
    }

    /// A stable digest of the answers, recorded in the commit trailers.    ///
    /// Lets `status` detect that answers changed without reading and comparing
    /// the configuration file, and records in Git what the tree was rendered
    /// from. Only answers, because only answers are input.
    pub fn answers_digest(&self) -> String {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        for (key, value) in &self.answers {
            // Length-prefixed so that `{ab: c}` and `{a: bc}` cannot collide.
            hasher.update(key.len().to_le_bytes());
            hasher.update(key.as_bytes());
            let canonical = value.canonical();
            hasher.update(canonical.len().to_le_bytes());
            hasher.update(canonical.as_bytes());
        }
        format!("sha256:{}", hex::encode(hasher.finalize()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        let mut ctx = Context::new().with_template(Value::Table(BTreeMap::from([(
            "name".to_string(),
            Value::String("rust-library".into()),
        )])));
        ctx.set_answer("project_name", Value::String("My Project".into()));
        ctx.set_computed("package_name", Value::String("my-project".into()));
        ctx.set_data(
            "licenses",
            Value::Table(BTreeMap::from([(
                "ids".to_string(),
                Value::Array(vec![Value::String("MIT".into())]),
            )])),
        );
        ctx
    }

    #[test]
    fn answers_and_computed_values_are_reachable_at_the_top_level() {
        let ctx = context();
        assert_eq!(
            ctx.get_path("project_name"),
            Some(&Value::String("My Project".into()))
        );
        assert_eq!(
            ctx.get_path("package_name"),
            Some(&Value::String("my-project".into()))
        );
    }

    #[test]
    fn data_and_template_are_namespaced() {
        let ctx = context();
        assert_eq!(
            ctx.get_path("data.licenses.ids"),
            Some(&Value::Array(vec![Value::String("MIT".into())]))
        );
        assert_eq!(
            ctx.get_path("template.name"),
            Some(&Value::String("rust-library".into()))
        );
        assert_eq!(ctx.get_path("data.absent"), None);
    }

    #[test]
    fn a_name_displays_with_its_namespace_so_a_cycle_says_where_to_look() {
        assert_eq!(Name::answer("a").to_string(), "answers.a");
        assert_eq!(Name::computed("b").to_string(), "computed.b");
        assert_eq!(Name::data("c").to_string(), "data.c");
    }

    /// The whole determinism guarantee rests on the context being free of
    /// ambient state, so assert its shape rather than trusting review.
    #[test]
    fn the_minijinja_context_exposes_no_runtime_namespaces() {
        let ctx = context();
        let value = ctx.to_minijinja();

        for absent in ["env", "now", "git", "platform", "repository", "runtime"] {
            assert!(
                value
                    .get_attr(absent)
                    .map(|v| v.is_undefined())
                    .unwrap_or(true),
                "`{absent}` must not be in the render context"
            );
        }

        assert!(!value.get_attr("data").unwrap().is_undefined());
        assert!(!value.get_attr("template").unwrap().is_undefined());
        assert!(!value.get_attr("project_name").unwrap().is_undefined());
    }

    #[test]
    fn the_digest_covers_answers_and_ignores_everything_else() {
        let mut a = context();
        let mut b = context();
        assert_eq!(a.answers_digest(), b.answers_digest());

        b.set_computed("package_name", Value::String("different".into()));
        b.set_data("licenses", Value::Array(vec![]));
        assert_eq!(
            a.answers_digest(),
            b.answers_digest(),
            "computed values and data are not input and must not change the digest"
        );

        a.set_answer("project_name", Value::String("Other".into()));
        assert_ne!(a.answers_digest(), b.answers_digest());
    }

    /// The digest goes into a commit trailer and is compared across runs, so it
    /// must not depend on the order answers happened to be recorded in.
    #[test]
    fn the_digest_is_stable_across_insertion_order() {
        let mut first = Context::new();
        first.set_answer("z", Value::Integer(1));
        first.set_answer("a", Value::Integer(2));

        let mut second = Context::new();
        second.set_answer("a", Value::Integer(2));
        second.set_answer("z", Value::Integer(1));

        assert_eq!(first.answers_digest(), second.answers_digest());
    }
}
