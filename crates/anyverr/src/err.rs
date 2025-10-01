use anyerr::AnyError as AnyErrorTemplate;
use anyerr::context::LiteralKeyStringMapContext;

pub use anyerr::Report;
pub use anyerr::kind::DefaultErrorKind as ErrKind;
pub use anyerr::{Intermediate, Overlay}; // These are helper traits.

pub type AnyError = AnyErrorTemplate<LiteralKeyStringMapContext, ErrKind>;
pub type AnyResult<T> = Result<T, AnyError>;
