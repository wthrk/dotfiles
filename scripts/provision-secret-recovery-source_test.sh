#!/usr/bin/env bash
#
# provisioning source script が primary/spare enrollment と BWS token / PIV session の管理を
# 高水準 Rust command へ委譲し、`status` / `clear` / `setup` / `put` を別 process として
# 組み立てないことを検証する。

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_DIR="$(mktemp -d)"
trap 'rm -rf "$TEST_DIR"' EXIT

DOTFILES_PROVISION_SOURCE_ONLY=1 source "$REPO_ROOT/scripts/provision-secret-recovery-source.sh"

fail() {
	printf 'test failure: %s\n' "$*" >&2
	exit 1
}

run_in_pseudo_terminal() {
	local command_path="$1"
	shift
	local run_args run_arg

	run_args="$command_path"
	for run_arg in "$@"; do
		run_args+=$'\n'"$run_arg"
	done

	command -v expect >/dev/null 2>&1 ||
		fail 'run_with_synced_manual_gates requires expect command'

	RUN_WITH_EXPECT_ARGS="$run_args" \
	expect -c '
		set timeout $env(EXPECT_TIMEOUT)
		set count_limit $env(RUN_WITH_EXPECT_GATES)
		set transcript_file $env(RUN_WITH_EXPECT_TRANSCRIPT)
		if {$transcript_file ne ""} {
			log_file -a $transcript_file
		}
		set args $env(RUN_WITH_EXPECT_ARGS)
		set argv_list [split $args "\n"]
		set cmd [lindex $argv_list 0]
		set spawn_args [lrange $argv_list 1 end]
		set count 0
		spawn -noecho $cmd {*}$spawn_args
		expect {
			-re {Enter:} {
				incr count
				send -- "\r"
				if {$count == $count_limit} {
					exp_continue
				}
				if {$count > $count_limit} {
					exit 1
				}
				exp_continue
			}
			eof {
				if {$count != $count_limit} {
					exit 1
				}
				if {[catch {wait} wait_result]} {
					exit 1
				}
				exit [lindex $wait_result 3]
			}
			timeout {
				exit 124
			}
		}
	'
}

run_with_synced_manual_gates() {
	local gate_count="$1"
	shift
	local command_path="$1"
	shift
	local transcript="$TEST_DIR/pty-transcript-${RANDOM}-$$"
	local status=0

	RUN_WITH_EXPECT_GATES="$gate_count" \
		RUN_WITH_EXPECT_TRANSCRIPT="$transcript" \
		EXPECT_TIMEOUT=15 \
		run_in_pseudo_terminal "$command_path" "$@"
	status=$?

	if [ "$status" -ne 0 ]; then
		[ -f "$transcript" ] && {
			printf '%s\n' 'run_with_synced_manual_gates: status !=0; transcript tail:' >&2
			tail -n 128 "$transcript" >&2
		}
		if grep -q $'\x04' "$transcript" 2>/dev/null; then
			printf '%s\n' 'run_with_synced_manual_gates: EOF marker (0x04) observed in transcript' >&2
		fi
	fi
	[ -f "$transcript" ] && command cat "$transcript"
	rm -f "$transcript"
	return "$status"
}

manual_gate_count_from_script() {
	local script_path="$1"
	grep -c '^[[:space:]]*pause "' "$script_path"
}

test_script_does_not_transport_yubikey_pin_or_bws_token() {
	if grep -Eq -- '--pin|PIV_PIN|YUBIKEY_PIN|pin_input|BWS_ACCESS_TOKEN|read_bws_access_token' \
		"$REPO_ROOT/scripts/provision-secret-recovery-source.sh"; then
		fail 'provision script が PIV PIN または BWS token を argv / environment / 独自 input として扱っている'
	fi
}

test_script_has_no_split_yubikey_storage_transition() {
	if grep -Eq -- 'secrets yubikey (status|clear|setup|put)( |$)' \
		"$REPO_ROOT/scripts/provision-secret-recovery-source.sh"; then
		fail 'provision script が YubiKey storage transition を別 CLI process に分割している'
	fi
}

test_repo_head_selects_production_cli_binary() {
	local invocation_log="$TEST_DIR/repo-head-cargo-invocation.log"
	direnv() {
		printf '%s\n' "$*" >"$invocation_log"
	}

	run_dotfiles_from_repo_head gpg export-ssh-public-key --primary-fingerprint fixture-fingerprint
	grep -Fxq \
		'exec . cargo run -p dotfiles-cli --bin dotfiles -- gpg export-ssh-public-key --primary-fingerprint fixture-fingerprint' \
		"$invocation_log" ||
		fail '--repo-head は feature 専用 stub ではなく production dotfiles binary を明示しなければならない'
	unset -f direnv
}

test_help_lists_debug_option() {
	local help_output
	help_output="$(bash "$REPO_ROOT/scripts/provision-secret-recovery-source.sh" --help)"
	[ "$help_output" = "Usage: $REPO_ROOT/scripts/provision-secret-recovery-source.sh [--repo-head] [--debug] [--primary-serial SERIAL] [--spare-serial SERIAL]" ] ||
		fail '--help は debug と primary/spare serial の CLI option を表示しなければならない'
}

test_serial_cli_options_override_environment_defaults() {
	local output
	output="$(
		export PROVISIONING_YUBIKEY_SERIAL=1001
		export SPARE_YUBIKEY_SERIAL=1002
		DOTFILES_PROVISION_SOURCE_ONLY=1 source \
			"$REPO_ROOT/scripts/provision-secret-recovery-source.sh" \
			--primary-serial 2001 --spare-serial 2002
		printf '%s %s\n' "$PROVISIONING_YUBIKEY_SERIAL" "$SPARE_YUBIKEY_SERIAL"
	)"
	[ "$output" = '2001 2002' ] ||
		fail 'CLI serial option は environment default を上書きしなければならない'
}

test_script_never_creates_github_repository() {
	if grep -Eq -- 'gh[[:space:]]+repo[[:space:]]+create|/user/repos|/orgs/.*/repos' \
		"$REPO_ROOT/scripts/provision-secret-recovery-source.sh"; then
		fail 'provision script は GitHub repository を作成してはならない'
	fi
}

test_primary_serial_uses_one_enrollment_command() {
	local invocation_log="$TEST_DIR/primary-invocation.log"
	dotfiles() {
		printf '%s\n' "$*" >"$invocation_log"
	}

	enroll_primary_yubikey 2001
	[ "$(<"$invocation_log")" = 'secrets yubikey enroll-primary --serial 2001' ] ||
		fail 'primary は serial 付き enroll-primary 一回だけを呼ばなければならない'
}

test_implicit_serial_uses_one_enrollment_command() {
	local invocation_log="$TEST_DIR/implicit-invocation.log"
	dotfiles() {
		printf '%s\n' "$*" >"$invocation_log"
	}

	enroll_primary_yubikey ''
	[ "$(<"$invocation_log")" = 'secrets yubikey enroll-primary' ] ||
		fail 'serial 未指定は enroll-primary 一回に委譲しなければならない'
}

test_spare_uses_primary_read_without_token_reinput() {
	local invocation_log="$TEST_DIR/spare-invocation.log"
	dotfiles() {
		printf '%s\n' "$*" >"$invocation_log"
	}

	enroll_spare_yubikey 2001 2002
	[ "$(<"$invocation_log")" = 'secrets yubikey enroll-spare --primary-serial 2001 --spare-serial 2002' ] ||
		fail 'spare は primary serial から token を読む enroll-spare 一回だけを呼ばなければならない'
}

test_debug_observes_the_actual_primary_enrollment_without_a_second_command() {
	local invocation_log="$TEST_DIR/debug-invocation.log"
	(
		set -- --debug
		DOTFILES_PROVISION_SOURCE_ONLY=1 source "$REPO_ROOT/scripts/provision-secret-recovery-source.sh"
		dotfiles() {
			printf '%s\n' "$*" >"$invocation_log"
		}

		enroll_primary_yubikey 2001
	)
	[ "$(<"$invocation_log")" = 'secrets yubikey enroll-primary --debug --serial 2001' ] ||
		fail '--debug は別の再検証 command ではなく実際の enroll-primary 一回へ転送しなければならない'
}

test_debug_observes_the_actual_spare_enrollment_without_a_second_command() {
	local invocation_log="$TEST_DIR/debug-spare-invocation.log"
	(
		set -- --debug
		DOTFILES_PROVISION_SOURCE_ONLY=1 source "$REPO_ROOT/scripts/provision-secret-recovery-source.sh"
		dotfiles() {
			printf '%s\n' "$*" >"$invocation_log"
		}

		enroll_spare_yubikey 2001 2002
	)
	[ "$(<"$invocation_log")" = 'secrets yubikey enroll-spare --primary-serial 2001 --spare-serial 2002 --debug' ] ||
		fail '--debug は別の再検証 command ではなく実際の enroll-spare 一回へ転送しなければならない'
}

test_script_contains_no_best_effort_success_fallbacks() {
	if grep -Eq -- '\|\|[[:space:]]*true' \
		"$REPO_ROOT/scripts/provision-secret-recovery-source.sh"; then
		fail 'provision script は外部 command failure を success として継続してはならない'
	fi
}

test_gpg_query_failure_is_not_reclassified_as_a_missing_recipient() {
	local output status
	set +e
	output="$({
		gpg() {
			return 17
		}
		recipient_secret_key_fingerprint fixture-recipient
	} 2>&1)"
	status=$?
	set -e
	[ "$status" -ne 0 ] ||
		fail 'GPG SDK/CLI failure を空 fingerprint（recipient 不在）として継続してはならない'
	grep -qF 'GPG secret key の fingerprint 取得に失敗しました' <<<"$output" ||
		fail 'GPG query failure は query operation failure として停止しなければならない'
}

test_missing_subkeys_are_added_before_validation_and_validation_failure_has_no_external_mutation() {
	local fake_bin="$TEST_DIR/preflight-failure-bin"
	local password_store="$TEST_DIR/preflight-failure-password-store"
	local command_log="$TEST_DIR/preflight-failure-command.log"
	: >"$command_log"
	mkdir -p "$fake_bin" "$password_store/.git"
	printf '%s\n' 'fixture-recipient' >"$password_store/.gpg-id"

	for command_name in gpg gh pass dotfiles; do
		printf '%s\n' \
			'#!/usr/bin/env bash' \
			'set -euo pipefail' \
			'printf "%s %s\\n" "$(basename "$0")" "$*" >>"$PROVISION_TEST_COMMAND_LOG"' \
			'case "$(basename "$0") $*" in' \
			'  "gpg --list-secret-keys --with-colons --fingerprint "*) printf "fpr:::::::::0123456789ABCDEF0123456789ABCDEF01234567:\\n" ;;' \
			'  "gpg --list-secret-keys --with-colons "*) printf "uid:::::::::Fixture User:\\n" ;;' \
			'  "gpg --list-keys --with-colons "*) printf "sub:::::::::::\\n" ;;' \
			'  "gpg --quick-add-key "*) : ;;' \
			'  "gh api user --jq .login") printf "fixture-owner\\n" ;;' \
			'  "gh api user/keys --paginate --jq length") printf "0\\n" ;;' \
			'  "dotfiles secrets gpg-backup validate --primary-fingerprint "*) exit 71 ;;' \
			'esac' \
			>"$fake_bin/$command_name"
		chmod +x "$fake_bin/$command_name"
	done

	local status subprocess_output
	set +e
	subprocess_output="$(
		PATH="$fake_bin:$PATH" \
			PROVISION_TEST_COMMAND_LOG="$command_log" \
			PASSWORD_STORE_DIR="$password_store" \
			PASS_REPO="fixture-owner/password-store" \
			run_with_synced_manual_gates 0 \
			"$REPO_ROOT/scripts/provision-secret-recovery-source.sh" \
			--primary-serial 2001 --spare-serial 2002 2>&1
	)"
	status=$?
	set -e
	[ "$status" -ne 0 ] ||
		fail 'E/A/S 補完後の packet validation failure は provisioning を停止しなければならない'
	[ -f "$command_log" ] ||
		fail "preflight failure fixture は command log を生成しなければならない: $subprocess_output"

	local quick_add_count encryption_line authentication_line signing_line validation_line
	quick_add_count="$(grep -c '^gpg --quick-add-key 0123456789ABCDEF0123456789ABCDEF01234567 ' "$command_log" || :)"
	encryption_line="$(grep -nF 'gpg --quick-add-key 0123456789ABCDEF0123456789ABCDEF01234567 cv25519 encrypt never' "$command_log" | cut -d: -f1 || :)"
	authentication_line="$(grep -nF 'gpg --quick-add-key 0123456789ABCDEF0123456789ABCDEF01234567 ed25519 auth never' "$command_log" | cut -d: -f1 || :)"
	signing_line="$(grep -nF 'gpg --quick-add-key 0123456789ABCDEF0123456789ABCDEF01234567 ed25519 sign never' "$command_log" | cut -d: -f1 || :)"
	validation_line="$(grep -nF 'dotfiles secrets gpg-backup validate --primary-fingerprint 0123456789ABCDEF0123456789ABCDEF01234567' "$command_log" | cut -d: -f1 || :)"
	[ "$quick_add_count" -eq 3 ] &&
		[ -n "$encryption_line" ] &&
		[ -n "$authentication_line" ] &&
		[ -n "$signing_line" ] &&
		[ -n "$validation_line" ] ||
		fail "missing E/A/S fixture は quick-add-key 3 回と packet validation を観測しなければならない。command log:\n$(command cat "$command_log")\nsubprocess:\n$subprocess_output"
	[ "$encryption_line" -lt "$authentication_line" ] &&
		[ "$authentication_line" -lt "$signing_line" ] &&
		[ "$signing_line" -lt "$validation_line" ] ||
		fail 'missing E/A/S は全て補完してから packet validation を実行しなければならない'
	! grep -Eq '^gh ssh-key add|^pass git |^dotfiles secrets yubikey |^dotfiles secrets pass-remote register|^dotfiles secrets gpg-backup register' "$command_log" ||
		fail 'packet validation failure 後に GitHub/Git/YubiKey/BWS mutation へ進んではならない'
}

test_main_flow_keeps_manual_gate_and_exact_enrollment_order() {
	local script="$REPO_ROOT/scripts/provision-secret-recovery-source.sh"
	local gate_line primary_line spare_line bws_line
	gate_line="$(grep -nF 'pause "Bitwarden Secrets Manager' "$script" | cut -d: -f1)"
	primary_line="$(grep -nF 'enroll_primary_yubikey "$YUBIKEY_SERIAL"' "$script" | cut -d: -f1)"
	spare_line="$(grep -nF 'enroll_spare_yubikey "$YUBIKEY_SERIAL" "$SPARE_SERIAL"' "$script" | cut -d: -f1)"
	bws_line="$(grep -nF 'log "BWS に password-store-remote を登録"' "$script" | cut -d: -f1)"
	[ -n "$gate_line" ] && [ -n "$primary_line" ] && [ -n "$spare_line" ] && [ -n "$bws_line" ] ||
		fail 'provision main flow の manual gate / enrollment / BWS 登録境界を解決できない'
	[ "$gate_line" -lt "$primary_line" ] &&
		[ "$primary_line" -lt "$spare_line" ] &&
		[ "$spare_line" -lt "$bws_line" ] ||
		fail 'manual BWS gate → primary enroll → optional spare enroll → BWS 登録の順序を維持しなければならない'
}

test_main_subprocess_uses_manual_repo_gate_and_exact_command_order() {
	command -v script >/dev/null 2>&1 ||
		fail 'full provisioning subprocess test には pseudo terminal を提供する script command が必要です'
	local fake_bin="$TEST_DIR/fake-bin"
	local password_store="$TEST_DIR/password-store"
	local command_log="$TEST_DIR/full-main-command.log"
	local enrollment_state="$TEST_DIR/full-main-enrollment-state"
	local enrollment_trace="$TEST_DIR/full-main-enrollment-trace"
	local -r manual_gate_count="$(manual_gate_count_from_script "$REPO_ROOT/scripts/provision-secret-recovery-source.sh")"
	[ "$manual_gate_count" -gt 0 ] ||
		fail 'production script の pause 呼び出し数が 0 か解決不能です（full main gate 同期が安全に実行できません）'
	mkdir -p "$fake_bin" "$password_store/.git"
	printf '%s\n' 'fixture-recipient' >"$password_store/.gpg-id"
	: >"$enrollment_state"
	: >"$enrollment_trace"

	for command_name in gpg gh pass dotfiles git; do
		printf '%s\n' \
			'#!/usr/bin/env bash' \
			'set -euo pipefail' \
			'printf "%s %s\n" "$(basename "$0")" "$*" >>"$PROVISION_TEST_COMMAND_LOG"' \
			'case "$(basename "$0") $*" in' \
			'  "gpg --list-secret-keys --with-colons --fingerprint "*) printf "fpr:::::::::0123456789ABCDEF0123456789ABCDEF01234567:\n" ;;' \
			'  "gpg --list-secret-keys --with-colons "*) printf "uid:::::::::Fixture User:\n" ;;' \
			'  "gpg --list-keys --with-colons "*) printf "sub:::::::::::eas:\n" ;;' \
			'  "gh api user --jq .login") printf "fixture-owner\n" ;;' \
			'  "gh api user/keys --paginate --jq length") printf "0\n" ;;' \
			'  "gh ssh-key list") printf "fixture ssh-ed25519-body\n" ;;' \
			'  "gh repo view fixture-owner/password-store --json isPrivate --jq .isPrivate") printf "true\n" ;;' \
			'  "pass git config --get remote.origin.url") printf "git@github.com:fixture-owner/password-store.git\n" ;;' \
			'  "pass git branch --show-current") printf "main\n" ;;' \
			'  "pass git config --get branch.main.merge") printf "refs/heads/main\n" ;;' \
			'  "dotfiles gpg export-ssh-public-key --primary-fingerprint "*) printf "ssh-ed25519 ssh-ed25519-body fixture\n" ;;' \
			'esac' \
			>"$fake_bin/$command_name"
		chmod +x "$fake_bin/$command_name"
	done

	# 二回実行の enrollment は command log の反復ではなく、同じ fake datastore を
	# fresh → initialized に遷移させる。二回目が前回 state を読まない成功は受け入れない。
	printf '%s\n' \
		'#!/usr/bin/env bash' \
		'set -euo pipefail' \
		'printf "dotfiles %s\n" "$*" >>"$PROVISION_TEST_COMMAND_LOG"' \
		'case "$*" in' \
		'  "gpg export-ssh-public-key "*) printf "ssh-ed25519 ssh-ed25519-body fixture\n" ;;' \
		'  "secrets yubikey enroll-primary "*)' \
		'    if grep -qx primary-initialized "$PROVISION_TEST_ENROLLMENT_STATE"; then' \
		'      printf "primary initialized\n" >>"$PROVISION_TEST_ENROLLMENT_TRACE"' \
		'    else' \
		'      printf "primary-initialized\n" >>"$PROVISION_TEST_ENROLLMENT_STATE"' \
		'      printf "primary fresh\n" >>"$PROVISION_TEST_ENROLLMENT_TRACE"' \
		'    fi ;;' \
		'  "secrets yubikey enroll-spare "*)' \
		'    grep -qx primary-initialized "$PROVISION_TEST_ENROLLMENT_STATE"' \
		'    if grep -qx spare-initialized "$PROVISION_TEST_ENROLLMENT_STATE"; then' \
		'      printf "spare initialized\n" >>"$PROVISION_TEST_ENROLLMENT_TRACE"' \
		'    else' \
		'      printf "spare-initialized\n" >>"$PROVISION_TEST_ENROLLMENT_STATE"' \
		'      printf "spare fresh\n" >>"$PROVISION_TEST_ENROLLMENT_TRACE"' \
		'    fi ;;' \
		'esac' \
		>"$fake_bin/dotfiles"
	chmod +x "$fake_bin/dotfiles"

	local status subprocess_output
	set +e
	subprocess_output="$(
		PATH="$fake_bin:$PATH" \
			PROVISION_TEST_COMMAND_LOG="$command_log" \
			PROVISION_TEST_ENROLLMENT_STATE="$enrollment_state" \
			PROVISION_TEST_ENROLLMENT_TRACE="$enrollment_trace" \
			PASSWORD_STORE_DIR="$password_store" \
			PASS_REPO="fixture-owner/password-store" \
			run_with_synced_manual_gates "$manual_gate_count" \
			"$REPO_ROOT/scripts/provision-secret-recovery-source.sh" \
			--primary-serial 2001 --spare-serial 2002 2>&1
	)"
	status=$?
	set -e
	[ "$status" -eq 0 ] ||
		fail "fake PATH の full provisioning subprocess は成功しなければならない: $subprocess_output"
	! grep -Eq '^gh repo create|^gh api (user/repos|orgs/.*/repos)' "$command_log" ||
		fail 'full provisioning subprocess は GitHub repository を作成してはならない'

	local primary_line spare_line pass_line backup_line verify_primary_line verify_spare_line
	primary_line="$(grep -nF 'dotfiles secrets yubikey enroll-primary --serial 2001' "$command_log" | cut -d: -f1)"
	spare_line="$(grep -nF 'dotfiles secrets yubikey enroll-spare --primary-serial 2001 --spare-serial 2002' "$command_log" | cut -d: -f1)"
	pass_line="$(grep -nF 'dotfiles secrets pass-remote register --url git@github.com:fixture-owner/password-store.git --yes --serial 2001' "$command_log" | cut -d: -f1)"
	backup_line="$(grep -nF 'dotfiles secrets gpg-backup register --primary-fingerprint 0123456789ABCDEF0123456789ABCDEF01234567 --serial 2001' "$command_log" | cut -d: -f1)"
	verify_primary_line="$(grep -nF 'dotfiles secrets verify-yubikey --serial 2001 --all' "$command_log" | cut -d: -f1)"
	verify_spare_line="$(grep -nF 'dotfiles secrets verify-yubikey --serial 2002 --all' "$command_log" | cut -d: -f1)"
	[ "$primary_line" -lt "$spare_line" ] &&
		[ "$spare_line" -lt "$pass_line" ] &&
		[ "$pass_line" -lt "$backup_line" ] &&
		[ "$backup_line" -lt "$verify_primary_line" ] &&
		[ "$verify_primary_line" -lt "$verify_spare_line" ] ||
		fail 'full provisioning subprocess の enrollment/BWS/verification 順序が正本と異なる'

	# 同じ password-store / remote / YubiKey serial と fake external state のまま manual gate を再同期して
	# main をもう一度実行する。二回目は repository 作成、secret 再入力、split storage transition をせず、
	# 高水準 enrollment → BWS registration → verification の順序を維持しなければならない。
	set +e
	subprocess_output="$(
		PATH="$fake_bin:$PATH" \
			PROVISION_TEST_COMMAND_LOG="$command_log" \
			PROVISION_TEST_ENROLLMENT_STATE="$enrollment_state" \
			PROVISION_TEST_ENROLLMENT_TRACE="$enrollment_trace" \
			PASSWORD_STORE_DIR="$password_store" \
			PASS_REPO="fixture-owner/password-store" \
			run_with_synced_manual_gates "$manual_gate_count" \
			"$REPO_ROOT/scripts/provision-secret-recovery-source.sh" \
			--primary-serial 2001 --spare-serial 2002 2>&1
	)"
	status=$?
	set -e
	[ "$status" -eq 0 ] ||
		fail "same-state second full provisioning subprocess は成功しなければならない: $subprocess_output"
	! grep -Eq '^gh repo create|^gh api (user/repos|orgs/.*/repos)' "$command_log" ||
		fail 'same-state second subprocess も GitHub repository を作成してはならない'
	[ "$(grep -cFx 'dotfiles secrets yubikey enroll-primary --serial 2001' "$command_log")" -eq 2 ] ||
		fail 'same-state two runs は primary enrollment を一回ずつだけ呼ばなければならない'
	[ "$(grep -cFx 'dotfiles secrets yubikey enroll-spare --primary-serial 2001 --spare-serial 2002' "$command_log")" -eq 2 ] ||
		fail 'same-state two runs は spare enrollment を一回ずつだけ呼ばなければならない'
	[ "$(grep -cFx 'dotfiles secrets verify-yubikey --serial 2002 --all' "$command_log")" -eq 2 ] ||
		fail 'same-state two runs は各回の verification まで到達しなければならない'
	[ "$(command cat "$enrollment_trace")" = $'primary fresh\nspare fresh\nprimary initialized\nspare initialized' ] ||
		fail 'same-state second subprocess は first-run datastore を initialized として読まなければならない'

	: >"$command_log"
	printf '%s\n' \
		'#!/usr/bin/env bash' \
		'set -euo pipefail' \
		'printf "dotfiles %s\n" "$*" >>"$PROVISION_TEST_COMMAND_LOG"' \
		'case "$*" in' \
		'  "gpg export-ssh-public-key "*) printf "ssh-ed25519 ssh-ed25519-body fixture\n" ;;' \
		'  "secrets yubikey enroll-primary "*) exit 71 ;;' \
		'esac' \
		>"$fake_bin/dotfiles"
	chmod +x "$fake_bin/dotfiles"
	set +e
	PATH="$fake_bin:$PATH" \
		PROVISION_TEST_COMMAND_LOG="$command_log" \
		PASSWORD_STORE_DIR="$password_store" \
		PASS_REPO="fixture-owner/password-store" \
		run_with_synced_manual_gates 2 \
		"$REPO_ROOT/scripts/provision-secret-recovery-source.sh" \
		--primary-serial 2001 --spare-serial 2002 \
		>/dev/null 2>&1
	status=$?
	set -e
	[ "$status" -ne 0 ] ||
		fail '高水準 enrollment failure は full subprocess の failure として伝播しなければならない'
	grep -qF 'dotfiles secrets yubikey enroll-primary --serial 2001' "$command_log" ||
		fail 'failure subprocess は primary enrollment を実行しなければならない'
	! grep -qF 'dotfiles secrets pass-remote register' "$command_log" ||
		fail 'primary enrollment failure 後に BWS provisioning へ進んではならない'
}

test_script_does_not_transport_yubikey_pin_or_bws_token
test_script_has_no_split_yubikey_storage_transition
test_repo_head_selects_production_cli_binary
test_help_lists_debug_option
test_serial_cli_options_override_environment_defaults
test_script_never_creates_github_repository
test_primary_serial_uses_one_enrollment_command
test_implicit_serial_uses_one_enrollment_command
test_spare_uses_primary_read_without_token_reinput
test_debug_observes_the_actual_primary_enrollment_without_a_second_command
test_debug_observes_the_actual_spare_enrollment_without_a_second_command
test_script_contains_no_best_effort_success_fallbacks
test_gpg_query_failure_is_not_reclassified_as_a_missing_recipient
test_missing_subkeys_are_added_before_validation_and_validation_failure_has_no_external_mutation
test_main_flow_keeps_manual_gate_and_exact_enrollment_order
test_main_subprocess_uses_manual_repo_gate_and_exact_command_order
