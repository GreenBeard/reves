# Overview

Detect unused cargo dependencies. This tool is an alternative to `cargo
machete`, and `cargo +nightly udeps`. The probably biggest plus of this tool is
that most of it doesn't require a nightly compiler (or the `RUSTC_BOOTSTRAP`
work around to the rust community refusing to publish nightly versions of the
stable versions). As some crates abuse build.rs and attempt to auto-detect
nightly (the rust community should accept the curse that is cfg flags instead of
abusing feature flags, and auto-detection) you won't hit those issues. You
probably should be using Bazel instead of cargo, but there aren't great guides
on setting up all of the things needed and most people don't love build systems
so you will probably just stick with cargo.

## License

All code in this repository is licensed under any of

- Apache License, Version 2.0, ([LICENSE.Apache-2.0](./LICENSE.Apache-2.0), or <https://spdx.org/licenses/Apache-2.0>)
- MIT license ([LICENSE.MIT](./LICENSE.MIT), or <https://spdx.org/licenses/MIT>)
- CC0, Version 1.0 ([LICENSE.CC0-1.0](./LICENSE.CC0-1.0), or <https://spdx.org/licenses/CC0-1.0>)

at your option.

## Maintenance

This code is unmaintained. I don't currently plan to acknowledge Github issues,
or pull requests. If I change my mind later then I will update this section. Use
it if you feel like it (or don't). There is currently no official version of
this crate on crates.io -- anything you find there is not published by me.
