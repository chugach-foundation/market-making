#!/bin/bash

if ./scripts/build.sh; then
    echo "Successfully built market making client. Running with default config at ./cfg/config.json."
    RUST_BACKTRACE=1 ./target/debug/rust_mm_client -c ./cfg/config.json
else 
    echo "Failed to build market making client."
fi