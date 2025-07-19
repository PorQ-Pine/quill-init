#!/bin/bash

cd "$(dirname ${0})"
BINARY_NAME="qinit"
BINARY_PATH="/tmp/qinit"
scp -P "${2}" "target/release/${BINARY_NAME}" "root@${1}:${BINARY_PATH}"
ssh "root@${1}" -p "${2}" 'killall -q "${BINARY_NAME}"; env RUST_LOG=info SLINT_KMS_ROTATION=270 '"${BINARY_PATH}"''
