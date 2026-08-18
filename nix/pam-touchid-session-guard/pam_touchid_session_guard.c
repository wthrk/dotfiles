/*
 * sudo の PAM auth chain で pam_tid.so より前に走り、実行者以外が console を持っているあいだ
 * pam_tid.so を認証経路へ入らせないための PAM モジュール。
 *
 * 観測された症状。別の利用者がユーザーの高速切り替えでログインしたまま画面をロックしている状態で
 * sudo を実行すると、パスワード要求がその利用者の名前で出る。実行者のパスワードでは通らず、
 * 約 47 秒待たされてから実行者のパスワード入力に落ちる。
 *
 * pam_tid.so が PAM_SUCCESS を返す条件は Authorization right `com.apple.security.sudo` を
 * AuthorizationCopyRights で取得できることである。この right は
 * /System/Library/Security/authorization.plist で `k-of-n = 2` の
 * `{ entitled, authenticate-session-owner }` と定義され、`authenticate-session-owner` は
 * `session-owner = true` を持つ user class の rule である。
 *
 * このモジュールが行うのは判定と 1 個の PAM data 設定だけである。utmpx の console 行に PAM user 以外の
 * 利用者が居るときに、pam_tid.so が参照する PAM data `askpass-enabled` を立てる。pam_tid.so はこの data が
 * 在ると LAEvaluatePolicy にも AuthorizationCopyRights にも入らず PAM_AUTHINFO_UNAVAIL を返すので、
 * sudo は sudo_local の次に置かれた pam_smartcard.so と pam_opendirectory.so の認証へそのまま進む。
 *
 * このモジュールは認証を行わず、返り値は常に PAM_IGNORE である。openpam_dispatch() は PAM_IGNORE を
 * 受け取った module を chain の判定から完全に外す。
 */

#include <string.h>
#include <utmpx.h>

#include <security/pam_appl.h>
#include <security/pam_modules.h>

/* pam_tid.so がこの PAM data の存在を見て認証を打ち切る。値は参照されない。 */
#define TOUCHID_SKIP_DATA_NAME "askpass-enabled"

/* loginwindow が GUI session に対して utmpx へ書く行名。 */
#define CONSOLE_LINE "console"

/*
 * utmpx の固定長フィールドを NUL 終端文字列と比較する。
 *
 * `ut_user`（256 バイト）と `ut_line`（32 バイト）は配列いっぱいまで詰まっていれば NUL 終端されない。
 * 終端の有無に依存しないよう、長さを `strnlen` で配列内に限って測り、その長さぶんだけ `memcmp` する。
 */
static int
field_equals(const char *field, size_t size, const char *value)
{
	size_t length = strnlen(field, size);

	return strlen(value) == length && memcmp(field, value, length) == 0;
}

/*
 * PAM user 以外の利用者が console にログインしているかを返す。
 *
 * GUI ログインは utmpx へ `console` 行として記録されるので、判定対象をその行に限る。別利用者の tty 行や、
 * ログアウト済みを表す DEAD_PROCESS 行は対象にしない。
 */
static int
foreign_console_session(const char *user)
{
	struct utmpx *entry;
	int found = 0;

	setutxent();
	while ((entry = getutxent()) != NULL) {
		if (entry->ut_type != USER_PROCESS)
			continue;
		if (!field_equals(entry->ut_line, sizeof(entry->ut_line), CONSOLE_LINE))
			continue;
		if (field_equals(entry->ut_user, sizeof(entry->ut_user), user))
			continue;
		found = 1;
		break;
	}
	endutxent();

	return found;
}

PAM_EXTERN int
pam_sm_authenticate(pam_handle_t *pamh, int flags, int argc, const char *argv[])
{
	const char *user = NULL;

	(void)flags;
	(void)argc;
	(void)argv;

	/* 実行者を特定できないときは Touch ID の可否に触れない。 */
	if (pam_get_user(pamh, &user, NULL) != PAM_SUCCESS || user == NULL)
		return PAM_IGNORE;

	if (foreign_console_session(user))
		pam_set_data(pamh, TOUCHID_SKIP_DATA_NAME, NULL, NULL);

	return PAM_IGNORE;
}

/*
 * openpam_dispatch() は pam_setcred() でも auth chain を辿るため、未定義だと chain ごとに
 * 「no pam_sm_setcred()」が記録される。このモジュールは credential を持たないので PAM_IGNORE を返す。
 */
PAM_EXTERN int
pam_sm_setcred(pam_handle_t *pamh, int flags, int argc, const char *argv[])
{
	(void)pamh;
	(void)flags;
	(void)argc;
	(void)argv;

	return PAM_IGNORE;
}
