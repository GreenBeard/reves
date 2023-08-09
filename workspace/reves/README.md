# Reves

This crate is designed to find unused dependencies, and mislabeled
dependencies. Currently it does not support cross-compilation as the crate is
attempting to avoid using any nightly features to the extent possible, and
without the host-config feature one can't specify flags for the host build.rs
scripts.

## Known causes of false positives, and false negatives
- "Normal" dependencies that are only use for doc links (not doc tests, but
  plain docs) are considered unused. There are no plans to fix this and having
  these style of dependencies definitely hurts build times for no little to no
  benefit.
