#![strict]

/// Build a `(width, height)` tuple using named-arg syntax.
///
/// This doc-test exercises the `RUSTDOC` wrapper: without it, `cargo test
/// --doc` would reject `make_rect(width: 7, height: 6)` as a rustc parse
/// error.
///
/// ```
/// use cargo_strict_fixture_multimod::geom::make_rect;
/// let r = make_rect(width: 7, height: 6);
/// assert_eq!(r, (7, 6));
/// ```
#[allow(trust::R0017, reason = "fixture models the same-typed-swap bug class")]
pub fn make_rect(width: u32, height: u32) -> (u32, u32) {
    (width, height)
}

/// Area of a `(width, height)` rectangle.
///
/// ```
/// use cargo_strict_fixture_multimod::geom::{make_rect, area};
/// assert_eq!(area(make_rect(width: 3, height: 4)), 12);
/// ```
pub fn area(rect: (u32, u32)) -> u32 {
    rect.0 * rect.1
}
