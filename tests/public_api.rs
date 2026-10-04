//! Exercises the crate's public API exactly as a downstream consumer would.
//!
//! These tests are in-process and parallel-safe: no environment variables, no
//! working-directory changes, no child processes and no fixed paths or ports.

use template_rust::{GreetError, MAX_NAME_CHARS, greet};

#[test]
fn greeting_round_trips_through_the_public_api() {
    assert_eq!(greet("Linus").as_deref(), Ok("Hello, Linus!"));
}

#[test]
fn errors_are_typed_and_implement_std_error() {
    let err: Box<dyn std::error::Error> = Box::new(greet(" ").unwrap_err());
    assert_eq!(err.to_string(), "name must not be empty");
}

#[test]
fn limit_counts_characters_not_bytes() {
    // Each "ü" is two bytes in UTF-8; the limit is in characters.
    let name = "ü".repeat(MAX_NAME_CHARS);
    assert!(greet(&name).is_ok());
    assert_eq!(
        greet(&format!("{name}ü")),
        Err(GreetError::NameTooLong {
            chars: MAX_NAME_CHARS + 1
        })
    );
}
