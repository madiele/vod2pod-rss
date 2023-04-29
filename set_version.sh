#!/bin/sh

if [ -f version.txt ]; then
  VERSION=$(cat version.txt)
else
  VERSION=$(grep -oP '^version = "\K[0-9]+\.[0-9]+\.[0-9]+' Cargo.toml)
fi

sed "s/^version = .*$/version = \"$VERSION\"/" Cargo.toml > Cargo.toml.tmp
mv Cargo.toml.tmp Cargo.toml
