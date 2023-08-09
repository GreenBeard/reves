use std::collections::BTreeMap;

// See `cargo/src/cargo/core/compiler/mod.rs`
pub(crate) fn envify(s: &str) -> String {
    return s.to_uppercase().replace('-', "_");
}

fn find_crates<'a>(
    variable: &str,
    crate_links: &'a BTreeMap<String, cargo_metadata::PackageId>,
) -> Vec<&'a cargo_metadata::PackageId> {
    let mut crates = Vec::new();
    let mut start: usize = 0;
    while let Some(i) = variable[start..].find('_') {
        let index: usize = start + i;
        if let Some(krate) = crate_links.get(&variable[0..index]) {
            crates.push(krate);
        }
        start = index + 1;
    }
    return crates;
}

/// # Arguments
///
/// * `crate_links` - a bijective map from uppercased [`envified`] `link` to crate `name`.
/// * `variable` - environment variable with `"DEP_"` prefix removed.
pub(crate) fn find_crate<'a>(
    variable: &str,
    crate_links: &'a BTreeMap<String, cargo_metadata::PackageId>,
) -> anyhow::Result<&'a cargo_metadata::PackageId> {
    #[cfg(debug_assertions)]
    for link in crate_links.keys() {
        assert_eq!(envify(link).as_str(), link.as_str());
    }

    let mut crates: Vec<&cargo_metadata::PackageId> = find_crates(variable, crate_links);
    match crates.len() {
        0 => {
            anyhow::bail!("No crate's `links` attribute matches DEP_{}", variable);
        }
        1 => {
            return Ok(crates.remove(0));
        }
        2..=usize::MAX => {
            anyhow::bail!(
                "Multiple crates' `links` attributes matches DEP_{} - {:?}",
                variable,
                crates
            );
        }
        _ => {
            // rust is dumb
            unreachable!();
        }
    }
}

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;

    #[test]
    fn test_find_crates() {
        let crate_links = BTreeMap::<String, cargo_metadata::PackageId>::from([
            (
                super::envify("banana"),
                cargo_metadata::PackageId {
                    repr: "apple".to_owned(),
                },
            ),
            (
                super::envify("banana_banana"),
                cargo_metadata::PackageId {
                    repr: "apple_apple".to_owned(),
                },
            ),
            (
                super::envify("cherry"),
                cargo_metadata::PackageId {
                    repr: "blueberry".to_owned(),
                },
            ),
        ]);
        let env_tests: Vec<(&str, Vec<cargo_metadata::PackageId>)> = vec![
            (
                "BANANA_",
                vec![cargo_metadata::PackageId {
                    repr: "apple".to_owned(),
                }],
            ),
            (
                "BANANA_BANANA_",
                vec![
                    cargo_metadata::PackageId {
                        repr: "apple".to_owned(),
                    },
                    cargo_metadata::PackageId {
                        repr: "apple_apple".to_owned(),
                    },
                ],
            ),
            (
                "BANANA_A",
                vec![cargo_metadata::PackageId {
                    repr: "apple".to_owned(),
                }],
            ),
            (
                "BANANA_BANANA_A",
                vec![
                    cargo_metadata::PackageId {
                        repr: "apple".to_owned(),
                    },
                    cargo_metadata::PackageId {
                        repr: "apple_apple".to_owned(),
                    },
                ],
            ),
            (
                "BANANA_A_B",
                vec![cargo_metadata::PackageId {
                    repr: "apple".to_owned(),
                }],
            ),
            (
                "BANANA_BANANA_A_B",
                vec![
                    cargo_metadata::PackageId {
                        repr: "apple".to_owned(),
                    },
                    cargo_metadata::PackageId {
                        repr: "apple_apple".to_owned(),
                    },
                ],
            ),
            (
                "BANANA_A_B_C",
                vec![cargo_metadata::PackageId {
                    repr: "apple".to_owned(),
                }],
            ),
            (
                "BANANA_BANANA_A_B_C",
                vec![
                    cargo_metadata::PackageId {
                        repr: "apple".to_owned(),
                    },
                    cargo_metadata::PackageId {
                        repr: "apple_apple".to_owned(),
                    },
                ],
            ),
            (
                "BANANA_A_B_C_",
                vec![cargo_metadata::PackageId {
                    repr: "apple".to_owned(),
                }],
            ),
            (
                "BANANA_BANANA_A_B_C_",
                vec![
                    cargo_metadata::PackageId {
                        repr: "apple".to_owned(),
                    },
                    cargo_metadata::PackageId {
                        repr: "apple_apple".to_owned(),
                    },
                ],
            ),
            ("FOO_", vec![]),
        ];

        for env_test in env_tests.iter() {
            assert_eq!(
                super::find_crates(env_test.0, &crate_links),
                env_test.1.iter().collect::<Vec<_>>()
            );
        }
    }
}
