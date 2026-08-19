#!/usr/bin/env bash
# Builds the CLI tools through Buck and packages them into the release archive.
#
# The set of tools, and the name each ships under, come from
# build-support/release-tools.json rather than from whatever Buck happened to
# emit. The archive is verified against that list before this exits, so a
# renamed or dropped tool fails the build instead of quietly changing the
# release.
#
# patchelf is taken from PATCHELF, defaulting to the Nix devShell's, because
# only the Linux release job links through Nix.
#
# Usage: package-tools.sh <slug> <buck2-command...>
set -euo pipefail

: "${PATCHELF:=nix develop --command patchelf}"

slug="$1"
shift
if [ "$#" -eq 0 ]; then
  echo "usage: $0 <slug> <buck2-command...>" >&2
  exit 2
fi

spec=build-support/release-tools.json
zip_name="wows_toolkit_tools_${slug}_linux64.zip"

mapfile -t targets < <(python3 -c '
import json, sys
spec = json.load(open(sys.argv[1]))
for tool in spec["tools"]:
    print(tool["target"])
' "$spec")

outputs_json=$("$@" build -c native_build.mode=release --show-json-output "${targets[@]}")

rm -rf artifacts "$zip_name"
mkdir -p artifacts

# Emits the archive-relative name of everything it stages, for the check below.
python3 - "$spec" "$outputs_json" <<'PY' > expected-contents.txt
import json, os, shutil, sys

spec = json.load(open(sys.argv[1]))
# "//:wowsunpack" on a command line and "root//:wowsunpack" in buck2's JSON are
# the same target; compare the part after the cell.
def key(label):
    _, sep, rest = label.partition("//")
    return rest if sep else label

outputs = {key(k): v for k, v in json.loads(sys.argv[2]).items()}

for tool in spec["tools"]:
    built = outputs.get(key(tool["target"]))
    if not built:
        sys.exit("buck2 reported no output for {}.".format(tool["target"]))
    name = tool["ship"] + os.path.splitext(built)[1]
    shutil.copy2(built, os.path.join("artifacts", name))
    print(name)

for extra in spec["extra_files"]:
    shutil.copy2(extra, os.path.join("artifacts", os.path.basename(extra)))
    print(os.path.basename(extra))
PY

chmod +w artifacts/*

# Linking through Nix records a store path as the interpreter, and that path
# exists only on the builder. patchelf only accepts ELF files, and the staging
# directory also holds the manifest that ships beside the binaries.
for f in artifacts/*; do
  if [ "$(head -c 4 "$f" | od -An -tx1 | tr -d ' \n')" != "7f454c46" ]; then
    continue
  fi
  $PATCHELF --set-interpreter /lib64/ld-linux-x86-64.so.2 --remove-rpath "$f"
done

zip -j "$zip_name" artifacts/*

diff <(tr -d '\r' < expected-contents.txt | sort) <(unzip -Z1 "$zip_name" | sort) || {
  echo "$zip_name does not match $spec." >&2
  exit 1
}
rm -f expected-contents.txt

echo "$zip_name contains:"
unzip -Z1 "$zip_name"
