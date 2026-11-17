param(
    [Parameter(Mandatory = $false)]
    [string]$Runtime = '',
    [string]$Toolchain = 'stable',
    [ValidateRange(1, 60)]
    [int]$TimingSeconds = 10
)

$ErrorActionPreference = 'Stop'
$sharedContracts = @(
    'script_init::tests::shared_unkeyed_array_contract',
    'script_expr::tests::shared_engine_setting_contract',
    'script_entity_state::tests::shared_foreign_component_contract',
    'script_init::tests::shared_corpse_retirement_contract',
    'script_expr::tests::shared_velocity_contract',
    'script_init::tests::shared_contains_condition_contract',
    'script_expr::tests::shared_total_health_contract',
    'script_expr::tests::shared_hostile_filter_contract',
    'script_expr::tests::shared_combat_target_contract',
    'script_expr::tests::shared_ally_filter_contract',
    'script_expr::tests::shared_native_filter_tag_clauses',
    'script_init::tests::shared_voice_lifecycle_contract',
    'junkenstein_voice::tests::shared_boss_speech_contract',
    'script_expr::tests::shared_body_field_contract',
    'script_expr::tests::shared_body_angle_contract',
    'linked_aim::tests::shared_captured_aim_numeric_contract',
    'script_enemy::tests::shared_captured_aim_lifecycle_contract',
    'script_init::tests::shared_charge_contract',
    'script_enemy::tests::shared_retired_entity_contract',
    'script_enemy::tests::shared_self_damage_clock_contract',
    'script_enemy::tests::shared_flashbang_effect_dispatch_contract',
    'script_enemy::tests::shared_flashbang_entry_guard_contract',
    'script_enemy::tests::shared_flashbang_exclusion_guard_contract',
    'script_expr::tests::shared_flashbang_filter_admission_contract',
    'script_expr::tests::shared_absent_target_filter_contract',
    'script_init::tests::shared_nested_destination_contract',
    'script_init::tests::shared_unbound_loadout_action_contract',
    'script_init::tests::shared_flashbang_base_state_contract',
    'script_enemy::tests::shared_flashbang_targeted_message_contract',
    'script_enemy::tests::shared_flashbang_velocity_action_contract',
    'game_bots::tests::shared_flashbang_horizontal_motion_contract',
    'script_init::tests::shared_foreign_notification_contract',
    'script_init::tests::shared_victory_view_route_contract',
    'script_init::tests::shared_spatial_owner_absence_contract',
    'script_init::tests::shared_ability_press_window_contract',
    'script_init::tests::shared_stack_admission_contract',
    'script_player::tests::shared_deadeye_aim_ray_visibility_contract',
    'junkenstein_validation::tests::shared_junkenstein_validation_contract'
)

$hasRuntime = -not [string]::IsNullOrWhiteSpace($Runtime)
$runtimePath = ''
$envNames = @(
    'ORPHEUS_RUNTIME_DIR',
    'ORPHEUS_BNET_DESCRIPTORS',
    'ORPHEUS_CATALOG_DATA',
    'ORPHEUS_TIMING_BENCH_RUNTIME',
    'ORPHEUS_TIMING_BENCH_SECONDS'
)
$previous = @{}
foreach ($name in $envNames) {
    $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
if ($hasRuntime) {
    $runtimePath = (Resolve-Path -LiteralPath $Runtime).Path
    $environment = @{
        ORPHEUS_RUNTIME_DIR = $runtimePath
        ORPHEUS_BNET_DESCRIPTORS = Join-Path $runtimePath 'bnet/descriptors.pb'
        ORPHEUS_CATALOG_DATA = Join-Path $runtimePath 'data'
        ORPHEUS_TIMING_BENCH_RUNTIME = $runtimePath
        ORPHEUS_TIMING_BENCH_SECONDS = [string]$TimingSeconds
    }
    foreach ($name in $environment.Keys) {
        [Environment]::SetEnvironmentVariable($name, $environment[$name], 'Process')
    }
}
else {
    foreach ($name in $envNames) {
        [Environment]::SetEnvironmentVariable($name, $null, 'Process')
    }
}

function Invoke-CargoCheck {
    param([string[]]$Arguments)
    & cargo "+$Toolchain" @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "cargo $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

function Assert-SharedContractInventory {
    param([string]$Feature)
    $inventory = @(& cargo "+$Toolchain" test --locked -p orpheus --lib --features $Feature -- --list)
    if ($LASTEXITCODE -ne 0) {
        throw "cargo test --locked -p orpheus --lib --features $Feature -- --list failed with exit code $LASTEXITCODE"
    }
    $ignoredInventory = @(& cargo "+$Toolchain" test --locked -p orpheus --lib --features $Feature -- --ignored --list)
    if ($LASTEXITCODE -ne 0) {
        throw "cargo test --locked -p orpheus --lib --features $Feature -- --ignored --list failed with exit code $LASTEXITCODE"
    }
    $discovered = @(
        $inventory | ForEach-Object {
            if ($_ -cmatch '^(\S+::tests::shared_\S+): test$') { $Matches[1] }
        }
    )
    $missingFromInventory = @($sharedContracts | Where-Object { $inventory -cnotcontains "${_}: test" })
    if ($missingFromInventory.Count -gt 0) {
        throw "Shared contract test(s) missing from inventory for feature '$Feature': $($missingFromInventory -join ', ')"
    }
    $ignoredContracts = @($sharedContracts | Where-Object { $ignoredInventory -ccontains "${_}: test" })
    if ($ignoredContracts.Count -gt 0) {
        throw "Shared contract test(s) must not be marked #[ignore] for feature '$Feature': $($ignoredContracts -join ', ')"
    }
    $missingFromContracts = @($discovered | Where-Object { $sharedContracts -cnotcontains $_ })
    if ($missingFromContracts.Count -gt 0) {
        throw "Shared contract test(s) missing from contracts list for feature '$Feature': $($missingFromContracts -join ', ')"
    }
}

Push-Location (Split-Path -Parent $PSScriptRoot)
try {
    & git diff --check
    if ($LASTEXITCODE -ne 0) {
        throw "git diff --check failed with exit code $LASTEXITCODE"
    }
    Invoke-CargoCheck -Arguments @('fmt', '--all', '--', '--check')
    foreach ($feature in @('default', 'inspector')) {
        Invoke-CargoCheck -Arguments @('clippy', '--locked', '--workspace', '--all-targets', '--features', $feature, '--', '-D', 'warnings')
        Invoke-CargoCheck -Arguments @('test', '--locked', '--workspace', '--all-targets', '--features', $feature, '--quiet')
        Assert-SharedContractInventory -Feature $feature
        if ($hasRuntime) {
            Invoke-CargoCheck -Arguments @('test', '--locked', '--workspace', '--lib', '--features', $feature, '--', '--ignored', '--test-threads=4', '--quiet')
        }
    }
    if ($hasRuntime) {
        Invoke-CargoCheck -Arguments @('run', '--locked', '--bin', 'orpheus_server', '--', '--check-runtime', '--runtime', $runtimePath)
        Write-Host 'Combined Rust source, runtime preflight and prepared checks passed.'
    }
    else {
        Write-Host 'Combined Rust source checks passed (runtime checks skipped; pass -Runtime to run them).'
    }
}
finally {
    Pop-Location
    foreach ($name in $previous.Keys) {
        [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process')
    }
}
