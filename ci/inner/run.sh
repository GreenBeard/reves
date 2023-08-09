#!/bin/bash

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

export RUSTFLAGS="-D warnings"

add_vendor_config() {
  rm -f ./workspace/.cargo/config.toml
  mkdir -p ./workspace/.cargo
  cat <<"EOF" > ./workspace/.cargo/config.toml
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
EOF
}

remove_vendor_config() {
  rm -f ./workspace/.cargo/config.toml
}

check_shellcheck() {
  old_state="$(shopt -p)"
  shopt -s globstar
  for file in ./**/*.sh; do
    status=0
    git check-ignore -q "$file" || status="$?"
    if [ "$status" -eq 1 ]; then
      shellcheck "$file"
    elif [ "$status" -eq 0 ]; then
      # Do nothing
      :
    else
      echo "unexpected git check-ignore result ($status)"
      exit 1
    fi
  done
  eval "${old_state}"
}

check_cargo_folder() {
  pushd "$1" >/dev/null
  cargo deny --offline check -- bans licenses sources
  cargo fmt --all -- --check
  cargo check --all-features --all-targets
  cargo clippy --all-features --all-targets
  cargo test --all-features --all-targets -r
  cargo test --doc
  popd >/dev/null
}

check_cargo() {
  check_cargo_folder ./workspace
  for dir in ./test_workspaces/*; do
    if [ -d "${dir}" ]; then
      check_cargo_folder "${dir}"
    fi
  done
  pushd ./workspace/harness >/dev/null
  cargo run -r
  popd >/dev/null
}

check_cargo_folder_msrv() {
  msrv="$1"
  pushd "$2" >/dev/null
  cargo "+${msrv}" fmt --all -- --check
  cargo "+${msrv}" check --all-features --all-targets
  cargo "+${msrv}" test --all-features --all-targets -r
  cargo "+${msrv}" test --doc
  popd >/dev/null
}

check_cargo_msrv() {
  msrv="$1"
  check_cargo_folder_msrv "${msrv}" ./workspace
  # TODO: decide if test_workspaces should also only require rust 1.63.0.
}

check_shellcheck

add_vendor_config
check_cargo
check_cargo_msrv 1.63.0
remove_vendor_config

test -z "$(git status --porcelain=v1)"

echo "Success"
