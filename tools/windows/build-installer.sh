#!/usr/bin/env bash
set -euo pipefail

fail() {
    printf '%s\n' "$1" >&2
    exit 2
}

if [[ $# != 6 || $1 != --fixture ]]; then
    fail 'Usage: build-installer.sh --fixture APP.exe SETUP.exe PACKAGE_VERSION BUILD_ORDINAL SCHEMA_VERSION_OR_UNKNOWN'
fi

payload=$2
output=$3
version=$4
build=$5
schema=$6
[[ -f $payload && -r $payload ]] || fail 'Specify a readable prebuilt Threadline executable.'
[[ ${output##*/} == *.exe ]] || fail 'Specify a setup output file ending in .exe.'
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][A-Za-z0-9][A-Za-z0-9.-]*)?$ ]] || fail 'Specify an explicit package version.'
[[ $build =~ ^[1-9][0-9]{0,8}$ ]] || fail 'Build ordinal must be a positive integer.'
if [[ $schema == unknown ]]; then
    schema=null
elif [[ ! $schema =~ ^[1-9][0-9]{0,8}$ ]]; then
    fail 'Provide a verified positive schema version, or unknown for packaging-only inputs.'
fi
fault=${THREADLINE_SETUP_FIXTURE_FAULT:-none}
compile_fault=()
case $fault in
    none) ;;
    stage-write) compile_fault=(-DFIXTURE_FAIL_STAGING=1) ;;
    *) fail 'The only supported fixture fault is stage-write.' ;;
esac
[[ -n ${NSISDIR:-} && -d $NSISDIR ]] || fail 'Set NSISDIR to the pinned NSIS data directory.'
makensis_path=${MAKENSIS:-makensis}
nsis_version=$("$makensis_path" -VERSION)
[[ $nsis_version == v3.11* ]] || fail 'Use the pinned NSIS 3.11 compiler.'

output_parent=$(dirname -- "$output")
mkdir -p -- "$output_parent"
output_parent=$(realpath -- "$output_parent")
output_name=$(basename -- "$output")
recipe_dir=$(dirname -- "$(realpath -- "$0")")
staging=$(mktemp -d -- "$output_parent/.threadline-installer.XXXXXXXX")
trap 'rm -rf -- "$staging"' EXIT
cp -- "$payload" "$staging/threadline.exe"

# Validate PE32+ and AMD64 from the copied bytes before packaging.
byte_count=$(wc -c < "$staging/threadline.exe")
(( byte_count >= 96 )) || fail 'The application payload is truncated.'
read -r signature_first signature_second < <(od -An -tu1 -N2 "$staging/threadline.exe")
[[ $signature_first == 77 && $signature_second == 90 ]] || fail 'The application payload is not a Windows executable.'
read -r pe_byte0 pe_byte1 pe_byte2 pe_byte3 < <(od -An -tu1 -j60 -N4 "$staging/threadline.exe")
pe_offset=$((pe_byte0 + (pe_byte1 << 8) + (pe_byte2 << 16) + (pe_byte3 << 24)))
(( pe_offset >= 64 && pe_offset + 26 <= byte_count )) || fail 'The Windows executable header is outside the application payload.'
read -r pe_signature machine < <(od -An -tx4 -j "$pe_offset" -N4 "$staging/threadline.exe" | {
    read -r pe_signature
    read -r machine < <(od -An -tx2 -j "$((pe_offset + 4))" -N2 "$staging/threadline.exe")
    printf '%s %s\n' "$pe_signature" "$machine"
})
read -r optional_header < <(od -An -tx2 -j "$((pe_offset + 24))" -N2 "$staging/threadline.exe")
[[ $pe_signature == 00004550 && $machine == 8664 && $optional_header == 020b ]] || fail 'The application payload must be Windows x64 PE32+.'

read -r app_hash _ < <(sha256sum -- "$staging/threadline.exe")
"$makensis_path" -V3 -WX -INPUTCHARSET UTF8 \
    -DLOCAL_FIXTURE=1 "${compile_fault[@]}" "-DPACKAGE_VERSION=$version" "-DPACKAGE_BUILD=$build" \
    "-DPAYLOAD_FILE=$staging/threadline.exe" "-DPAYLOAD_SHA256=$app_hash" \
    "-DINSTALLER_OUTPUT=$staging/Threadline-Installer-Test.exe" "$recipe_dir/threadline-installer.nsi"

read -r setup_hash _ < <(sha256sum -- "$staging/Threadline-Installer-Test.exe")
setup_size=$(wc -c < "$staging/Threadline-Installer-Test.exe")
printf '%s\n' '{' \
    "  \"package_version\": \"$version\"," \
    "  \"package_build\": $build," \
    '  "channel": "local-fixture",' \
    '  "target": "x86_64-pc-windows-msvc",' \
    '  "minimum_os": "Windows 11 x64 build 22000",' \
    '  "publisher_verification": "unsigned local fixture",' \
    "  \"fixture_fault\": \"$fault\"," \
    "  \"current_schema\": $schema," \
    "  \"size\": $setup_size," \
    "  \"sha256\": \"$setup_hash\"," \
    "  \"app_sha256\": \"$app_hash\"" \
    '}' > "$staging/setup.json"
mv -f -- "$staging/Threadline-Installer-Test.exe" "$output_parent/$output_name"
mv -f -- "$staging/setup.json" "$output_parent/$output_name.json"
printf 'Built %s\nManifest %s.json\n' "$output_parent/$output_name" "$output_parent/$output_name"
