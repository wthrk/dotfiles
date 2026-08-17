//! Claude Code の `PreToolUse` から呼ばれ、`Bash` へ渡されたコマンドを止める判定を返す。
//!
//! 対象は `docs/task-governance/workflow.md`「9. コマンド操作の機械強制」が挙げる 2 件だけである。
//! どちらも繰り返し破られたため、規約文と別に、実行前へ同じ規則を評価する経路をここに置く。
//!
//! 判定はシェルが実行する語と展開に対して行う。単一引用の中はコミットメッセージ本文のような文言で
//! あり、実行される語と区別できないため、規則の突き合わせに使わない。二重引用の中は文言に見えても
//! `$?` が展開され `$(...)` と backtick が実行されるため、その綴りだけは引用の外と同じに扱う。
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
    hook_event_name: &'static str,
    permission_decision: &'static str,
    permission_decision_reason: String,
}

/// ファイル閲覧をシェルへ流したときの理由。
const READ_REASON: &str = "ファイル閲覧は Read ツールで行ってください。`cat` / `head` / `tail` / `sed` をシェルから使うと、読んだ範囲が出力に残らず、切り詰めた一部だけで判断することになります（docs/task-governance/workflow.md「コマンド操作の機械強制」）。";

/// 終了コードを退避したときの理由。
const EXIT_REASON: &str = "終了コードを `$?` で退避しないでください。退避して出し直すと、コマンド自身の失敗が後続の成功で覆われます（docs/task-governance/workflow.md「コマンド操作の機械強制」）。";

/// 語として現れたら止めるコマンド名。
const READERS: [&str; 4] = ["cat", "head", "tail", "sed"];

/// 展開される位置に現れたら止める綴り。
const EXIT_CAPTURES: [&str; 2] = ["$?", "${?}"];

/// 引用の外で語を切るシェルのメタ文字。
///
/// backtick と、コマンド置換の中に現れる括弧は、開いた入れ子を閉じる位置まで追う必要があるため、
/// この表に任せず `views` で個別に扱う。
const SEPARATORS: [char; 8] = ['|', '&', ';', '<', '>', '(', ')', '\n'];

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
            hook_event_name: "PreToolUse",
            permission_decision: "deny",
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
/// 語を確定できない入力は、規則に反していないことの根拠にならないため誤りとして返す。
fn judge(command: &str) -> Result<Option<&'static str>> {
    let views = views(command);
    let words = shlex::split(&views.separated)
        .context("コマンドの引用が閉じていないため、語へ分けられませんでした")?;

    if EXIT_CAPTURES
        .iter()
        .any(|capture| views.expanded.contains(capture))
    {
        return Ok(Some(EXIT_REASON));
    }

    if words.iter().any(|word| is_reader(word)) {
        return Ok(Some(READ_REASON));
    }

    Ok(None)
}

/// 判定に使う、同じコマンドの 2 つの写し。
struct Views {
    /// 区切りを空白へ置き換え、コマンド置換の本体を引用の外へ出した文字列。ここから語を切り出す。
    separated: String,
    /// `separated` のうち、シェルが展開または実行する綴りだけを残した文字列。
    ///
    /// 単一引用の中と、二重引用の中の文言はここに残らない。
    expanded: String,
}

/// `views` が写しながら追う入れ子の種類。空なら引用にも置換にも囲まれていない位置を指す。
#[derive(Clone, Copy)]
enum Frame {
    /// 単一引用の中。展開も置換も起きないため、中身は文言として扱う。
    SingleQuote,
    /// 二重引用の中。語は切れないが、`$?` は展開され `$(...)` と backtick は実行される。
    DoubleQuote,
    /// `$(` で開いたコマンド置換の中。閉じ括弧まで引用の外と同じに扱う。
    Substitution,
    /// コマンド置換の中で `(` が開いた subshell の中。
    ///
    /// 数えずに写すと、`$( (true); cat README.md)` の内側の `)` で置換を閉じたことにしてしまい、
    /// シェルが置換の中で実行する後続の語が二重引用の文言へ紛れる。
    Group,
    /// backtick で開いたコマンド置換の中。次の backtick まで引用の外と同じに扱う。
    Backtick,
}

/// 引用、escape、コマンド置換の入れ子を追いながらコマンド文字列を写す。
///
/// メタ文字を空白へ置き換えるのは、`2>&1|tail` のように区切りが語へ密着していても語を取り出せる
/// ようにするためである。引用そのものは残し、語へまとめるのは `shlex` に任せる。
///
/// 二重引用は語を切らないため中身をそのまま残すが、その中でコマンド置換が始まったら `separated`
/// 側の二重引用を一度閉じ、閉じ位置で開き直す。閉じないと `shlex` が置換の本体まで 1 語へまとめ、
/// `"$(cat README.md)"` の `cat` が語として現れない。
fn views(command: &str) -> Views {
    let characters: Vec<char> = command.chars().collect();
    let mut separated = String::with_capacity(command.len());
    let mut expanded = String::with_capacity(command.len());
    let mut frames: Vec<Frame> = Vec::new();
    let mut index = 0;

    while let Some(&character) = characters.get(index) {
        match frames.last().copied() {
            Some(Frame::SingleQuote) => {
                separated.push(character);
                if character == '\'' {
                    frames.pop();
                }
                index += 1;
            }
            Some(Frame::DoubleQuote) => {
                if character == '\\' {
                    // 二重引用の中の escape は次の 1 文字の展開を打ち消す。文言として写す。
                    separated.push(character);
                    if let Some(&literal) = characters.get(index + 1) {
                        separated.push(literal);
                        index += 1;
                    }
                    index += 1;
                } else if character == '"' {
                    separated.push(character);
                    frames.pop();
                    index += 1;
                } else if let Some(capture) = exit_capture_at(&characters, index) {
                    separated.push_str(capture);
                    expanded.push_str(capture);
                    index += capture.chars().count();
                } else if opens_substitution(&characters, index) {
                    separated.push('"');
                    separated.push(' ');
                    expanded.push(' ');
                    frames.push(Frame::Substitution);
                    index += 2;
                } else if character == '`' {
                    separated.push('"');
                    separated.push(' ');
                    expanded.push(' ');
                    frames.push(Frame::Backtick);
                    index += 1;
                } else {
                    separated.push(character);
                    index += 1;
                }
            }
            frame => {
                if character == '\\' {
                    separated.push(character);
                    if let Some(&literal) = characters.get(index + 1) {
                        separated.push(literal);
                        expanded.push(literal);
                        index += 1;
                    }
                    index += 1;
                } else if character == '\'' {
                    frames.push(Frame::SingleQuote);
                    separated.push(character);
                    index += 1;
                } else if character == '"' {
                    frames.push(Frame::DoubleQuote);
                    separated.push(character);
                    index += 1;
                } else if opens_substitution(&characters, index) {
                    frames.push(Frame::Substitution);
                    separated.push_str("  ");
                    expanded.push_str("  ");
                    index += 2;
                } else if character == '`' {
                    separated.push(' ');
                    expanded.push(' ');
                    if matches!(frame, Some(Frame::Backtick)) {
                        frames.pop();
                        reopen_double_quote(&frames, &mut separated);
                    } else {
                        frames.push(Frame::Backtick);
                    }
                    index += 1;
                } else if character == '('
                    && matches!(frame, Some(Frame::Substitution | Frame::Group))
                {
                    frames.push(Frame::Group);
                    separated.push(' ');
                    expanded.push(' ');
                    index += 1;
                } else if character == ')'
                    && matches!(frame, Some(Frame::Substitution | Frame::Group))
                {
                    frames.pop();
                    separated.push(' ');
                    expanded.push(' ');
                    reopen_double_quote(&frames, &mut separated);
                    index += 1;
                } else if SEPARATORS.contains(&character) {
                    separated.push(' ');
                    expanded.push(' ');
                    index += 1;
                } else {
                    separated.push(character);
                    expanded.push(character);
                    index += 1;
                }
            }
        }
    }

    Views {
        separated,
        expanded,
    }
}

/// 索引位置から `$(` が始まるか。
fn opens_substitution(characters: &[char], index: usize) -> bool {
    characters.get(index) == Some(&'$') && characters.get(index + 1) == Some(&'(')
}

/// 索引位置から始まる終了コードの綴りを返す。当たらなければ `None`。
fn exit_capture_at(characters: &[char], index: usize) -> Option<&'static str> {
    EXIT_CAPTURES.iter().copied().find(|capture| {
        capture.chars().eq(characters[index..]
            .iter()
            .copied()
            .take(capture.chars().count()))
    })
}

/// 置換を閉じた位置が二重引用の中なら、`separated` 側の引用を開き直す。
///
/// 開き直さないと、置換の直後に現れる閉じ引用が新しい引用の開始になり、`shlex` が語を取り違える。
fn reopen_double_quote(frames: &[Frame], separated: &mut String) {
    if matches!(frames.last(), Some(Frame::DoubleQuote)) {
        separated.push('"');
    }
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
        assert_eq!(judge("cargo test | tail -20")?, Some(READ_REASON));
        assert_eq!(judge("/bin/cat README.md")?, Some(READ_REASON));
        Ok(())
    }

    #[test]
    fn a_reader_glued_to_a_metacharacter_is_denied() -> Result<()> {
        // 区切りが語へ密着していても、シェルは `tail` を別の語として実行する。
        assert_eq!(judge("nix flake check 2>&1|tail -20")?, Some(READ_REASON));
        Ok(())
    }

    #[test]
    fn capturing_the_exit_code_is_denied() -> Result<()> {
        assert_eq!(judge("cargo xtask check; exit=$?")?, Some(EXIT_REASON));
        assert_eq!(judge("cargo xtask check; echo ${?}")?, Some(EXIT_REASON));
        Ok(())
    }

    #[test]
    fn capturing_the_exit_code_inside_double_quotes_is_denied() -> Result<()> {
        // 二重引用は語を切らないだけで、`$?` はそのまま展開される。
        assert_eq!(judge(r#"cargo build; echo "exit=$?""#)?, Some(EXIT_REASON));
        assert_eq!(
            judge(r#"cargo xtask check; rc="$?"; echo $rc"#)?,
            Some(EXIT_REASON)
        );
        assert_eq!(
            judge(r#"cargo xtask check; echo "${?}""#)?,
            Some(EXIT_REASON)
        );
        Ok(())
    }

    #[test]
    fn a_reader_inside_a_command_substitution_is_denied() -> Result<()> {
        // 置換の本体は引用の中にあっても実行される。
        assert_eq!(judge("echo $(cat README.md)")?, Some(READ_REASON));
        assert_eq!(judge(r#"echo "$(cat README.md)""#)?, Some(READ_REASON));
        assert_eq!(
            judge("cargo build; echo `cat README.md`")?,
            Some(READ_REASON)
        );
        assert_eq!(judge(r#"echo "`cat README.md`""#)?, Some(READ_REASON));
        // 置換の中の `(` は subshell を開くだけであり、その閉じ括弧では置換は終わらない。
        assert_eq!(
            judge(r#"echo "$( (true); cat README.md)""#)?,
            Some(READ_REASON)
        );
        assert_eq!(
            judge(r#"echo "$( (cd rust; cargo build) 2>&1 | tail -20 )""#)?,
            Some(READ_REASON)
        );
        Ok(())
    }

    #[test]
    fn an_escaped_expansion_inside_double_quotes_passes() -> Result<()> {
        // `\$` は展開されないため、文言として扱う。
        assert_eq!(judge(r#"echo "\$? は退避しない""#)?, None);
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
    fn a_quoted_argument_is_one_word() -> Result<()> {
        // 展開も置換も起きない引用の中は利用者が書いた文言であり、シェルが実行する語ではない。
        assert_eq!(
            judge(r#"git commit -m "feat(docs): cat の扱いを追記""#)?,
            None
        );
        assert_eq!(
            judge("gh pr comment 113 --body 'ご指摘のとおり cat をやめました'")?,
            None
        );
        assert_eq!(
            judge(r#"git commit -m "feat(docs): add cat handling notes""#)?,
            None
        );
        assert_eq!(
            judge("git commit -m 'feat(xtask): 終了コードを $? で退避する操作を止める'")?,
            None
        );
        Ok(())
    }

    #[test]
    fn an_ordinary_command_passes() -> Result<()> {
        assert_eq!(judge("cargo xtask check static")?, None);
        assert_eq!(judge("")?, None);
        Ok(())
    }

    #[test]
    fn a_command_that_cannot_be_split_into_words_is_not_passed() {
        assert!(judge("cat 'README.md").is_err());
    }
}
