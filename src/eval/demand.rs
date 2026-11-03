//! Scoped demand resolution. Pure MiniJinja attempts may be retried, never prompts or effects.

use std::collections::BTreeSet;
use std::sync::Arc;

use super::{EvalError, Partials, evaluate, render_string};
use crate::context::{Context, Name, Namespace};
use crate::data::{Loader, Rendered};
use crate::template::{Manifest, computed_expression};

pub(crate) struct Session<'a, 'repo> {
    pub context: Context,
    manifest: &'a Manifest,
    loader: &'a mut Loader<'repo>,
    partials: &'a Arc<Partials>,
    active: BTreeSet<Name>,
}

impl<'a, 'repo> Session<'a, 'repo> {
    pub fn new(
        manifest: &'a Manifest,
        loader: &'a mut Loader<'repo>,
        partials: &'a Arc<Partials>,
    ) -> Self {
        let mut context = Context::new().with_template(manifest.metadata());
        context.defer(
            manifest
                .data
                .keys()
                .map(Name::data)
                .chain(manifest.computed.keys().map(Name::computed)),
        );
        Self {
            context,
            manifest,
            loader,
            partials,
            active: BTreeSet::new(),
        }
    }

    pub fn run<T, E: From<EvalError>>(
        &mut self,
        mut attempt: impl FnMut(&Context) -> Result<T, E>,
    ) -> Result<T, E> {
        loop {
            let result = attempt(&self.context);
            if let Some(name) = self.context.take_demand() {
                self.ensure(name)?;
            } else {
                return result;
            }
        }
    }

    pub fn ensure(&mut self, name: Name) -> Result<(), EvalError> {
        if self.context.has(&name) {
            return Ok(());
        }
        if !self.active.insert(name.clone()) {
            return Err(EvalError::Expression {
                location: name.to_string(),
                expression: name.to_string(),
                reason:
                    "cyclic runtime dependency; make the dependency visible in the manifest graph"
                        .into(),
            });
        }
        let partials = self.partials.clone();
        let value = match name.namespace {
            Namespace::Data => {
                let decl = self.manifest.data[&name.key].clone();
                let location = format!("data.{}", name.key);
                let source =
                    self.run(|ctx| render_string(&decl.source, ctx, &location, &partials))?;
                let reference = decl
                    .reference
                    .as_deref()
                    .map(|text| {
                        self.run(|ctx| {
                            render_string(text, ctx, &format!("{location}.ref"), &partials)
                        })
                    })
                    .transpose()?;
                let path = decl
                    .path
                    .as_deref()
                    .map(|text| {
                        self.run(|ctx| {
                            render_string(text, ctx, &format!("{location}.path"), &partials)
                        })
                    })
                    .transpose()?;
                let value = self.loader.load(
                    &name.key,
                    &decl,
                    Rendered {
                        source: &source,
                        reference: reference.as_deref(),
                        path: path.as_deref(),
                    },
                )?;
                self.context.set_data(&name.key, value);
                None
            }
            Namespace::Computed => {
                let declared = self.manifest.computed[&name.key].clone();
                Some(match computed_expression(&declared) {
                    Some(expression) => {
                        self.run(|ctx| evaluate(expression, ctx, &name.to_string(), &partials))?
                    }
                    None => declared,
                })
            }
            _ => None,
        };
        if let Some(value) = value {
            self.context.set_computed(&name.key, value);
        }
        self.active.remove(&name);
        Ok(())
    }

    pub fn computed(&mut self) -> Result<(), EvalError> {
        for key in self.manifest.computed.keys() {
            self.ensure(Name::computed(key))?;
        }
        Ok(())
    }

    pub fn finish(self) -> Context {
        self.context.snapshot()
    }
}
