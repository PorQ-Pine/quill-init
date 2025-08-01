#!/bin/bash

cd "$(dirname ${0})"
BINARY_NAME="qinit"
BINARY_PATH="/tmp/qinit"
ssh "root@${1}" -p "${2}" killall qinit
scp -P "${2}" "target/release/${BINARY_NAME}" "root@${1}:${BINARY_PATH}"
ssh "root@${1}" -t -p "${2}" 'env RUST_LOG=info SLINT_KMS_ROTATION=270 '"${BINARY_PATH}"''
