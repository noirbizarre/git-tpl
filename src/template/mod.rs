//! The template manifest, its questions, and the values they produce.

mod choice;
mod manifest;
mod question;
mod value;

pub use choice::{Choice, ChoiceError};
pub use manifest::{DEFAULT_ROOT, DataSourceDecl, MANIFEST_NAME, Manifest, ManifestError};
pub use question::{GIT_PREFIX, Question, QuestionKind, is_expression};
pub use value::{Value, ValueError};
