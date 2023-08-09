/// ```rust,ignore
/// const _BUNNY_NAME: &str = bunny::NAME;
/// ```
fn _nothing_one() {}

/// ```rust,ignore
/// const _BUNNY_NAME: &str = bunny::NAME;
/// // should cause a warning
/// while true { break; }
/// ```
fn _nothing_two() {}

/// ```rust,ignore
/// // should cause a warning
/// while true { break; }
/// ```
fn _nothing_three() {}
