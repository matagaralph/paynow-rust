use std::{error, fmt, io};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    EmptyCart,
    /// An item has a quantity of zero.
    ZeroQuantity,
    ZeroTotal,
    InvalidEmail,
    InvalidMerchantTrace,
    MissingHash,
    HashMismatch,
    MalformedResponse,
    /// A signed response lacked a field the operation needs, or held an unusable value.
    InvalidField(&'static str),
    /// A signed response carried a status the operation does not expect.
    UnexpectedStatus(String),
    /// Paynow rejected the request; holds the reason it gave.
    Api(String),
    Transport(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCart => f.write_str("payment must contain at least one item"),
            Self::ZeroQuantity => f.write_str("item quantity must be at least one"),
            Self::ZeroTotal => f.write_str("transaction total must be greater than zero"),
            Self::InvalidEmail => {
                f.write_str("a valid auth email is required for mobile transactions")
            }
            Self::InvalidMerchantTrace => {
                f.write_str("merchant trace must be between 1 and 32 characters")
            }
            Self::MissingHash => f.write_str("response does not contain a hash"),
            Self::HashMismatch => f.write_str("response hash does not match"),
            Self::MalformedResponse => f.write_str("response is not valid form-encoded data"),
            Self::InvalidField(field) => {
                write!(f, "response field `{field}` is missing or invalid")
            }
            Self::UnexpectedStatus(status) => write!(f, "unexpected response status `{status}`"),
            Self::Api(message) => write!(f, "Paynow rejected the request: {message}"),
            Self::Transport(err) => write!(f, "request to Paynow failed: {err}"),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Transport(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self::Transport(err)
    }
}
