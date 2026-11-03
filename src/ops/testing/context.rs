//! Context assertions are demand roots in the same scoped session as the rendered files.

use super::*;
use crate::context::Name;

/// Declarative assertions over the template's resolved context.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContextExpect {
    /// Paths that must exist, even if their values are null or falsy.
    pub present: Vec<String>,
    /// Paths that must not exist.
    pub absent: Vec<String>,
    /// Literal, typed equality targets.
    pub equals: BTreeMap<String, Value>,
    /// Typed expressions that must return boolean true.
    pub expressions: Vec<String>,
}

impl ContextExpect {
    pub(super) fn is_empty(&self) -> bool {
        self.present.is_empty()
            && self.absent.is_empty()
            && self.equals.is_empty()
            && self.expressions.is_empty()
    }

    pub(super) fn parse(
        value: Option<&Value>,
        shape: &impl Fn(String) -> TestingError,
    ) -> Result<Self, TestingError> {
        let Some(value) = value else {
            return Ok(Self::default());
        };
        let table = value
            .as_table()
            .ok_or_else(|| shape("`expect.context` must be a table.".into()))?;
        for key in table.keys() {
            if !matches!(
                key.as_str(),
                "present" | "absent" | "equals" | "expressions"
            ) {
                let hint = closest(key, ["present", "absent", "equals", "expressions"])
                    .map(|near| format!(" Did you mean `{near}`?"))
                    .unwrap_or_default();
                return Err(shape(format!(
                    "unknown context expectation `expect.context.{key}`.{hint}"
                )));
            }
        }
        let result = Self {
            present: string_array(table.get("present"), "expect.context.present", shape)?,
            absent: string_array(table.get("absent"), "expect.context.absent", shape)?,
            equals: match table.get("equals") {
                None => BTreeMap::new(),
                Some(Value::Table(values)) => values.clone(),
                Some(_) => {
                    return Err(shape(
                        "`expect.context.equals` must be a table of literal expected values."
                            .into(),
                    ));
                }
            },
            expressions: string_array(
                table.get("expressions"),
                "expect.context.expressions",
                shape,
            )?,
        };
        for path in result
            .present
            .iter()
            .chain(&result.absent)
            .chain(result.equals.keys())
        {
            if matches!(path.as_str(), "data" | "template") || path.split('.').any(str::is_empty) {
                return Err(shape(format!(
                    "invalid context path `{path}`; use nonempty dotted table members, such as `data.source.field`, or an expression for namespace objects."
                )));
            }
        }
        for path in &result.absent {
            if result.present.contains(path) || result.equals.contains_key(path) {
                return Err(shape(format!(
                    "context path `{path}` cannot be both absent and present or equal."
                )));
            }
        }
        for expression in &result.expressions {
            if crate::eval::whole_expression(expression).is_none() {
                return Err(shape("`expect.context.expressions` requires whole `{{ ... }}` expressions returning booleans.".into()));
            }
        }
        Ok(result)
    }

    fn check(
        &self,
        session: &mut crate::eval::Session<'_, '_>,
        template: &Resolved,
        failures: &mut Vec<Failure>,
    ) -> Result<(), OpError> {
        // An unloaded declaration is not absent. Demand only roots actually asserted by this case.
        for path in self
            .present
            .iter()
            .chain(&self.absent)
            .chain(self.equals.keys())
        {
            let mut parts = path.split('.');
            let root = parts.next().expect("validated context path");
            if root == "data" {
                if let Some(source) = parts
                    .next()
                    .filter(|name| template.manifest.data.contains_key(*name))
                {
                    session.ensure(Name::data(source))?;
                }
            } else if template.manifest.computed.contains_key(root) {
                session.ensure(Name::computed(root))?;
            }
        }
        let context = &session.context;
        for path in &self.present {
            if context.get_path(path).is_none() {
                failures.push(Failure::ContextMissing {
                    path: path.clone(),
                    expected: None,
                });
            }
        }
        for path in &self.absent {
            if let Some(actual) = context.get_path(path) {
                failures.push(Failure::ContextUnexpected {
                    path: path.clone(),
                    actual: actual.clone(),
                });
            }
        }
        for (path, expected) in &self.equals {
            match context.get_path(path) {
                None => failures.push(Failure::ContextMissing {
                    path: path.clone(),
                    expected: Some(expected.clone()),
                }),
                Some(actual) if actual != expected => failures.push(Failure::ContextMismatch {
                    path: path.clone(),
                    expected: expected.clone(),
                    actual: actual.clone(),
                }),
                Some(_) => {}
            }
        }
        let partials = template.partials()?;
        for (index, expression) in self.expressions.iter().enumerate() {
            let location = format!("expect.context.expressions[{index}]");
            let actual = session
                .run(|context| crate::eval::evaluate(expression, context, &location, &partials))?;
            if actual != Value::Bool(true) {
                failures.push(Failure::ContextExpression {
                    expression: expression.clone(),
                    actual,
                });
            }
        }
        Ok(())
    }
}

/// Keep acquisition inside the production render; a finished context cannot load lazy declarations.
#[allow(clippy::too_many_arguments)]
pub(super) fn render(
    template: &Resolved,
    source: &str,
    project: Option<(&dyn GitBackend, &Path)>,
    supplied: crate::answers::SuppliedAnswers,
    user: &UserConfig,
    trust: Trust<'_>,
    expect: Option<&ContextExpect>,
    failures: &mut Vec<Failure>,
) -> Result<RenderedOnce, OpError> {
    super::super::render_resolved_preparing(
        template,
        source,
        project,
        supplied,
        user,
        Answering::defaults(),
        trust,
        None,
        match project {
            Some((_, root)) if crate::config::Config::exists_in(root) => {
                super::super::Preparation::Update
            }
            Some(_) => super::super::Preparation::Init,
            None => super::super::Preparation::Files,
        },
        |session| {
            if let Some(expect) = expect.filter(|expect| !expect.is_empty()) {
                expect.check(session, template, failures)?;
            }
            Ok(())
        },
    )
}
