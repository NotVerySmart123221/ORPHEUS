#!/usr/bin/env bash
set -euo pipefail

RUNTIME=""
TOOLCHAIN="stable"
TOOLCHAIN_EXPLICIT=0
TIMING_SECONDS=10

usage() {
    cat <<'EOF'
Usage: check_combined_rust.sh [--runtime <dir>] [--toolchain <tc>] [--timing-seconds <1..60>]

Options:
  --runtime <dir>          Optional runtime directory; when set, exports ORPHEUS_* env vars
                           and runs ignored lib tests plus orpheus_server --check-runtime.
  --toolchain <tc>         Rust toolchain selector (default: stable). If omitted and rustup
                           +stable is unavailable, falls back to bare cargo.
  --timing-seconds <1..60> Timing benchmark duration in seconds (default: 10).
  -h, --help               Show this help message.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --runtime)
            [[ $# -ge 2 ]] || { echo "Missing value for --runtime" >&2; exit 2; }
            RUNTIME="$2"
            shift 2
            ;;
        --toolchain)
            [[ $# -ge 2 ]] || { echo "Missing value for --toolchain" >&2; exit 2; }
            TOOLCHAIN="$2"
            TOOLCHAIN_EXPLICIT=1
            shift 2
            ;;
        --timing-seconds)
            [[ $# -ge 2 ]] || { echo "Missing value for --timing-seconds" >&2; exit 2; }
            TIMING_SECONDS="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if ! [[ "$TIMING_SECONDS" =~ ^[0-9]+$ ]] || (( TIMING_SECONDS < 1 || TIMING_SECONDS > 60 )); then
    echo "--timing-seconds must be an integer between 1 and 60 (got: $TIMING_SECONDS)" >&2
    exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

if cargo +"$TOOLCHAIN" --version >/dev/null 2>&1; then
    CARGO_CMD=(cargo "+$TOOLCHAIN")
elif (( TOOLCHAIN_EXPLICIT == 1 )); then
    echo "Requested toolchain '+$TOOLCHAIN' is not available via cargo" >&2
    exit 1
else
    CARGO_CMD=(cargo)
fi

SHARED_CONTRACTS=(
    "script_init::tests::shared_unkeyed_array_contract"
    "script_expr::tests::shared_engine_setting_contract"
    "script_entity_state::tests::shared_foreign_component_contract"
    "script_init::tests::shared_corpse_retirement_contract"
    "script_expr::tests::shared_velocity_contract"
    "script_init::tests::shared_contains_condition_contract"
    "script_expr::tests::shared_total_health_contract"
    "script_expr::tests::shared_hostile_filter_contract"
    "script_expr::tests::shared_combat_target_contract"
    "script_expr::tests::shared_ally_filter_contract"
    "script_expr::tests::shared_native_filter_tag_clauses"
    "script_init::tests::shared_voice_lifecycle_contract"
    "junkenstein_voice::tests::shared_boss_speech_contract"
    "script_expr::tests::shared_body_field_contract"
    "script_expr::tests::shared_body_angle_contract"
    "linked_aim::tests::shared_captured_aim_numeric_contract"
    "script_enemy::tests::shared_captured_aim_lifecycle_contract"
    "script_init::tests::shared_charge_contract"
    "script_enemy::tests::shared_retired_entity_contract"
    "script_enemy::tests::shared_self_damage_clock_contract"
    "script_enemy::tests::shared_flashbang_effect_dispatch_contract"
    "script_enemy::tests::shared_flashbang_entry_guard_contract"
    "script_enemy::tests::shared_flashbang_exclusion_guard_contract"
    "script_expr::tests::shared_flashbang_filter_admission_contract"
    "script_expr::tests::shared_absent_target_filter_contract"
    "script_init::tests::shared_nested_destination_contract"
    "script_init::tests::shared_unbound_loadout_action_contract"
    "script_init::tests::shared_flashbang_base_state_contract"
    "script_enemy::tests::shared_flashbang_targeted_message_contract"
    "script_enemy::tests::shared_flashbang_velocity_action_contract"
    "game_bots::tests::shared_flashbang_horizontal_motion_contract"
    "script_init::tests::shared_foreign_notification_contract"
    "script_init::tests::shared_victory_view_route_contract"
    "script_init::tests::shared_spatial_owner_absence_contract"
    "script_init::tests::shared_ability_press_window_contract"
    "script_init::tests::shared_stack_admission_contract"
    "script_player::tests::shared_deadeye_aim_ray_visibility_contract"
    "junkenstein_validation::tests::shared_junkenstein_validation_contract"
)

verify_shared_contracts() {
    local feature="$1"
    local inventory ignored_inventory
    inventory="$("${CARGO_CMD[@]}" test --locked -p orpheus --lib --features "$feature" -- --list)"
    ignored_inventory="$("${CARGO_CMD[@]}" test --locked -p orpheus --lib --features "$feature" -- --ignored --list)"

    local contract
    for contract in "${SHARED_CONTRACTS[@]}"; do
        if ! grep -Fxq "${contract}: test" <<< "$inventory"; then
            echo "Shared contract test is missing for feature '${feature}': ${contract}" >&2
            return 1
        fi
        if grep -Fxq "${contract}: test" <<< "$ignored_inventory"; then
            echo "Shared contract test must not be marked #[ignore] for feature '${feature}': ${contract}" >&2
            return 1
        fi
    done

    local line test_name found
    while IFS= read -r line; do
        if [[ "$line" =~ ^([^[:space:]]+::tests::shared_[^[:space:]]+):[[:space:]]test$ ]]; then
            test_name="${BASH_REMATCH[1]}"
            found=0
            for contract in "${SHARED_CONTRACTS[@]}"; do
                if [[ "$test_name" == "$contract" ]]; then
                    found=1
                    break
                fi
            done
            if (( found == 0 )); then
                echo "Unlisted shared contract test found for feature '${feature}': ${test_name}" >&2
                return 1
            fi
        fi
    done <<< "$inventory"
}

RUNTIME_PATH=""
if [[ -n "$RUNTIME" ]]; then
    if [[ ! -d "$RUNTIME" ]]; then
        echo "Runtime directory does not exist: $RUNTIME" >&2
        exit 1
    fi
    RUNTIME_PATH="$(cd "$RUNTIME" && pwd)"
    export ORPHEUS_RUNTIME_DIR="$RUNTIME_PATH"
    export ORPHEUS_BNET_DESCRIPTORS="$RUNTIME_PATH/bnet/descriptors.pb"
    export ORPHEUS_CATALOG_DATA="$RUNTIME_PATH/data"
    export ORPHEUS_TIMING_BENCH_RUNTIME="$RUNTIME_PATH"
    export ORPHEUS_TIMING_BENCH_SECONDS="$TIMING_SECONDS"
else
    unset ORPHEUS_RUNTIME_DIR ORPHEUS_BNET_DESCRIPTORS ORPHEUS_CATALOG_DATA ORPHEUS_TIMING_BENCH_RUNTIME ORPHEUS_TIMING_BENCH_SECONDS
fi

git diff --check
"${CARGO_CMD[@]}" fmt --all -- --check

for feature in default inspector; do
    "${CARGO_CMD[@]}" clippy --locked --workspace --all-targets --features "$feature" -- -D warnings
    "${CARGO_CMD[@]}" test --locked --workspace --all-targets --features "$feature" --quiet
    verify_shared_contracts "$feature"
    if [[ -n "$RUNTIME_PATH" ]]; then
        "${CARGO_CMD[@]}" test --locked --workspace --lib --features "$feature" -- --ignored --test-threads=4 --quiet
    fi
done

if [[ -n "$RUNTIME_PATH" ]]; then
    "${CARGO_CMD[@]}" run --locked --bin orpheus_server -- --check-runtime --runtime "$RUNTIME_PATH"
    echo "Combined Rust source, runtime preflight and prepared checks passed."
else
    echo "Combined Rust source checks passed (runtime checks skipped; pass --runtime to run them)."
fi
