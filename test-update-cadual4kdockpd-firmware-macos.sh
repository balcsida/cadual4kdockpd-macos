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

  cat >"$dir/unrar" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
dest="${@: -1}"
mkdir -p "$dest"
case "$*" in
  *CADUAL4KDOCKPD_9392.exe*)
    mkdir -p "$dest/Patch_CADUAL4KDOCKPD"
    : >"$dest/Patch_CADUAL4KDOCKPD/CADUAL4KDOCKPD_9392.exe"
    ;;
  *Hub_1_9392_L1.ROM*)
    printf 'l1' >"$dest/Hub_1_9392_L1.ROM"
    printf 'l2' >"$dest/Hub_1_9392_L2.ROM"
    ;;
esac
MOCK

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

  chmod +x "$dir/fwupdtool" "$dir/unrar" "$dir/shasum"
}

run_help_test() {
  local output
  output="$("$SCRIPT" --help)"
  assert_contains "$output" "Usage:"
  assert_contains "$output" "--dry-run"
  assert_contains "$output" "--flash"
  assert_contains "$output" "Patch_CADUAL4KDOCKPD.rar"
}

run_default_dry_run_test() {
  local tmp mock_bin archive output
  tmp="$(mktemp -d)"
  mock_bin="$tmp/bin"
  mkdir -p "$mock_bin"
  make_mock_bin "$mock_bin"
  archive="$tmp/Patch_CADUAL4KDOCKPD.rar"
  : >"$archive"

  output="$(PATH="$mock_bin:/usr/bin:/bin" "$SCRIPT" --archive "$archive" --work-dir "$tmp/work" --yes)"

  assert_contains "$output" "Dry run: skipping firmware write"
  assert_contains "$output" "HUB_16"
  assert_contains "$output" "mock-l1"
  assert_contains "$output" "already reports 93.92; skipping"
}

run_help_test
run_default_dry_run_test

printf 'ok\n'
