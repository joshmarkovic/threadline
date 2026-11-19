#!/usr/bin/env bash
set -euo pipefail

fail() {
    printf '%s\n' "$1" >&2
    exit 2
}

assert_x64_pe() {
    local artifact=$1
    local byte_count pe_offset signature_first signature_second
    local pe_byte0 pe_byte1 pe_byte2 pe_byte3 pe_signature machine optional_header
    byte_count=$(wc -c < "$artifact")
    (( byte_count >= 96 )) || fail 'A package input is truncated.'
    read -r signature_first signature_second < <(od -An -tu1 -N2 "$artifact")
    [[ $signature_first == 77 && $signature_second == 90 ]] || fail 'A package input is not a Windows executable.'
    read -r pe_byte0 pe_byte1 pe_byte2 pe_byte3 < <(od -An -tu1 -j60 -N4 "$artifact")
    pe_offset=$((pe_byte0 + (pe_byte1 << 8) + (pe_byte2 << 16) + (pe_byte3 << 24)))
    (( pe_offset >= 64 && pe_offset + 26 <= byte_count )) || fail 'An executable header is outside its file.'
    read -r pe_signature < <(od -An -tx4 -j "$pe_offset" -N4 "$artifact")
    read -r machine < <(od -An -tx2 -j "$((pe_offset + 4))" -N2 "$artifact")
    read -r optional_header < <(od -An -tx2 -j "$((pe_offset + 24))" -N2 "$artifact")
    [[ $pe_signature == 00004550 && $machine == 8664 && $optional_header == 020b ]] ||
        fail 'Package inputs must be Windows x64 PE32+.'
}

case ${1:-} in
    --fixture)
        [[ $# == 6 ]] ||
            fail 'Usage: build-installer.sh --fixture APP.exe SETUP.exe VERSION BUILD SCHEMA_OR_UNKNOWN'
        channel=local-fixture
        shift
        ;;
    --release-stage)
        [[ $# == 8 ]] ||
            fail 'Usage: build-installer.sh --release-stage SIGNER_CONFIG VERIFIER.exe APP.exe UNSIGNED.exe VERSION BUILD SCHEMA'
        signer_config=$2
        verifier=$3
        shift 3
        channel=$(jq -er '.channel | strings | select(. == "stable" or . == "preview")' "$signer_config") ||
            fail 'Choose an explicit stable or preview release channel.'
        ;;
    *)
        fail 'Choose --fixture or --release-stage.'
        ;;
esac

payload=$1
output=$2
version=$3
build=$4
schema=$5
[[ -f $payload && -r $payload ]] || fail 'Specify a readable prebuilt Threadline executable.'
[[ ${output##*/} == *.exe ]] || fail 'Specify a setup output file ending in .exe.'
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][A-Za-z0-9][A-Za-z0-9.-]*)?$ ]] || fail 'Specify an explicit package version.'
[[ $build =~ ^[1-9][0-9]{0,8}$ ]] || fail 'Build ordinal must be a positive integer.'
if [[ $schema == unknown && $channel == local-fixture ]]; then
    schema=null
elif [[ ! $schema =~ ^[1-9][0-9]{0,8}$ ]]; then
    fail 'Provide a verified positive schema version. Local packaging-only fixtures also accept unknown.'
fi
fault=${THREADLINE_SETUP_FIXTURE_FAULT:-none}
private_public_key=${THREADLINE_PRIVATE_UPDATE_PUBLIC_KEY:-}
if [[ -n $private_public_key ]]; then
    [[ $channel == local-fixture && $private_public_key =~ ^04[0-9A-Fa-f]{128}$ && $fault == none && $schema != null ]] ||
        fail 'Use the uncompressed P-256 public key with a complete private update build.'
fi
compile_defines=()
if [[ $channel == local-fixture ]]; then
    compile_defines=(-DLOCAL_FIXTURE=1)
    case $fault in
        none) ;;
        stage-write) compile_defines+=(-DFIXTURE_FAIL_STAGING=1) ;;
        *) fail 'The only supported fixture fault is stage-write.' ;;
    esac
else
    [[ $fault == none ]] || fail 'A release stage cannot inject a staging fault.'
    [[ $channel != stable || $version != *-* ]] || fail 'A stable package cannot have a prerelease version.'
    [[ -f $verifier && -r $verifier ]] || fail 'Supply the built Windows Authenticode verifier.'
    publisher=$(jq -er '.publisher_display | strings | select(test("^[A-Za-z0-9][A-Za-z0-9(). &-]{0,119}$"))' "$signer_config") ||
        fail 'Supply an approved publisher display name.'
    publisher_pin=$(jq -er '.publisher_certificate_sha256 | strings | select(test("^[0-9a-f]{64}$"))' "$signer_config") ||
        fail 'Supply the approved publisher leaf certificate SHA-256.'
    signer_reference=$(jq -er '.signer_reference | strings | select(test("^CurrentUser/My/[0-9a-fA-F]{40}$"))' "$signer_config") ||
        fail 'Supply the approved signing certificate reference in CurrentUser/My.'
    expected_verifier=$(jq -er '.publisher_verifier_sha256 | strings | select(test("^[0-9a-f]{64}$"))' "$signer_config") ||
        fail 'Supply the recorded SHA-256 of the native verifier.'
    expected_app=$(jq -er '.signed_app_sha256 | strings | select(test("^[0-9a-f]{64}$"))' "$signer_config") ||
        fail 'Supply the recorded SHA-256 of the signed application.'
    artifact_url=$(jq -er '.artifact_url | strings | select(test("^https://[A-Za-z0-9][A-Za-z0-9.-]*(:443)?/[A-Za-z0-9._/-]+[.]exe$"))' "$signer_config") ||
        fail 'Supply the approved exact HTTPS artifact URL.'
    [[ /$artifact_url/ != */../* && /$artifact_url/ != */./* ]] || fail 'Use a canonical artifact URL.'
    oldest_schema=$(jq -er '.oldest_supported_schema | numbers | select(. > 0 and . < 1000000000 and . == floor)' "$signer_config") ||
        fail 'Supply the verified oldest schema supported for upgrades.'
    (( oldest_schema <= schema )) || fail 'The oldest supported schema exceeds the actual application schema.'
fi
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
assert_x64_pe "$staging/threadline.exe"
if [[ -n $private_public_key ]]; then
    LC_ALL=C grep -aiFq -- "$private_public_key" "$staging/threadline.exe" ||
        fail 'Build the Windows app with the same THREADLINE_PRIVATE_UPDATE_PUBLIC_KEY before packaging it.'
fi
read -r app_hash _ < <(sha256sum -- "$staging/threadline.exe")
if [[ $channel != local-fixture ]]; then
    [[ $app_hash == "$expected_app" ]] || fail 'The application does not match the recorded signed digest.'
    cp -- "$verifier" "$staging/verify-publisher.exe"
    assert_x64_pe "$staging/verify-publisher.exe"
    read -r verifier_hash _ < <(sha256sum -- "$staging/verify-publisher.exe")
    [[ $verifier_hash == "$expected_verifier" ]] || fail 'The native verifier does not match the recorded digest.'
    compile_defines+=("-DPACKAGE_PUBLISHER=$publisher" "-DPUBLISHER_PIN=$publisher_pin"
        "-DPUBLISHER_VERIFIER=$staging/verify-publisher.exe")
fi

notification_hash=null
if [[ $schema != null && $schema -ge 14 ]]; then
    activity_helper=${THREADLINE_NOTIFICATION_HELPER:-"$(dirname -- "$payload")/Threadline.NotificationRegistration.exe"}
    [[ -f $activity_helper && -r $activity_helper ]] ||
        fail 'Build Threadline.NotificationRegistration.exe with tools/windows/build-notification-helper.ps1 and place it beside the application or set THREADLINE_NOTIFICATION_HELPER.'
    cp -- "$activity_helper" "$staging/Threadline.NotificationRegistration.exe"
    assert_x64_pe "$staging/Threadline.NotificationRegistration.exe"
    read -r notification_sha _ < <(sha256sum -- "$staging/Threadline.NotificationRegistration.exe")
    if [[ $channel != local-fixture ]]; then
        expected_helper=$(jq -er '.notification_helper_sha256 | strings | select(test("^[0-9a-f]{64}$"))' "$signer_config") ||
            fail 'Supply the recorded SHA-256 of the notification helper.'
        [[ $notification_sha == "$expected_helper" ]] || fail 'The notification helper does not match its recorded digest.'
    fi
    notification_hash="\"$notification_sha\""
    compile_defines+=(-DREGISTER_ACTIVITY=1 "-DACTIVITY_HELPER=$staging/Threadline.NotificationRegistration.exe")
fi
"$makensis_path" -V3 -WX -INPUTCHARSET UTF8 \
    "${compile_defines[@]}" "-DPACKAGE_VERSION=$version" "-DPACKAGE_BUILD=$build" \
    "-DPAYLOAD_FILE=$staging/threadline.exe" "-DPAYLOAD_SHA256=$app_hash" \
    "-DINSTALLER_OUTPUT=$staging/threadline-package.exe" "$recipe_dir/threadline-installer.nsi"

read -r setup_hash _ < <(sha256sum -- "$staging/threadline-package.exe")
setup_size=$(wc -c < "$staging/threadline-package.exe")
if [[ $channel == local-fixture ]]; then
    printf '%s\n' '{' \
        "  \"package_version\": \"$version\"," \
        "  \"package_build\": $build," \
        '  "channel": "local-fixture",' \
        '  "target": "x86_64-pc-windows-msvc",' \
        '  "minimum_os": "Windows 11 x64 build 22000",' \
        '  "publisher_verification": "unsigned local fixture",' \
        '  "restart_protocol": 1,' \
        "  \"fixture_fault\": \"$fault\"," \
        "  \"current_schema\": $schema," \
        "  \"notification_helper_sha256\": $notification_hash," \
        "  \"size\": $setup_size," \
        "  \"sha256\": \"$setup_hash\"," \
        "  \"app_sha256\": \"$app_hash\"" \
        '}' > "$staging/setup.json"
    if [[ -n $private_public_key ]]; then
        jq --arg pin "${private_public_key,,}" '. + {private_update_public_key: $pin}' "$staging/setup.json" > "$staging/private.json"
        mv -- "$staging/private.json" "$staging/setup.json"
    fi
else
    jq -n --arg version "$version" --arg channel "$channel" --arg url "$artifact_url" \
        --arg publisher "$publisher" --arg pin "$publisher_pin" --arg signer "$signer_reference" \
        --arg unsignedHash "$setup_hash" --arg appHash "$app_hash" --arg verifierHash "$verifier_hash" \
        --argjson build "$build" --argjson size "$setup_size" --argjson schema "$schema" \
        --argjson oldest "$oldest_schema" --argjson notificationHash "$notification_hash" '{
            status: "publication_pending", target: "x86_64-pc-windows-msvc", minimum_os: "Windows 11 x64 build 22000",
            version: $version, channel: $channel, artifact_url: $url, package_build: $build, restart_protocol: 1,
            publisher_display: $publisher, publisher_certificate_sha256: $pin, signer_reference: $signer,
            current_schema: $schema, oldest_supported_schema: $oldest,
            unsigned_size: $size, unsigned_sha256: $unsignedHash, app_sha256: $appHash,
            publisher_verifier_sha256: $verifierHash, notification_helper_sha256: $notificationHash
        }' > "$staging/setup.json"
fi
mv -f -- "$staging/threadline-package.exe" "$output_parent/$output_name"
mv -f -- "$staging/setup.json" "$output_parent/$output_name.json"
printf 'Built %s\nManifest %s.json\n' "$output_parent/$output_name" "$output_parent/$output_name"
