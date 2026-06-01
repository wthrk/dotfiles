//! private `password-store` repository の Git clone と store filesystem 観測へ application が
//! 要求する port 契約。
//!
//! この module は「`~/.password-store` の存在確認」「GPG authentication subkey 経由の SSH agent
//! 認証による Git clone」「clone 後 store が `pass` から読める構成かの観測」という capability だけを
//! 宣言する。git2 / libssh2 / SSH agent socket の実装詳細、filesystem 走査、`git` CLI 呼び出しは
//! adapter 側へ閉じ、ここには露出しない。clone URL の妥当性や store 可読性の業務判断は domain rule に
//! 委ね、port は外部依存 capability の宣言にとどめる。

use super::super::domain::pass_restore::{PasswordStoreReadiness, PasswordStoreRemote};
use crate::Result;

/// use case が `~/.password-store` の filesystem 状態へ要求する capability 契約。
///
/// caller は clone 前の不存在確認と clone 後の可読性確認の順序を application/domain 側で決める。
/// implementor は `$HOME` 解決と filesystem 走査だけを担い、`~/.password-store` 既存時の停止可否や
/// store 可読性の充足判定そのものの業務規則は再定義しない。
#[cfg_attr(test, mockall::automock)]
pub trait PasswordStorePort {
    /// `~/.password-store` が既に存在するかを確認する。
    ///
    /// 設計（spec L174 / 停止条件 L212）は clone 前に `~/.password-store` 不存在を要求する。
    /// implementor は path の存在有無だけを返し、停止判断は caller（application）が行う。
    fn password_store_exists(&self) -> Result<bool>;

    /// clone 先 store directory を走査し、`pass` 可読性の観測結果（`.gpg-id` 存在・recipient 行・
    /// 復号確認用サンプル entry）を返す。
    ///
    /// implementor は store root の識別ファイル有無・`.gpg-id` 各行・サンプル `*.gpg` entry path だけを
    /// 観測して [`PasswordStoreReadiness`] へ翻訳し、recipient 形式妥当性や復号可否の充足判定は domain rule /
    /// keyring 照合へ委ねる。`pass` CLI への無条件シェルアウトはしない。
    fn inspect_password_store(&self) -> Result<PasswordStoreReadiness>;

    /// 検証失敗時のロールバックとして、clone で作成した `~/.password-store` を best-effort で削除する。
    ///
    /// clone は成功したが clone 後の可読性確認で失敗した場合、残置した store が次回 restore-pass を既存
    /// store ガード（`password_store_exists`）で停止させ復旧不能にする。implementor は `~/.password-store`
    /// directory tree を best-effort で削除し、不在なら成功扱いとする。停止判断と呼び出し順序は caller が持つ。
    fn remove_password_store(&mut self) -> Result<()>;
}

/// use case が private `password-store` repository の Git clone へ要求する capability 契約。
///
/// caller は clone の実行順序（不存在確認の後に clone）を application 側で決める。implementor は git2 +
/// libssh2 で、GPG authentication subkey 由来の identity を gpg-agent の SSH agent（#14 の socket 解決）
/// 経由で credentials callback に提示して `~/.password-store` へ clone する。`git` CLI と GitHub API は
/// 使わず、SSH agent 経路だけを使う。clone 先 path 解決は filesystem adapter と同じ `~/.password-store`
/// に固定する。
#[cfg_attr(test, mockall::automock)]
pub trait GitClonePort {
    /// 検証済み clone URL を `~/.password-store` へ SSH agent 認証で clone する。
    ///
    /// implementor は SSH agent から GPG authentication subkey 由来 identity を提示して clone を行い、
    /// 既存 directory への上書きはしない（不存在確認は caller の責務）。URL は domain で検証済みの
    /// `PasswordStoreRemote` を受け取り、adapter で再検証しない。clone は temp directory 経由で原子的に行い、
    /// 失敗時は destination を残さず、成功時のみ `~/.password-store` へ rename する（既存 store は決して
    /// 上書き・削除しない）。したがって caller は clone 失敗時に destination の rollback 削除を行ってはならない
    /// （TOCTOU で他 process の store を誤削除しうるため）。
    fn clone_password_store(&mut self, remote: &PasswordStoreRemote) -> Result<()>;
}
