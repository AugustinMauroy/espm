#!/bin/bash

# Run cargo bench and capture the output
CARGO_BENCH_OUTPUT=$(cargo +nightly bench 2>&1)

# Use regex to extract benchmark results and convert to JSON
echo "$CARGO_BENCH_OUTPUT" | grep -oE 'test [^ ]+ .* bench: +[0-9,]+(\.[0-9]+)? ns/iter' | \
awk '{
    test_name = $2;
    for (i = 5; i < NF; i++) {
        if ($i ~ /^[0-9,]+(\.[0-9]+)?$/) {
            time = $i;
            gsub(/,/, "", time);
            break;
        }
    }
    if (time) {
        printf("{\"test\": \"%s\", \"time_ns\": %.2f}\n", test_name, time);
    }
}' | jq -s '.' > benchmark_results.json

# Show result
cat benchmark_results.json
