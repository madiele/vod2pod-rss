#!/bin/bash
# specify the path to the TOML file
TOML_FILE="Cargo.toml"

# create a temporary file to store the modified TOML code
TMP_FILE=$(mktemp)

# use sed to delete the [dev-dependencies] section from the TOML file
VERSION=$(grep -oP '^version = "\K[0-9]+\.[0-9]+\.[0-9]+' $TOML_FILE)
sed '/\[dev-dependencies\]/,/^$/d' $TOML_FILE | sed 's/^version = .*$/version = "0\.0\.1"/' > "$TMP_FILE"
echo "$0: version found $VERSION"
echo "$VERSION" > version.txt
cat version.txt

# print the modified TOML code to the console
cat "$TMP_FILE" > "$TOML_FILE"

# remove the temporary file
rm "$TMP_FILE"

