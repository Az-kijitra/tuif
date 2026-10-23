//! SPEC-0300-0011 §8/§9 (R-62, R-62-1): name validation shared by rename
//! (§8) and create (§9). Planned API (not yet implemented — see
//! `super::mod` doc comment):
//!
//! ```ignore
//! pub enum NameError { Required, InvalidName }
//! pub fn validate_name(name: &str) -> Result<(), NameError>
//! ```
//!
//! R-62 rules (in order):
//! 1. leading/trailing-whitespace-trimmed name is empty -> `Required`
//! 2. name is exactly `.` or `..` -> `InvalidName`
//! 3. name contains one of the 9 forbidden characters
//!    `\ / : * ? " < > |` -> `InvalidName`
//!
//! R-62-1 notes the *frontend* only performs the empty check itself and
//! relies on the backend for the rest; this module is that backend
//! authority, shared identically by `preflight::preflight_rename` and
//! `preflight::preflight_create` (AC-66).

/// R-62: the outcome of `validate_name` when a name is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    /// R-62 #1: the trimmed name is empty.
    Required,
    /// R-62 #2/#3: the name is `.`/`..`, or contains a forbidden character.
    InvalidName,
}

/// R-62 #3: the 9 forbidden characters (Evaluator-confirmed list, not 8).
const FORBIDDEN_CHARS: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

/// R-62: shared name validation used by both rename (§8) and create (§9).
pub fn validate_name(name: &str) -> Result<(), NameError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(NameError::Required);
    }
    if trimmed == "." || trimmed == ".." {
        return Err(NameError::InvalidName);
    }
    if name.chars().any(|ch| FORBIDDEN_CHARS.contains(&ch)) {
        return Err(NameError::InvalidName);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORBIDDEN_CHARS: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

    // SPEC-0300-0011 AC-52 (R-62 #1): empty or whitespace-only names are
    // rejected as "required", not "invalid_name".
    #[test]
    fn ac52_empty_name_is_required_error() {
        assert_eq!(validate_name(""), Err(NameError::Required));
    }

    #[test]
    fn ac52_whitespace_only_name_is_required_error() {
        assert_eq!(validate_name("   "), Err(NameError::Required));
        assert_eq!(validate_name("\t\n"), Err(NameError::Required));
    }

    // SPEC-0300-0011 AC-53 (R-62 #2): "." and ".." are rejected as
    // invalid_name (distinct from the empty/required case).
    #[test]
    fn ac53_dot_is_invalid_name() {
        assert_eq!(validate_name("."), Err(NameError::InvalidName));
    }

    #[test]
    fn ac53_dotdot_is_invalid_name() {
        assert_eq!(validate_name(".."), Err(NameError::InvalidName));
    }

    // SPEC-0300-0011 AC-54 (R-62 #3): each of the 9 forbidden characters,
    // individually, makes the name invalid (9 cases, per the Evaluator's
    // confirmed 9-character list — not 8).
    #[test]
    fn ac54_each_of_the_nine_forbidden_characters_is_invalid_name() {
        assert_eq!(
            FORBIDDEN_CHARS.len(),
            9,
            "R-62 #3 lists exactly 9 forbidden characters"
        );
        for ch in FORBIDDEN_CHARS {
            let name = format!("bad{ch}name");
            assert_eq!(
                validate_name(&name),
                Err(NameError::InvalidName),
                "expected '{ch}' to be rejected as invalid_name"
            );
        }
    }

    // Sanity check: an ordinary name with none of the above problems is
    // accepted (both rename and create rely on this common-case success).
    #[test]
    fn ordinary_name_is_accepted() {
        assert_eq!(validate_name("report.txt"), Ok(()));
        assert_eq!(validate_name("New Folder"), Ok(()));
    }

    // A name that is only invalid after trimming surrounding whitespace is
    // still accepted once trimmed (R-62 #1 only rejects the *trimmed*
    // result being empty; it does not forbid leading/trailing spaces from
    // being silently significant elsewhere — this module's contract is
    // limited to the three listed rules).
    #[test]
    fn leading_and_trailing_whitespace_around_a_valid_name_is_not_by_itself_an_error() {
        assert_eq!(validate_name("  report.txt  "), Ok(()));
    }
}
