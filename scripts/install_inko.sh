#!/usr/bin/env bash

set -e

apk add --update make libffi libffi-dev rust cargo build-base git libgcc

if [[ ! -d inko ]]
then
    git clone https://github.com/inko-lang/inko.git inko
fi

cd inko
cargo build --release --features libffi-system
cd ..
