//! Claude Code の `PreToolUse` から呼ばれ、`Bash` へ渡されたコマンドを止める判定を返す。
//!
//! 対象は `docs/task-governance/workflow.md`「9. コマンド操作の機械強制」が挙げる 2 件だけである。
//! どちらも繰り返し破られたため、規約文と別に、実行前へ同じ規則を評価する経路をここに置く。
//!
//! 止めるのは、`shlex` が分けた語に `cat` / `head` / `tail` / `sed` が現れる場合と、コマンドが
//! `$?` または `${?}` の綴りを含む場合である。引用や置換をここで読み解かない。
//! 入力を解せなかった場合は止める側へ倒す。素通しにすると、フックが壊れていることと規則に反して
//! いないことが区別できなくなる。

use std::io::{self, Read as _};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use crate::Result;

/// フックが標準入力へ渡すイベントのうち、判定に使う部分だけを受ける。
///
/// `matcher` で `Bash` に限定しているため、`tool_input` は `command` を持つ。
#[derive(Deserialize)]
struct Event {
    tool_input: ToolInput,
}

/// `Bash` へ渡された実行コマンド。
#[derive(Deserialize)]
struct ToolInput {
    command: String,
}

/// Claude Code が `PreToolUse` の判断として読み取る唯一の形。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Decision {
    hook_specific_output: Denial,
}

/// 拒否そのものと、利用者へ返す理由。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Denial {
    hook_event_name: HookEventName,
    permission_decision: PermissionDecision,
    permission_decision_reason: String,
}

/// 判断を返す相手のフックイベント。
///
/// このフックは `PreToolUse` からしか呼ばれないため、変種はその 1 つだけを持つ。
#[derive(Serialize)]
enum HookEventName {
    /// ツール実行前に可否を尋ねるイベント。
    PreToolUse,
}

/// Claude Code が読み取る許可判断（閉集合）。
///
/// 通す場合はそもそも何も出力しないため、綴りを持つのは拒否だけである。
#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum PermissionDecision {
    /// コマンドを実行させない。
    Deny,
}

/// ファイル閲覧をシェルへ流したときの理由。
const READ_REASON: &str = "ファイル閲覧は Read ツールで行ってください。`cat` / `head` / `tail` / `sed` をシェルから使うと、読んだ範囲が出力に残らず、切り詰めた一部だけで判断することになります。判定は語の一致だけを見るため、引用の中の語も止まります。文言にこれらの語が要る場合は `git commit -F <file>` / `gh pr create --body-file <file>` を使ってください（docs/task-governance/workflow.md「コマンド操作の機械強制」）。";

/// 終了コードの綴りがコマンド文字列にあるときの理由。
const EXIT_REASON: &str = "コマンド文字列に `$?` / `${?}` の綴りを置かないでください。終了コードを退避して出し直すと、コマンド自身の失敗が後続の成功で覆われます。判定は綴りの包含だけを見るため、引用の中の文言も止まります。文言にこの綴りが要る場合は `git commit -F <file>` / `gh pr create --body-file <file>` を使ってください（docs/task-governance/workflow.md「コマンド操作の機械強制」）。";

/// 語として現れたら止めるコマンド名。
const READERS: [&str; 4] = ["cat", "head", "tail", "sed"];

/// 現れたら止める、終了コードの綴り。
const EXIT_CAPTURES: [&str; 2] = ["$?", "${?}"];

/// 標準入力のイベントを読んで判定し、止める場合だけ標準出力へ拒否を出す。
///
/// 通す場合は何も出さない。Claude Code は出力が無いことを「このフックは判断しない」として扱う。
pub(crate) fn pre_tool_use() -> Result<()> {
    let reason = match deny_reason() {
        Ok(None) => return Ok(()),
        Ok(Some(reason)) => reason.to_owned(),
        // 判定できないことは、規則に反していないことの根拠にならない。理由を添えて止める。
        Err(error) => format!("PreToolUse フックが判定できませんでした。止めます: {error:#}"),
    };

    let decision = Decision {
        hook_specific_output: Denial {
            hook_event_name: HookEventName::PreToolUse,
            permission_decision: PermissionDecision::Deny,
            permission_decision_reason: reason,
        },
    };
    println!("{}", serde_json::to_string(&decision)?);
    Ok(())
}

/// 標準入力を読み、止めるべきなら理由を返す。
fn deny_reason() -> Result<Option<&'static str>> {
    let mut body = String::new();
    io::stdin()
        .read_to_string(&mut body)
        .context("PreToolUse フックの入力を読めませんでした")?;
    let event: Event =
        serde_json::from_str(&body).context("PreToolUse フックの入力を解せませんでした")?;

    judge(&event.tool_input.command)
}

/// コマンドを語へ分けて規則に照らす。当たらなければ `None`。
///
/// 語へ分けられない入力は誤りとして返す。読み落ちは規則に反していないことの根拠にならない。
fn judge(command: &str) -> Result<Option<&'static str>> {
    let words = shlex::split(command)
        .context("コマンドの引用が閉じていないため、語へ分けられませんでした")?;

    if words.iter().any(|word| is_reader(word)) {
        return Ok(Some(READ_REASON));
    }

    if EXIT_CAPTURES
        .iter()
        .any(|capture| command.contains(capture))
    {
        return Ok(Some(EXIT_REASON));
    }

    Ok(None)
}

/// 語がファイル閲覧コマンドを指すか。`/bin/cat` のような絶対指定も同じものとして見る。
fn is_reader(word: &str) -> bool {
    word.rsplit('/')
        .next()
        .is_some_and(|name| READERS.contains(&name))
}

#[cfg(test)]
mod tests {
    //! 規則に当たるコマンドが止まり、当たらないコマンドが通ることを固定する。

    use super::{EXIT_REASON, READ_REASON, judge};
    use crate::Result;

    #[test]
    fn reading_a_file_through_the_shell_is_denied() -> Result<()> {
        assert_eq!(judge("cat README.md")?, Some(READ_REASON));
        assert_eq!(judge("head -5 README.md")?, Some(READ_REASON));
        assert_eq!(judge("sed -n 1p README.md")?, Some(READ_REASON));
        assert_eq!(judge("nix flake check 2>&1 | tail -20")?, Some(READ_REASON));
        assert_eq!(judge("/bin/cat README.md")?, Some(READ_REASON));
        Ok(())
    }

    #[test]
    fn capturing_the_exit_code_is_denied() -> Result<()> {
        assert_eq!(judge("cargo xtask check; exit=$?")?, Some(EXIT_REASON));
        assert_eq!(judge(r#"cargo build; echo "exit=$?""#)?, Some(EXIT_REASON));
        assert_eq!(judge("cargo xtask check; echo ${?}")?, Some(EXIT_REASON));
        Ok(())
    }

    #[test]
    fn a_quoted_exit_code_spelling_is_denied() -> Result<()> {
        // 綴りの包含だけを見るため、引用の中に書いただけの文言も当たる。
        assert_eq!(
            judge("git commit -m 'feat(xtask): 終了コードを $? で退避する操作を止める'")?,
            Some(EXIT_REASON)
        );
        Ok(())
    }

    #[test]
    fn a_quoted_argument_is_one_word() -> Result<()> {
        // 引用の中は 1 語にまとまるため、文言全体が規則の綴りと一致しない限り当たらない。
        assert_eq!(
            judge(r#"git commit -m "feat(docs): cat の扱いを追記""#)?,
            None
        );
        assert_eq!(
            judge("gh pr comment 113 --body 'ご指摘のとおり cat をやめました'")?,
            None
        );
        Ok(())
    }

    #[test]
    fn a_word_that_merely_contains_a_command_name_passes() -> Result<()> {
        // 語全体で突き合わせる。部分一致で止めると関係のないコマンドまで落ちる。
        assert_eq!(judge("nix profile install nixpkgs#gnused")?, None);
        assert_eq!(judge("git log --format=%h")?, None);
        Ok(())
    }

    #[test]
    fn an_ordinary_command_passes() -> Result<()> {
        assert_eq!(judge("nix build .#foo")?, None);
        assert_eq!(judge("cargo xtask check static")?, None);
        assert_eq!(judge("")?, None);
        Ok(())
    }

    #[test]
    fn a_command_that_cannot_be_split_into_words_is_not_passed() {
        assert!(judge("cat 'README.md").is_err());
    }
}
