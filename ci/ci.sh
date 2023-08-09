#!/bin/bash

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

pushd podman/debian >/dev/null
image_id="$(podman build -q .)"
popd >/dev/null

# Allow network access during the preparation phase, but not the actual checking
# phase.

podman run \
  --rm \
  --init \
  --mount type=bind,src=.,dst=/work \
  "${image_id}" bash -l /work/ci/inner/prep.sh \
;

podman run \
  --rm \
  --init \
  --network none \
  --mount type=bind,src=.,dst=/work \
  "${image_id}" bash -l /work/ci/inner/run.sh \
;
