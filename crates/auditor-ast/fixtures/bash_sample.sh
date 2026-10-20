#!/bin/bash
# A sample bash file

hello() {
    echo "Hello, world!"
}

function greet {
    local name=$1
    echo "Hello, $name!"
}
