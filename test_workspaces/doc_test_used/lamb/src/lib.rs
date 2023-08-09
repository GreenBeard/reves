/// ```rust
/// const _BUNNY_NAME: &str = bunny::NAME;
/// ```
fn _nothing_one() {}

/// ```rust
/// const _BUNNY_NAME: &str = bunny::NAME;
/// // should cause a warning
/// while true {
///   break;
/// }
/// ```
fn _nothing_two() {}

/// ```rust
/// // should cause a warning
/// while true {
///   break;
/// }
/// ```
fn _nothing_three() {}
