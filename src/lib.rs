//! Library half of the template crate.
//!
//! Keep domain logic here, behind a small documented public API, and keep
//! `src/main.rs` a thin shell that parses input, calls the library and maps
//! errors to an exit code. Unit tests live beside the code they cover; tests
//! that exercise the public API as a consumer would live under `tests/`.

#![deny(missing_docs)]
#![deny(unsafe_code)]

use std::error::Error;
use std::fmt;

/// Longest name [`greet`] accepts, in characters.
///
/// Bounding caller-supplied input keeps allocation proportional to a limit we
/// chose rather than to whatever the caller sends.
pub const MAX_NAME_CHARS: usize = 64;

/// Why [`greet`] rejected its input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GreetError {
    /// The name was empty or contained only whitespace.
    EmptyName,
    /// The name was longer than [`MAX_NAME_CHARS`] characters.
    NameTooLong {
        /// Number of characters in the rejected name.
        chars: usize,
    },
}

impl fmt::Display for GreetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => write!(f, "name must not be empty"),
            Self::NameTooLong { chars } => write!(
                f,
                "name is {chars} characters long; the limit is {MAX_NAME_CHARS}"
            ),
        }
    }
}

impl Error for GreetError {}

/// Builds a greeting for `name`.
///
/// Leading and trailing whitespace is trimmed before the name is checked.
///
/// # Errors
///
/// Returns [`GreetError::EmptyName`] when the trimmed name is empty, and
/// [`GreetError::NameTooLong`] when it exceeds [`MAX_NAME_CHARS`] characters.
///
/// # Examples
///
/// ```
/// use template_rust::{greet, GreetError};
///
/// assert_eq!(greet("  Ada ").as_deref(), Ok("Hello, Ada!"));
/// assert_eq!(greet("   "), Err(GreetError::EmptyName));
/// ```
pub fn greet(name: &str) -> Result<String, GreetError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(GreetError::EmptyName);
    }
    let chars = name.chars().count();
    if chars > MAX_NAME_CHARS {
        return Err(GreetError::NameTooLong { chars });
    }
    Ok(format!("Hello, {name}!"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greets_a_trimmed_name() {
        assert_eq!(greet("\tGrace\n").as_deref(), Ok("Hello, Grace!"));
    }

    #[test]
    fn rejects_an_empty_name() {
        assert_eq!(greet(""), Err(GreetError::EmptyName));
    }

    #[test]
    fn accepts_a_name_at_the_limit() {
        let name = "a".repeat(MAX_NAME_CHARS);
        assert!(greet(&name).is_ok());
    }

    #[test]
    fn rejects_a_name_over_the_limit() {
        let name = "é".repeat(MAX_NAME_CHARS + 1);
        assert_eq!(
            greet(&name),
            Err(GreetError::NameTooLong {
                chars: MAX_NAME_CHARS + 1
            })
        );
    }

    #[test]
    fn error_messages_name_the_problem() {
        assert_eq!(GreetError::EmptyName.to_string(), "name must not be empty");
        assert_eq!(
            GreetError::NameTooLong { chars: 70 }.to_string(),
            "name is 70 characters long; the limit is 64"
        );
    }
}
