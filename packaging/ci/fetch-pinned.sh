#!/bin/sh
# Downloads a file a CI job needs and checks it against its pinned SHA-256 before anything runs it.
#
#   packaging/ci/fetch-pinned.sh URL SHA256 OUTPUT
#
# An empty SHA256 is a pin not made yet: the job stops and prints the download's checksum, to be
# checked against the publisher's release page and written into .gitlab-ci.yml. A mismatch stops
# the job too. Nothing is executed here.
set -eu
url="$1"
expected="$2"
out="$3"
curl --fail --silent --show-error --location --retry 3 --output "$out" "$url"
actual="$(sha256sum "$out" | cut -d' ' -f1)"
if [ -z "$expected" ]; then
  echo "error: no SHA-256 pinned for $url" >&2
  echo "       its SHA-256 is $actual; check it against the release and pin it in .gitlab-ci.yml" >&2
  rm -f "$out"
  exit 1
fi
if [ "$actual" != "$expected" ]; then
  echo "error: $url has SHA-256 $actual, not the pinned $expected" >&2
  rm -f "$out"
  exit 1
fi
echo "fetched $url ($actual)"
