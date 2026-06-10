//! **heck** is a case conversion library.
//!
//! Vendored for Trust strict-mode case study RT-12.
//! Original: https://github.com/withoutboats/heck v0.5.0 (MIT OR Apache-2.0)
//!
//! This library exists to provide case conversion between common cases like
//! CamelCase and snake_case. It is intended to be unicode aware, internally
//! consistent, and reasonably well performing.
//!
//! ## Definition of a word boundary
//!
//! Word boundaries are defined by non-alphanumeric characters, as well as
//! within those words in this manner:
//!
//! 1. If an uppercase character is followed by lowercase letters, a word
//!    boundary is considered to be just prior to that uppercase character.
//! 2. If multiple uppercase characters are consecutive, they are considered to
//!    be within a single word, except that the last will be part of the next wo
//!    word if it is followed by lowercase characters (see rule 1).
//!
//! That is, "HelloWorld" is segmented `Hello|World` whereas "XMLHttpRequest" is
//! segmented `XML|Http|Request`.
//!
//! Characters not within words (such as spaces, punctuations, and underscores)
//! are not included in the output string except as they are a part of the case
//! being converted to. Multiple adjacent word boundaries (such as a series of
//! underscores) are folded into one. ("hello__world" in snake case is therefore
//! "hello_world", not the exact same string). Leading or trailing word boundary
//! indicators are dropped, except insofar as CamelCase capitalizes the first
//! word.
//!
//! ### Cases contained in this library:
//!
//! 1. UpperCamelCase
//! 2. lowerCamelCase
//! 3. snake_case
//! 4. kebab-case
//! 5. SHOUTY_SNAKE_CASE
//! 6. Title Case
//! 7. SHOUTY-KEBAB-CASE
//! 8. Train-Case
#![forbid(unsafe_code)]
#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use core::fmt;

// ── core transform engine ────────────────────────────────────────────────────

fn transform<F, G>(
    s: &str,
    mut with_word: F,
    mut boundary: G,
    f: &mut fmt::Formatter,
) -> fmt::Result
where
    F: FnMut(&str, &mut fmt::Formatter) -> fmt::Result,
    G: FnMut(&mut fmt::Formatter) -> fmt::Result,
{
    /// Tracks the current 'mode' of the transformation algorithm as it scans
    /// the input string.
    ///
    /// The mode is a tri-state which tracks the case of the last cased
    /// character of the current word. If there is no cased character
    /// (either lowercase or uppercase) since the previous word boundary,
    /// than the mode is `Boundary`. If the last cased character is lowercase,
    /// then the mode is `Lowercase`. Othertherwise, the mode is
    /// `Uppercase`.
    #[derive(Clone, Copy, PartialEq)]
    enum WordMode {
        /// There have been no lowercase or uppercase characters in the current
        /// word.
        Boundary,
        /// The previous cased character in the current word is lowercase.
        Lowercase,
        /// The previous cased character in the current word is uppercase.
        Uppercase,
    }

    let mut first_word = true;

    for word in s.split(|c: char| !c.is_alphanumeric()) {
        let mut char_indices = word.char_indices().peekable();
        let mut init = 0;
        let mut mode = WordMode::Boundary;

        while let Some((i, c)) = char_indices.next() {
            if let Some(&(next_i, next)) = char_indices.peek() {
                // The mode including the current character, assuming the
                // current character does not result in a word boundary.
                let next_mode = if c.is_lowercase() {
                    WordMode::Lowercase
                } else if c.is_uppercase() {
                    WordMode::Uppercase
                } else {
                    mode
                };

                // Word boundary after if current is not uppercase and next
                // is uppercase
                if next_mode == WordMode::Lowercase && next.is_uppercase() {
                    if !first_word {
                        boundary(f)?;
                    }
                    with_word(&word[init..next_i], f)?;
                    first_word = false;
                    init = next_i;
                    mode = WordMode::Boundary;

                // Otherwise if current and previous are uppercase and next
                // is lowercase, word boundary before
                } else if mode == WordMode::Uppercase && c.is_uppercase() && next.is_lowercase() {
                    if !first_word {
                        boundary(f)?;
                    } else {
                        first_word = false;
                    }
                    with_word(&word[init..i], f)?;
                    init = i;
                    mode = WordMode::Boundary;

                // Otherwise no word boundary, just update the mode
                } else {
                    mode = next_mode;
                }
            } else {
                // Collect trailing characters as a word
                if !first_word {
                    boundary(f)?;
                } else {
                    first_word = false;
                }
                with_word(&word[init..], f)?;
                break;
            }
        }
    }

    Ok(())
}

fn lowercase(s: &str, f: &mut fmt::Formatter) -> fmt::Result {
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == 'Σ' && chars.peek().is_none() {
            write!(f, "ς")?;
        } else {
            write!(f, "{}", c.to_lowercase())?;
        }
    }
    Ok(())
}

fn uppercase(s: &str, f: &mut fmt::Formatter) -> fmt::Result {
    for c in s.chars() {
        write!(f, "{}", c.to_uppercase())?;
    }
    Ok(())
}

fn capitalize(s: &str, f: &mut fmt::Formatter) -> fmt::Result {
    let mut char_indices = s.char_indices();
    if let Some((_, c)) = char_indices.next() {
        write!(f, "{}", c.to_uppercase())?;
        if let Some((i, _)) = char_indices.next() {
            lowercase(s: &s[i..], f: f)?;
        }
    }
    Ok(())
}

// ── kebab-case ───────────────────────────────────────────────────────────────

/// This trait defines a kebab case conversion.
///
/// In kebab-case, word boundaries are indicated by hyphens.
///
/// ## Example:
///
/// ```rust
/// use heck_strict::ToKebabCase;
///
/// let sentence = "We are going to inherit the earth.";
/// assert_eq!(sentence.to_kebab_case(), "we-are-going-to-inherit-the-earth");
/// ```
pub trait ToKebabCase: alloc::borrow::ToOwned {
    /// Convert this type to kebab case.
    fn to_kebab_case(&self) -> Self::Owned;
}

impl ToKebabCase for str {
    fn to_kebab_case(&self) -> Self::Owned {
        AsKebabCase(self).to_string()
    }
}

/// This wrapper performs a kebab case conversion in [`fmt::Display`].
pub struct AsKebabCase<T: AsRef<str>>(pub T);

impl<T: AsRef<str>> fmt::Display for AsKebabCase<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        transform(s: self.0.as_ref(), with_word: lowercase, boundary: |f| write!(f, "-"), f: f)
    }
}

// ── lower camelCase ──────────────────────────────────────────────────────────

/// This trait defines a lower camel case conversion.
///
/// In lowerCamelCase, word boundaries are indicated by capital letters,
/// excepting the first word.
pub trait ToLowerCamelCase: alloc::borrow::ToOwned {
    /// Convert this type to lower camel case.
    fn to_lower_camel_case(&self) -> Self::Owned;
}

impl ToLowerCamelCase for str {
    fn to_lower_camel_case(&self) -> String {
        AsLowerCamelCase(self).to_string()
    }
}

/// This wrapper performs a lower camel case conversion in [`fmt::Display`].
pub struct AsLowerCamelCase<T: AsRef<str>>(pub T);

impl<T: AsRef<str>> fmt::Display for AsLowerCamelCase<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut first = true;
        transform(
            s: self.0.as_ref(),
            with_word: |s, f| {
                if first {
                    first = false;
                    lowercase(s: s, f: f)
                } else {
                    capitalize(s: s, f: f)
                }
            },
            boundary: |_| Ok(()),
            f: f,
        )
    }
}

// ── SHOUTY-KEBAB-CASE ────────────────────────────────────────────────────────

/// This trait defines a shouty kebab case conversion.
///
/// In SHOUTY-KEBAB-CASE, word boundaries are indicated by hyphens and all
/// words are in uppercase.
pub trait ToShoutyKebabCase: alloc::borrow::ToOwned {
    /// Convert this type to shouty kebab case.
    fn to_shouty_kebab_case(&self) -> Self::Owned;
}

impl ToShoutyKebabCase for str {
    fn to_shouty_kebab_case(&self) -> Self::Owned {
        AsShoutyKebabCase(self).to_string()
    }
}

/// This wrapper performs a SHOUTY-KEBAB-CASE conversion in [`fmt::Display`].
pub struct AsShoutyKebabCase<T: AsRef<str>>(pub T);

impl<T: AsRef<str>> fmt::Display for AsShoutyKebabCase<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        transform(s: self.0.as_ref(), with_word: uppercase, boundary: |f| write!(f, "-"), f: f)
    }
}

// ── SHOUTY_SNAKE_CASE ────────────────────────────────────────────────────────

/// This trait defines a shouty snake case conversion.
///
/// In SHOUTY_SNAKE_CASE, word boundaries are indicated by underscores and all
/// words are in uppercase.
pub trait ToShoutySnakeCase: alloc::borrow::ToOwned {
    /// Convert this type to shouty snake case.
    fn to_shouty_snake_case(&self) -> Self::Owned;
}

/// Oh heck, `ToShoutySnekCase` is an alias for [`ToShoutySnakeCase`]. See
/// ToShoutySnakeCase for more documentation.
pub trait ToShoutySnekCase: alloc::borrow::ToOwned {
    /// CONVERT THIS TYPE TO SNEK CASE.
    fn TO_SHOUTY_SNEK_CASE(&self) -> Self::Owned; // intentional non-snake-case: matches output convention
}

impl<T: ?Sized + ToShoutySnakeCase> ToShoutySnekCase for T {
    fn TO_SHOUTY_SNEK_CASE(&self) -> Self::Owned {
        self.to_shouty_snake_case()
    }
}

impl ToShoutySnakeCase for str {
    fn to_shouty_snake_case(&self) -> Self::Owned {
        AsShoutySnakeCase(self).to_string()
    }
}

/// This wrapper performs a SHOUTY_SNAKE_CASE conversion in [`fmt::Display`].
pub struct AsShoutySnakeCase<T: AsRef<str>>(pub T);

/// `AsShoutySnekCase` is an alias for [`AsShoutySnakeCase`].
pub type AsShoutySnekCase<T> = AsShoutySnakeCase<T>;

impl<T: AsRef<str>> fmt::Display for AsShoutySnakeCase<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        transform(s: self.0.as_ref(), with_word: uppercase, boundary: |f| write!(f, "_"), f: f)
    }
}

// ── snake_case ───────────────────────────────────────────────────────────────

/// This trait defines a snake case conversion.
///
/// In snake_case, word boundaries are indicated by underscores.
pub trait ToSnakeCase: alloc::borrow::ToOwned {
    /// Convert this type to snake case.
    fn to_snake_case(&self) -> Self::Owned;
}

/// Oh heck, `SnekCase` is an alias for [`ToSnakeCase`]. See ToSnakeCase for
/// more documentation.
pub trait ToSnekCase: alloc::borrow::ToOwned {
    /// Convert this type to snek case.
    fn to_snek_case(&self) -> Self::Owned;
}

impl<T: ?Sized + ToSnakeCase> ToSnekCase for T {
    fn to_snek_case(&self) -> Self::Owned {
        self.to_snake_case()
    }
}

impl ToSnakeCase for str {
    fn to_snake_case(&self) -> String {
        AsSnakeCase(self).to_string()
    }
}

/// This wrapper performs a snake case conversion in [`fmt::Display`].
pub struct AsSnakeCase<T: AsRef<str>>(pub T);

/// `AsSnekCase` is an alias for [`AsSnakeCase`].
pub type AsSnekCase<T> = AsSnakeCase<T>;

impl<T: AsRef<str>> fmt::Display for AsSnakeCase<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        transform(s: self.0.as_ref(), with_word: lowercase, boundary: |f| write!(f, "_"), f: f)
    }
}

// ── Title Case ───────────────────────────────────────────────────────────────

/// This trait defines a title case conversion.
///
/// In Title Case, word boundaries are indicated by spaces, and every word is
/// capitalized.
pub trait ToTitleCase: alloc::borrow::ToOwned {
    /// Convert this type to title case.
    fn to_title_case(&self) -> Self::Owned;
}

impl ToTitleCase for str {
    fn to_title_case(&self) -> String {
        AsTitleCase(self).to_string()
    }
}

/// This wrapper performs a title case conversion in [`fmt::Display`].
pub struct AsTitleCase<T: AsRef<str>>(pub T);

impl<T: AsRef<str>> fmt::Display for AsTitleCase<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        transform(s: self.0.as_ref(), with_word: capitalize, boundary: |f| write!(f, " "), f: f)
    }
}

// ── Train-Case ───────────────────────────────────────────────────────────────

/// This trait defines a train case conversion.
///
/// In Train-Case, word boundaries are indicated by hyphens and words start
/// with Capital Letters.
pub trait ToTrainCase: alloc::borrow::ToOwned {
    /// Convert this type to Train-Case.
    fn to_train_case(&self) -> Self::Owned;
}

impl ToTrainCase for str {
    fn to_train_case(&self) -> Self::Owned {
        AsTrainCase(self).to_string()
    }
}

/// This wrapper performs a train case conversion in [`fmt::Display`].
pub struct AsTrainCase<T: AsRef<str>>(pub T);

impl<T: AsRef<str>> fmt::Display for AsTrainCase<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        transform(s: self.0.as_ref(), with_word: capitalize, boundary: |f| write!(f, "-"), f: f)
    }
}

// ── UpperCamelCase / PascalCase ──────────────────────────────────────────────

/// This trait defines an upper camel case conversion.
///
/// In UpperCamelCase, word boundaries are indicated by capital letters,
/// including the first word.
pub trait ToUpperCamelCase: alloc::borrow::ToOwned {
    /// Convert this type to upper camel case.
    fn to_upper_camel_case(&self) -> Self::Owned;
}

impl ToUpperCamelCase for str {
    fn to_upper_camel_case(&self) -> String {
        AsUpperCamelCase(self).to_string()
    }
}

/// `ToPascalCase` is an alias for [`ToUpperCamelCase`].
pub trait ToPascalCase: alloc::borrow::ToOwned {
    /// Convert this type to upper camel case.
    fn to_pascal_case(&self) -> Self::Owned;
}

impl<T: ?Sized + ToUpperCamelCase> ToPascalCase for T {
    fn to_pascal_case(&self) -> Self::Owned {
        self.to_upper_camel_case()
    }
}

/// This wrapper performs an upper camel case conversion in [`fmt::Display`].
pub struct AsUpperCamelCase<T: AsRef<str>>(pub T);

/// `AsPascalCase` is an alias for [`AsUpperCamelCase`].
pub type AsPascalCase<T> = AsUpperCamelCase<T>;

impl<T: AsRef<str>> fmt::Display for AsUpperCamelCase<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        transform(s: self.0.as_ref(), with_word: capitalize, boundary: |_| Ok(()), f: f)
    }
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_basic() {
        assert_eq!("CamelCase".to_snake_case(), "camel_case");
        assert_eq!("XMLHttpRequest".to_snake_case(), "xml_http_request");
    }

    #[test]
    fn kebab_basic() {
        assert_eq!("CamelCase".to_kebab_case(), "camel-case");
        assert_eq!("XMLHttpRequest".to_kebab_case(), "xml-http-request");
    }

    #[test]
    fn upper_camel_basic() {
        assert_eq!("snake_case".to_upper_camel_case(), "SnakeCase");
        assert_eq!("kebab-case".to_upper_camel_case(), "KebabCase");
    }

    #[test]
    fn shouty_snake_basic() {
        assert_eq!("CamelCase".to_shouty_snake_case(), "CAMEL_CASE");
    }

    #[test]
    fn title_basic() {
        assert_eq!("hello world".to_title_case(), "Hello World");
    }

    #[test]
    fn train_basic() {
        assert_eq!("hello-world".to_train_case(), "Hello-World");
    }
}
