//! ノート取得元レジストリ（provenance の学習・再利用）の wire/ドメイン型と決定論規則。
//!
//! 利用者要件: (3) どこからノートを取得したか（provenance）を repo 管理のファイルへ保存し、
//! (4) 次回以降はそのレジストリを参照して再利用し再探索しない。再利用 hit したパッケージは保存 source を直接
//! fetch した seed ノートを抽出 port へ渡し、**ツール探索なしの要約のみ 1 回**の GitHub Models 呼び出しで済む。
//! 未知ノート（registry/機械解決で seed が取れないもの）だけが tool-use 探索（最大 model 呼び出し数回）を要する。
//! よって registry が回を追って埋まるほど GitHub Models のレート消費が実際に逓減する。本 module はレジストリの
//! 「パッケージ名 → 取得元」マップとその更新規則（決定論・安定ソート）だけを domain rule として固定する。
//! ファイル I/O（TOML encode/decode）は adapter（`adapters/registry_store.rs`）が担い、本 module は `toml`
//! クレートへ依存しない純粋 domain である。再利用判断（origin 別の再探索要否）は [`NotesSourceEntry::reusable_source`]
//! が持ち、seed の有無による model 呼び出し回数の切替（要約のみ 1 回 / 探索）は抽出 port 実装（adapter）が担う。
//!
//! 信頼境界: レジストリは repo 管理（レビュー対象）だが、AI-discovered で書き込む `source` URL は AI 由来で
//! ある。レジストリへ書く URL は記録前に host allowlist（[`super::validate::is_allowed_url`]）で機械検証し、
//! 許可外 host の source は **学習しない**（`origin=none` へ倒す）。これにより次回参照（フロー 1）でも許可外
//! URL を fetch しない。host 検証は application（`record`）が記録直前に適用し、本 domain 型は origin と source
//! の対応規則・安定ソートだけを持つ。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::diff::DeltaSource;

/// ノート取得元の出所（どの解決経路で取得元が確定したか）。
///
/// `origin` は「次回も再探索すべきか」の判断材料になる: `Mechanical` / `AiDiscovered` は有効な取得元が
/// 確定済みなので次回はその `source` を直接 fetch して再探索しない。`NoneFound` は有効な取得元が見つから
/// なかった記録であり、次回も探索対象（機械解決 → AI 探索）に戻す（取得元が後から現れる可能性に追従する）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum NotesOrigin {
    /// 機械解決（Releases API range / changelog 解決）で取得元 URL が確定した。
    Mechanical,
    /// AI エージェント探索が実際に fetch して有効ノートを得た取得元 URL が確定した。
    AiDiscovered,
    /// 有効な取得元が見つからなかった（version-only へ縮退）。次回も探索対象に戻す。
    None,
}

/// レジストリ 1 エントリ（1 パッケージの provenance）。
///
/// `source` は実際にノートを取得した URL（許可ホスト https。`origin=none` では `None`）。`origin` は出所。
/// `discovered_at` は記録時刻（任意・人間可読の RFC3339）、`note` は任意の人間可読メモ。`source` 以外は
/// 運用補助であり、再利用判断（フロー 1）は `origin` と `source` だけで行う。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct NotesSourceEntry {
    /// 実際にノートを取得した URL（許可ホスト https。`origin=none` では `None`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<String>,
    /// 取得元の出所（再探索要否の判断材料）。
    pub(crate) origin: NotesOrigin,
    /// 記録時刻（任意・人間可読の RFC3339）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) discovered_at: Option<String>,
    /// 任意の人間可読メモ。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) note: Option<String>,
}

impl NotesSourceEntry {
    /// レジストリ参照（フロー 1）で再利用できる有効な保存 source を返す。
    ///
    /// `origin=none`（取得元未発見）や `source` 不在では `None` を返し、application は機械解決 → AI 探索へ
    /// 進む。`Mechanical` / `AiDiscovered` で `source` が在るときだけ、その URL を直接 fetch する再利用対象に
    /// なる（再探索しない）。再利用前の host 妥当性検査は application が記録時に既に通している前提だが、
    /// レジストリは repo 管理で人手改変もありうるため、再利用側でも host allowlist を再適用する。
    pub(crate) fn reusable_source(&self) -> Option<&str> {
        match self.origin {
            NotesOrigin::Mechanical | NotesOrigin::AiDiscovered => self.source.as_deref(),
            NotesOrigin::None => None,
        }
    }
}

/// パッケージ名 → 取得元エントリのレジストリ（決定論・安定ソートの map）。
///
/// 内部は `BTreeMap` でパッケージ名昇順を保ち、TOML 直列化の diff を最小化する。読み書きは adapter が
/// 行い、本型は lookup（参照）と upsert（記録）の純粋規則だけを持つ。同一パッケージの再記録は上書きする
/// （自己修復で新しい取得元へ追従するため）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct NotesSourceRegistry {
    /// パッケージ名 → 取得元エントリ（`BTreeMap` で名前昇順＝決定論・安定 diff）。
    entries: BTreeMap<String, NotesSourceEntry>,
}

/// パッケージ名と出所からレジストリの一意キーを組み立てる純粋関数（finding 3369076719）。
///
/// nix 由来と brew 由来は同名でも別パッケージ（例: nix の `firefox` と cask の `firefox`）であり、ノート取得元
/// provenance を name だけで突合すると別出所の取得元を取り違える。キーへ出所を含めて `<source>/<name>`
/// （例 `nix/firefox` / `brew/firefox`）にし、同名でも出所が違えば別エントリとして学習・再利用する。出所は
/// [`DeltaSource::as_stable_key`]（`Debug` 表現非依存）を使い、決定論を保つ。`<source>/` 前置のため `BTreeMap`
/// の昇順整列は出所ごとにまとまり、直列化 diff も安定する。
pub(crate) fn registry_key(name: &str, source: DeltaSource) -> String {
    format!("{}/{name}", source.as_stable_key())
}

impl NotesSourceRegistry {
    /// 指定パッケージ（名前 + 出所）の保存済みエントリを参照する（無ければ `None`）。
    ///
    /// キーは出所込み（[`registry_key`]）。同名でも nix/brew は別エントリとして引く（finding 3369076719）。
    pub(crate) fn lookup(&self, name: &str, source: DeltaSource) -> Option<&NotesSourceEntry> {
        self.entries.get(&registry_key(name, source))
    }

    /// 指定パッケージ（名前 + 出所）の取得元を記録（追記/上書き）する。
    ///
    /// 既存エントリがあれば上書きする（自己修復で取得元が移動したプロジェクトに追従するため）。キーは出所込み
    /// （[`registry_key`]）で、同名でも nix/brew を別エントリとして学習する（finding 3369076719）。`BTreeMap`
    /// なので挿入後も名前昇順を保ち、直列化は決定論になる。
    pub(crate) fn record(&mut self, name: &str, source: DeltaSource, entry: NotesSourceEntry) {
        self.entries.insert(registry_key(name, source), entry);
    }
}

#[cfg(test)]
mod tests {
    //! レジストリの参照・記録・再利用判断（origin 別）と、TOML 直列化が決定論（名前昇順）であることを固定する。

    use super::*;

    fn entry(source: Option<&str>, origin: NotesOrigin) -> NotesSourceEntry {
        NotesSourceEntry {
            source: source.map(str::to_string),
            origin,
            discovered_at: None,
            note: None,
        }
    }

    #[test]
    fn reusable_source_only_for_resolved_origins() {
        // 退行固定（再利用判断）: mechanical / ai-discovered で source が在れば再利用、none は常に再探索。
        assert_eq!(
            entry(
                Some("https://github.com/o/r/releases"),
                NotesOrigin::Mechanical
            )
            .reusable_source(),
            Some("https://github.com/o/r/releases")
        );
        assert_eq!(
            entry(
                Some("https://github.com/o/r/blob/x"),
                NotesOrigin::AiDiscovered
            )
            .reusable_source(),
            Some("https://github.com/o/r/blob/x")
        );
        // origin=none は source が在っても再利用しない（次回も探索対象に戻す）。
        assert_eq!(entry(None, NotesOrigin::None).reusable_source(), None);
        assert_eq!(
            entry(Some("https://github.com/o/r"), NotesOrigin::None).reusable_source(),
            None
        );
    }

    #[test]
    fn record_upserts_and_lookup_reads_back() {
        let mut registry = NotesSourceRegistry::default();
        assert!(registry.lookup("neovim", DeltaSource::NixEval).is_none());
        registry.record(
            "neovim",
            DeltaSource::NixEval,
            entry(
                Some("https://github.com/neovim/neovim/releases"),
                NotesOrigin::Mechanical,
            ),
        );
        assert_eq!(
            registry
                .lookup("neovim", DeltaSource::NixEval)
                .and_then(|e| e.reusable_source()),
            Some("https://github.com/neovim/neovim/releases")
        );
        // 自己修復: 同一パッケージ・同一出所の再記録は上書きする（取得元が移動したプロジェクトに追従）。
        registry.record(
            "neovim",
            DeltaSource::NixEval,
            entry(
                Some("https://github.com/neovim/neovim/blob/master/CHANGELOG"),
                NotesOrigin::AiDiscovered,
            ),
        );
        assert_eq!(
            registry
                .lookup("neovim", DeltaSource::NixEval)
                .map(|e| e.origin),
            Some(NotesOrigin::AiDiscovered)
        );
    }

    #[test]
    fn same_name_nix_and_brew_are_kept_separate() {
        // 退行固定（finding 3369076719）: 同名 `firefox` でも nix 由来と brew 由来は別パッケージであり、
        // provenance を別エントリとして学習・参照する。name だけで突合すると別出所の取得元を取り違える。
        let mut registry = NotesSourceRegistry::default();
        registry.record(
            "firefox",
            DeltaSource::NixEval,
            entry(
                Some("https://github.com/mozilla/firefox/releases"),
                NotesOrigin::Mechanical,
            ),
        );
        registry.record(
            "firefox",
            DeltaSource::BrewTap,
            entry(
                Some("https://github.com/homebrew/homebrew-cask/blob/x/firefox.rb"),
                NotesOrigin::AiDiscovered,
            ),
        );
        // 出所ごとに別エントリとして引ける（取り違えない）。
        assert_eq!(
            registry
                .lookup("firefox", DeltaSource::NixEval)
                .and_then(|e| e.reusable_source()),
            Some("https://github.com/mozilla/firefox/releases")
        );
        assert_eq!(
            registry
                .lookup("firefox", DeltaSource::BrewTap)
                .and_then(|e| e.reusable_source()),
            Some("https://github.com/homebrew/homebrew-cask/blob/x/firefox.rb")
        );
        // キーは出所込み（`<source>/<name>`）。
        assert_eq!(registry_key("firefox", DeltaSource::NixEval), "nix/firefox");
        assert_eq!(
            registry_key("firefox", DeltaSource::BrewTap),
            "brew/firefox"
        );
    }

    #[test]
    fn registry_serializes_deterministically_in_name_order() -> crate::Result<()> {
        // 決定論固定: 挿入順に依らず TOML はキー（`<source>/<name>`）昇順で直列化され diff を最小化する。
        let mut registry = NotesSourceRegistry::default();
        registry.record(
            "ripgrep",
            DeltaSource::NixEval,
            entry(
                Some("https://github.com/BurntSushi/ripgrep/releases"),
                NotesOrigin::Mechanical,
            ),
        );
        registry.record(
            "bat",
            DeltaSource::NixEval,
            entry(
                Some("https://github.com/sharkdp/bat/releases"),
                NotesOrigin::AiDiscovered,
            ),
        );
        registry.record("zlib", DeltaSource::NixEval, entry(None, NotesOrigin::None));

        let rendered = toml::to_string(&registry)?;
        // キーは `nix/<name>` で、TOML テーブル名はクォートされる（`/` を含むため）。昇順で並ぶ。
        let expected = "\
[\"nix/bat\"]
source = \"https://github.com/sharkdp/bat/releases\"
origin = \"ai-discovered\"

[\"nix/ripgrep\"]
source = \"https://github.com/BurntSushi/ripgrep/releases\"
origin = \"mechanical\"

[\"nix/zlib\"]
origin = \"none\"
";
        assert_eq!(rendered, expected);
        // 往復しても同値（名前順・origin 値の保存）。
        let parsed: NotesSourceRegistry = toml::from_str(&rendered)?;
        assert_eq!(parsed, registry);
        Ok(())
    }
}
