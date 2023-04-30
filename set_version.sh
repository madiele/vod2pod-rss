#!/bin/sh

if [ -f version.txt ]; then
  cat version.txt
  VERSION=$(cat version.txt)
else
  VERSION=$(grep -oP '^version = "\K[0-9]+\.[0-9]+\.[0-9]+' Cargo.toml)
fi

sed "s/^version = .*$/version = \"$VERSION\"/" Cargo.toml > Cargo.toml.tmp
VERSION_HTML="<small class=\"text-muted\">$VERSION<\/small>"
sed "s/<\!-- ###VERSION### -->/$VERSION_HTML/" templates/index.html > templates/index.html.tmp
mv Cargo.toml.tmp Cargo.toml
mv templates/index.html.tmp templates/index.html
