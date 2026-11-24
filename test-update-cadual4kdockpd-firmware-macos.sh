#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$ROOT_DIR/update-cadual4kdockpd-firmware-macos.sh"

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

assert_contains() {
  local haystack="$1"
  local needle="$2"
  printf '%s' "$haystack" | grep -F -- "$needle" >/dev/null || fail "expected output to contain: $needle"
}

make_mock_bin() {
  local dir="$1"

  cat >"$dir/fwupdtool" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
for arg in "$@"; do
  if [ "$arg" = "get-plugins" ]; then
    printf 'genesys\n'
    exit 0
  fi
done
if printf '%s\n' "$*" | grep -F 'firmware-parse' >/dev/null; then
  printf '<firmware><version>93.92</version></firmware>\n'
  exit 0
fi
if printf '%s\n' "$*" | grep -F 'get-devices' >/dev/null; then
  cat <<'JSON'
{
  "Devices": [
    {
      "Name": "USB2.1 Hub",
      "DeviceId": "mock-l1",
      "InstanceIds": ["USB\\VID_05E3&PID_0610", "USB\\VID_05E3&PID_0610&HUB_16"],
      "Version": "93.5",
      "VersionRaw": 37637
    },
    {
      "Name": "USB2.1 Hub",
      "DeviceId": "mock-l2",
      "InstanceIds": ["USB\\VID_05E3&PID_0610", "USB\\VID_05E3&PID_0610&HUB_00"],
      "Version": "93.92",
      "VersionRaw": 37778
    }
  ]
}
JSON
  exit 0
fi
if printf '%s\n' "$*" | grep -F 'install-blob' >/dev/null; then
  printf 'install-blob should not run in dry-run\n' >&2
  exit 9
fi
printf 'unexpected fwupdtool args: %s\n' "$*" >&2
exit 2
MOCK

  # One mock for every extractor: only $MOCK_EXTRACTOR works, each call is logged.
  cat >"$dir/bsdtar" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
tool="$(basename "$0")"
MOCK_ARGS="$*"
printf '%s\n' "$tool" >>"$MOCK_LOG"
[ "$tool" = "${MOCK_EXTRACTOR:-}" ] || exit 1
dest=""
case "$tool" in
  bsdtar) while [ "$#" -gt 0 ]; do [ "$1" = "-C" ] && dest="$2"; shift; done ;;
  unrar) dest="${*: -1}" ;;
  *) for arg in "$@"; do case "$arg" in -o*) dest="${arg#-o}" ;; esac; done ;;
esac
mkdir -p "$dest"
case "$MOCK_ARGS" in
  *Hub_1_9392_L1.ROM*)
    printf 'l1' >"$dest/Hub_1_9392_L1.ROM"
    printf 'l2' >"$dest/Hub_1_9392_L2.ROM"
    ;;
  *CADUAL4KDOCKPD_9392.exe*)
    mkdir -p "$dest/Patch_CADUAL4KDOCKPD"
    : >"$dest/Patch_CADUAL4KDOCKPD/CADUAL4KDOCKPD_9392.exe"
    ;;
esac
MOCK
  cp "$dir/bsdtar" "$dir/7zz"
  cp "$dir/bsdtar" "$dir/7z"
  cp "$dir/bsdtar" "$dir/unrar"

  cat >"$dir/shasum" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
for path in "${@: -2}"; do
  case "$path" in
    *Hub_1_9392_L1.ROM)
      printf '9dc14b65f251de4a74aa8f5a613d90607a87333ff71fc26079eb82d4e7c69f6c  %s\n' "$path"
      ;;
    *Hub_1_9392_L2.ROM)
      printf '69ba8fe38a9c020bcca428bb795e10e177b2e5101fa58cfa2270c61df1ca3880  %s\n' "$path"
      ;;
  esac
done
MOCK

  chmod +x "$dir"/*
}

run_help_test() {
  local output
  output="$("$SCRIPT" --help)"
  assert_contains "$output" "Usage:"
  assert_contains "$output" "--dry-run"
  assert_contains "$output" "--flash"
  assert_contains "$output" "Patch_CADUAL4KDOCKPD.rar"
  assert_contains "$output" "bsdtar"
}

# run_script EXTRACTOR [script args...]: sets $tmp, $log and $output; never fails itself.
run_script() {
  local mock_bin
  tmp="$(mktemp -d)"
  log="$tmp/extractors.log"
  mock_bin="$tmp/bin"
  mkdir -p "$mock_bin"
  make_mock_bin "$mock_bin"
  : >"$log"
  : >"$tmp/Patch_CADUAL4KDOCKPD.rar"
  status=0
  output="$(MOCK_LOG="$log" MOCK_EXTRACTOR="$1" PATH="$mock_bin:/usr/bin:/bin" \
    "$SCRIPT" --archive "$tmp/Patch_CADUAL4KDOCKPD.rar" --work-dir "$tmp/work" --yes "${@:2}" 2>&1)" || status=$?
}

assert_dry_run_ok() {
  [ "$status" -eq 0 ] || fail "script exited $status: $output"
  assert_contains "$output" "Dry run: skipping firmware write"
  assert_contains "$output" "HUB_16"
  assert_contains "$output" "mock-l1"
  assert_contains "$output" "already reports 93.92; skipping"
}

# Each extractor works on its own, after every tool before it has failed.
run_extractor_fallback_test() {
  local tool
  for tool in bsdtar 7zz 7z unrar; do
    run_script "$tool"
    assert_dry_run_ok
    assert_contains "$output" "Extracted with $tool"
    [ "$(tail -n 1 "$log")" = "$tool" ] || fail "$tool was not the last extractor tried"
    [ -f "$tmp/work/roms/Hub_1_9392_L2.ROM" ] || fail "$tool did not extract the ROMs"
  done
  [ "$(tr '\n' ' ' <"$log")" = "bsdtar 7zz 7z unrar bsdtar 7zz 7z unrar " ] ||
    fail "wrong order for unrar fallback: $(tr '\n' ' ' <"$log")"
}

run_no_working_extractor_test() {
  run_script none
  [ "$status" -ne 0 ] || fail "expected failure when no extractor works"
  assert_contains "$output" "No working extractor"
}

# Valid ROMs in the work dir: no extractor is called and no archive is needed.
run_preextracted_roms_test() {
  local mock_bin
  tmp="$(mktemp -d)"
  log="$tmp/extractors.log"
  mock_bin="$tmp/bin"
  mkdir -p "$mock_bin" "$tmp/work/roms"
  make_mock_bin "$mock_bin"
  : >"$log"
  printf 'l1' >"$tmp/work/roms/Hub_1_9392_L1.ROM"
  printf 'l2' >"$tmp/work/roms/Hub_1_9392_L2.ROM"
  status=0
  output="$(MOCK_LOG="$log" MOCK_EXTRACTOR=none PATH="$mock_bin:/usr/bin:/bin" \
    "$SCRIPT" --archive "$tmp/missing.rar" --work-dir "$tmp/work" --yes 2>&1)" || status=$?
  assert_dry_run_ok
  assert_contains "$output" "Using previously extracted ROM files"
  [ ! -s "$log" ] || fail "extractor was called despite valid ROMs: $(cat "$log")"
}

# ROMs that fail the pinned hash are never used, even without --flash.
run_bad_hash_test() {
  local mock_bin
  tmp="$(mktemp -d)"
  mock_bin="$tmp/bin"
  mkdir -p "$mock_bin" "$tmp/work/roms"
  make_mock_bin "$mock_bin"
  rm "$mock_bin/shasum"
  printf 'l1' >"$tmp/work/roms/Hub_1_9392_L1.ROM"
  printf 'l2' >"$tmp/work/roms/Hub_1_9392_L2.ROM"
  status=0
  output="$(MOCK_LOG="$tmp/log" MOCK_EXTRACTOR=bsdtar PATH="$mock_bin:/usr/bin:/bin" \
    "$SCRIPT" --archive "$tmp/missing.rar" --work-dir "$tmp/work" --yes 2>&1)" || status=$?
  [ "$status" -ne 0 ] || fail "expected failure for ROMs with wrong hashes"
  assert_contains "$output" "Archive not found"
  : >"$tmp/missing.rar"
  status=0
  output="$(MOCK_LOG="$tmp/log" MOCK_EXTRACTOR=bsdtar PATH="$mock_bin:/usr/bin:/bin" \
    "$SCRIPT" --archive "$tmp/missing.rar" --work-dir "$tmp/work" --yes 2>&1)" || status=$?
  [ "$status" -ne 0 ] || fail "expected failure for extracted ROMs with wrong hashes"
  assert_contains "$output" "Unexpected SHA256"
}

run_help_test
run_extractor_fallback_test
run_no_working_extractor_test
run_preextracted_roms_test
run_bad_hash_test

printf 'ok\n'
