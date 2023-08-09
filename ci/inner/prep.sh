#!/bin/bash

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

pushd ./workspace >/dev/null
# TODO: figure out how to cache properly
cargo vendor
popd >/dev/null
