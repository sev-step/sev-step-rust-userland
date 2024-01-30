#!/bin/bash

# Find all build.sh files in immediate subdirectories only
find . -mindepth 2 -maxdepth 2 -type f -name "build.sh" | while read -r file; do
    # Get the directory of the build.sh file
    dir=$(dirname "$file")

    # Pushd to enter the directory, execute the script, and popd to return
    echo "Building ${dir}"
    pushd "$dir" > /dev/null || exit
    ./build.sh
    popd > /dev/null || exit
done
