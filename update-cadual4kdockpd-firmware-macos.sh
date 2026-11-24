#!/usr/bin/env bash
set -euo pipefail

TARGET_VERSION="93.92"
TARGET_RAW="37778"
EXPECTED_L1_SHA256="9dc14b65f251de4a74aa8f5a613d90607a87333ff71fc26079eb82d4e7c69f6c"
EXPECTED_L2_SHA256="69ba8fe38a9c020bcca428bb795e10e177b2e5101fa58cfa2270c61df1ca3880"

ARCHIVE="${HOME}/Downloads/Patch_CADUAL4KDOCKPD.rar"
WORK_DIR="${HOME}/Downloads/Patch_CADUAL4KDOCKPD_firmware"
ASSUME_YES=0
DRY_RUN=1
NO_FLASH=0
FORCE=0
DID_FLASH=0
L1_DEVICE_ID="${L1_DEVICE_ID:-}"
L2_DEVICE_ID="${L2_DEVICE_ID:-}"

usage() {
  cat <<'EOF'
Usage:
  ./update-cadual4kdockpd-firmware-macos.sh [options]

Automates the macOS fwupdtool workflow for updating the i-tec CADUAL4KDOCKPD
Genesys Logic USB hub firmware from the vendor Patch_CADUAL4KDOCKPD.rar package.

Options:
  --archive PATH       Vendor RAR archive path
                       Default: ~/Downloads/Patch_CADUAL4KDOCKPD.rar
  --work-dir PATH      Extraction/work directory
                       Default: ~/Downloads/Patch_CADUAL4KDOCKPD_firmware
  --l1-device-id ID    Override device ID for Hub_1_9392_L1.ROM
  --l2-device-id ID    Override device ID for Hub_1_9392_L2.ROM
  --flash              Enable firmware writes. Without this, the script is a
                       dry run and only prints the fwupdtool flash commands.
  --dry-run            Explicitly keep the default no-write dry-run behavior
  --no-flash           Extract, inspect, and report versions only
  --force              Flash even if a hub already reports 93.92
  -y, --yes            Auto-confirm prompts
  -h, --help           Show this help

Environment overrides:
  L1_DEVICE_ID         Same as --l1-device-id
  L2_DEVICE_ID         Same as --l2-device-id

Required tools:
  brew install fwupd jq
  Extraction uses the first of bsdtar (ships with macOS), 7zz, 7z or unrar
  that can read the archive. No extractor is needed when ROM files matching
  the pinned SHA-256 values are already in the work dir.
EOF
}

die() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

step() {
  printf '\n==> %s\n' "$*"
}

note() {
  printf '    %s\n' "$*"
}

confirm() {
  local prompt="$1"
  local reply

  if [ "$ASSUME_YES" -eq 1 ]; then
    note "Auto-confirmed: $prompt"
    return 0
  fi

  printf '%s [y/N] ' "$prompt"
  read -r reply
  case "$reply" in
    y|Y|yes|YES) return 0 ;;
    *) return 1 ;;
  esac
}

confirm_update_phrase() {
  local reply

  if [ "$ASSUME_YES" -eq 1 ]; then
    note "Auto-confirmed firmware-write prompt"
    return 0
  fi

  printf 'Type UPDATE to begin firmware writes: '
  read -r reply
  [ "$reply" = "UPDATE" ]
}

parse_args() {
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --archive)
        [ "$#" -ge 2 ] || die "--archive requires a path"
        ARCHIVE="$2"
        shift 2
        ;;
      --work-dir)
        [ "$#" -ge 2 ] || die "--work-dir requires a path"
        WORK_DIR="$2"
        shift 2
        ;;
      --l1-device-id)
        [ "$#" -ge 2 ] || die "--l1-device-id requires an ID"
        L1_DEVICE_ID="$2"
        shift 2
        ;;
      --l2-device-id)
        [ "$#" -ge 2 ] || die "--l2-device-id requires an ID"
        L2_DEVICE_ID="$2"
        shift 2
        ;;
      --flash)
        DRY_RUN=0
        NO_FLASH=0
        shift
        ;;
      --dry-run)
        DRY_RUN=1
        shift
        ;;
      --no-flash)
        NO_FLASH=1
        DRY_RUN=1
        shift
        ;;
      --force)
        FORCE=1
        shift
        ;;
      -y|--yes)
        ASSUME_YES=1
        shift
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        die "Unknown option: $1"
        ;;
    esac
  done
}

check_dependencies() {
  local missing=""
  local cmd

  for cmd in fwupdtool jq shasum; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
      missing="$missing $cmd"
    fi
  done

  if [ -n "$missing" ]; then
    die "Missing required tools:$missing. Install with: brew install fwupd jq"
  fi
}

hash_file() {
  shasum -a 256 "$1" | awk '{print $1; exit}'
}

roms_valid() {
  [ -f "$L1_ROM" ] && [ -f "$L2_ROM" ] &&
    [ "$(hash_file "$L1_ROM")" = "$EXPECTED_L1_SHA256" ] &&
    [ "$(hash_file "$L2_ROM")" = "$EXPECTED_L2_SHA256" ]
}

# extract_members ARCHIVE DEST MEMBER...: try each installed extractor in turn.
# A tool that is installed may still fail (Homebrew 7zz cannot decode this RAR).
extract_members() {
  local archive="$1"
  local dest="$2"
  local tool
  shift 2

  for tool in bsdtar 7zz 7z unrar; do
    command -v "$tool" >/dev/null 2>&1 || continue
    case "$tool" in
      bsdtar) bsdtar -xf "$archive" -C "$dest" "$@" ;;
      unrar) unrar x -o+ "$archive" "$@" "$dest/" ;;
      *) "$tool" x -y "-o$dest" "$archive" "$@" >/dev/null ;;
    esac && { note "Extracted with $tool"; return 0; }
    note "$tool could not extract $archive; trying the next tool"
  done

  die "No working extractor for $archive. Tried bsdtar, 7zz, 7z, unrar; install one with: brew install unrar"
}

extract_firmware() {
  OUTER_DIR="$WORK_DIR/outer"
  ROM_DIR="$WORK_DIR/roms"
  OUTER_EXE="$OUTER_DIR/Patch_CADUAL4KDOCKPD/CADUAL4KDOCKPD_9392.exe"
  L1_ROM="$ROM_DIR/Hub_1_9392_L1.ROM"
  L2_ROM="$ROM_DIR/Hub_1_9392_L2.ROM"

  if roms_valid; then
    step "Using previously extracted ROM files"
    note "$L1_ROM"
    note "$L2_ROM"
    return 0
  fi

  [ -f "$ARCHIVE" ] || die "Archive not found: $ARCHIVE"

  mkdir -p "$OUTER_DIR" "$ROM_DIR"

  # Always redo both levels: a failed extractor can leave a truncated file behind.
  step "Extracting Windows updater from vendor RAR"
  extract_members "$ARCHIVE" "$OUTER_DIR" Patch_CADUAL4KDOCKPD/CADUAL4KDOCKPD_9392.exe

  step "Extracting raw Genesys ROM files from updater"
  extract_members "$OUTER_EXE" "$ROM_DIR" Hub_1_9392_L1.ROM Hub_1_9392_L2.ROM
}

verify_hashes() {
  local l1_sha
  local l2_sha

  step "Verifying firmware hashes"
  l1_sha="$(hash_file "$L1_ROM")"
  l2_sha="$(hash_file "$L2_ROM")"

  printf '    Hub_1_9392_L1.ROM  %s\n' "$l1_sha"
  printf '    Hub_1_9392_L2.ROM  %s\n' "$l2_sha"

  [ "$l1_sha" = "$EXPECTED_L1_SHA256" ] || die "Unexpected SHA256 for $L1_ROM"
  [ "$l2_sha" = "$EXPECTED_L2_SHA256" ] || die "Unexpected SHA256 for $L2_ROM"
}

parse_roms() {
  step "Checking ROM metadata with fwupdtool"
  fwupdtool --plugins genesys firmware-parse "$L1_ROM" genesys-usbhub >/dev/null
  fwupdtool --plugins genesys firmware-parse "$L2_ROM" genesys-usbhub >/dev/null
  note "Both ROMs parsed as Genesys USB hub firmware"
  note "Target version: $TARGET_VERSION / raw $TARGET_RAW / hex 0x9392"
}

refresh_devices() {
  local attempt
  local err_file="$TMP_DIR/fwupd-get-devices.err"

  for attempt in 1 2 3 4 5; do
    if fwupdtool --plugins genesys get-devices --show-all --json >"$DEVICES_JSON" 2>"$err_file"; then
      return 0
    fi
    note "fwupdtool did not return devices on attempt $attempt; waiting before retry"
    if [ -s "$DEVICES_JSON" ]; then
      sed 's/^/    /' "$DEVICES_JSON" >&2
    fi
    if [ -s "$err_file" ]; then
      sed 's/^/    /' "$err_file" >&2
    fi
    sleep 2
  done

  die "fwupdtool could not detect stable Genesys hub devices"
}

device_id_for_hub() {
  local hub="$1"
  jq -r --arg hub "$hub" '
    .Devices[]?
    | select(((.InstanceIds // []) | join(" ")) | contains($hub))
    | .DeviceId
  ' "$DEVICES_JSON" | awk 'NF {print; exit}'
}

device_version_for_id() {
  local device_id="$1"
  jq -r --arg id "$device_id" '
    .Devices[]?
    | select(.DeviceId == $id)
    | .Version // "unknown"
  ' "$DEVICES_JSON" | awk 'NF {print; exit}'
}

print_device_report() {
  jq -r '
    .Devices[]?
    | select(((.InstanceIds // []) | join(" ")) | contains("VID_05E3&PID_0610"))
    | [
        (.InstanceIds // [] | map(select(contains("HUB_"))) | join(",")),
        .DeviceId,
        (.Version // "unknown"),
        (.VersionRaw // "unknown")
      ]
    | @tsv
  ' "$DEVICES_JSON" |
  while IFS="$(printf '\t')" read -r hub device_id version raw; do
    [ -n "$hub" ] || hub="unknown-hub"
    printf '    %-10s version=%-6s raw=%-6s id=%s\n' "$hub" "$version" "$raw" "$device_id"
  done
}

prompt_missing_device_id() {
  local label="$1"
  local hub="$2"
  local reply

  if [ "$ASSUME_YES" -eq 1 ]; then
    die "Could not auto-detect $label device ID for $hub"
  fi

  printf 'Could not auto-detect %s (%s). Paste its DeviceId, or leave blank to abort: ' "$label" "$hub"
  read -r reply
  [ -n "$reply" ] || die "No device ID supplied for $label"
  printf '%s\n' "$reply"
}

discover_devices() {
  step "Finding updateable Genesys hub devices"
  refresh_devices
  print_device_report

  if [ -z "$L1_DEVICE_ID" ]; then
    L1_DEVICE_ID="$(device_id_for_hub HUB_16 || true)"
  fi
  if [ -z "$L2_DEVICE_ID" ]; then
    L2_DEVICE_ID="$(device_id_for_hub HUB_00 || true)"
  fi

  if [ -z "$L1_DEVICE_ID" ]; then
    L1_DEVICE_ID="$(prompt_missing_device_id "L1/root hub" "HUB_16")"
  fi
  if [ -z "$L2_DEVICE_ID" ]; then
    L2_DEVICE_ID="$(prompt_missing_device_id "L2/downstream hub" "HUB_00")"
  fi
}

print_plan() {
  local l1_version
  local l2_version

  l1_version="$(device_version_for_id "$L1_DEVICE_ID")"
  l2_version="$(device_version_for_id "$L2_DEVICE_ID")"

  step "Planned firmware mapping"
  printf '    L1/root:       %s -> %s  current=%s target=%s\n' "$L1_ROM" "$L1_DEVICE_ID" "$l1_version" "$TARGET_VERSION"
  printf '    L2/downstream: %s -> %s  current=%s target=%s\n' "$L2_ROM" "$L2_DEVICE_ID" "$l2_version" "$TARGET_VERSION"
}

needs_flash() {
  local current_version="$1"
  [ "$FORCE" -eq 1 ] || [ "$current_version" != "$TARGET_VERSION" ]
}

flash_one() {
  local label="$1"
  local rom="$2"
  local device_id="$3"
  local current_version
  local rc
  local after_version

  current_version="$(device_version_for_id "$device_id")"

  if ! needs_flash "$current_version"; then
    note "$label already reports $TARGET_VERSION; skipping"
    return 0
  fi

  if [ "$NO_FLASH" -eq 1 ]; then
    note "No-flash mode: would update $label from $current_version to $TARGET_VERSION"
    return 0
  fi

  if [ "$DRY_RUN" -eq 1 ]; then
    note "Dry run: skipping firmware write for $label"
    printf '    fwupdtool --plugins genesys --prepare --cleanup -y install-blob %s %s %s\n' "$rom" "$device_id" "$TARGET_VERSION"
    return 0
  fi

  step "Flashing $label"
  set +e
  fwupdtool --plugins genesys --prepare --cleanup -y install-blob "$rom" "$device_id" "$TARGET_VERSION"
  rc=$?
  set -e

  refresh_devices
  after_version="$(device_version_for_id "$device_id")"

  if [ "$rc" -ne 0 ]; then
    if [ "$after_version" = "$TARGET_VERSION" ]; then
      DID_FLASH=1
      note "fwupdtool returned $rc after write/verify, but $label now reports $TARGET_VERSION"
      note "This matches the known macOS attach/reset I/O error after successful verification."
      return 0
    fi
    die "$label flash failed with exit $rc and now reports version $after_version"
  fi

  [ "$after_version" = "$TARGET_VERSION" ] || die "$label flash ended without target version; current=$after_version"
  DID_FLASH=1
  note "$label now reports $TARGET_VERSION"
}

flash_firmware() {
  local l1_version
  local l2_version

  l1_version="$(device_version_for_id "$L1_DEVICE_ID")"
  l2_version="$(device_version_for_id "$L2_DEVICE_ID")"

  if ! needs_flash "$l1_version" && ! needs_flash "$l2_version"; then
    step "No firmware write needed"
    note "Both hub layers already report $TARGET_VERSION"
    return 0
  fi

  if [ "$DRY_RUN" -eq 0 ] && [ "$NO_FLASH" -eq 0 ]; then
    step "Final safety check"
    note "Leave dock power and USB-C connected."
    note "Do not unplug anything while erase/write/verify is running."
    confirm "Continue to firmware write preparation?" || die "Aborted by user"
    confirm_update_phrase || die "Firmware write not confirmed"
  fi

  flash_one "L1/root hub" "$L1_ROM" "$L1_DEVICE_ID"
  flash_one "L2/downstream hub" "$L2_ROM" "$L2_DEVICE_ID"
}

final_report() {
  step "Final firmware report"
  refresh_devices
  print_device_report

  if [ "$DID_FLASH" -eq 1 ]; then
    step "Power-cycle the dock"
    note "1. Unplug USB-C from the Mac."
    note "2. Remove power from the dock."
    note "3. Wait about 10 seconds."
    note "4. Reconnect dock power."
    note "5. Reconnect USB-C to the Mac."
    note "Then re-run this script with --no-flash to confirm both hub layers still report $TARGET_VERSION."
  else
    step "No firmware write happened in this run"
    note "No power-cycle is required by this script run."
  fi
}

main() {
  TMP_DIR="$(mktemp -d)"
  DEVICES_JSON="$TMP_DIR/devices.json"
  trap 'rm -rf "$TMP_DIR"' EXIT

  parse_args "$@"

  step "CADUAL4KDOCKPD macOS firmware updater"
  note "Archive:  $ARCHIVE"
  note "Work dir: $WORK_DIR"
  note "Target:   $TARGET_VERSION / raw $TARGET_RAW / hex 0x9392"
  if [ "$NO_FLASH" -eq 1 ]; then
    note "Mode:     no-flash report only"
  elif [ "$DRY_RUN" -eq 1 ]; then
    note "Mode:     dry run, no firmware writes; pass --flash to enable writes"
  else
    note "Mode:     firmware writes enabled by --flash"
  fi

  check_dependencies
  confirm "Start extraction and device inspection?" || die "Aborted by user"
  extract_firmware
  verify_hashes
  parse_roms
  discover_devices
  print_plan
  flash_firmware
  final_report
}

main "$@"
