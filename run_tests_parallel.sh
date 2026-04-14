#!/bin/bash
# Run cargo tests with 12 concurrent threads
# Usage: ./run_tests_parallel.sh [--test TEST_NAME] [TEST_FILTER]

export RUST_TEST_THREADS=12

cargo test "$@" -- --nocapture