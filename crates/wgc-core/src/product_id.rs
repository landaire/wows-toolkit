// filled in a later task

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProductIdParseError {
    #[error("parse error")]
    ParseError,
}

pub struct ProductId;
pub struct Realm;
