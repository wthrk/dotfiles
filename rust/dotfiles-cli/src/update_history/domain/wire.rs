//! 更新履歴 TOML の wire/ドメイン型と閉集合 enum。
//!
//! field 名と enum 値はプラン確定の TOML スキーマ（`docs/update-history/<YYYY-MM>.toml`）に一致させる。
//! `ref` は Rust 予約語のため serde rename で TOML key `ref` に対応させる。閉集合（変更種別・変更
//! カテゴリ・重要度）は生文字列ではなく enum で表し、serde rename で TOML 値（kebab-case 含む）へ写す。
//!
//! これらは domain value であり、`toml` クレートの具体型へは依存しない。encode/decode は adapter が
//! serde derive を介して行う。不変条件（severity が変更カテゴリから機械算出されること等）は
//! sibling module（`severity` / `aggregate`）の domain 関数として固定する。

use serde::{Deserialize, Serialize};

/// 1 回の nightly bump で記録される更新エントリ（TOML `[[update]]` 1 件に対応）。
///
/// `at` はエントリ単位の RFC3339 タイムスタンプであり、暦日キーは持たない（1 ファイルに 1 日複数件
/// 入りうる）。`severity` / `overall` はエントリ全体の重要度・機械見出しで、いずれも `packages` の
/// 変更カテゴリから決定論的に算出される（[`super::severity`] 参照）。本型は wire 表現の保持に徹し、
/// 算出規則そのものは domain 関数側に置く。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UpdateEntry {
    /// 適用時刻（RFC3339。CI が `--at` で注入する文字列をそのまま保持する）。
    pub(crate) at: String,
    /// bump 前の nixpkgs リビジョン。
    pub(crate) nixpkgs_old: String,
    /// bump 後の nixpkgs リビジョン。
    pub(crate) nixpkgs_new: String,
    /// diff 対象の参照構成（例: `darwinConfigurations.<ref>`）。
    pub(crate) reference: String,
    /// 変更カテゴリ集合から機械算出した全体重要度。
    pub(crate) severity: Severity,
    /// 「N アプリ更新: 🔒2 ⚠️1 ✨3」形式の機械見出し。
    pub(crate) overall: String,
    /// このエントリで更新された各パッケージ。
    #[serde(default, rename = "package")]
    pub(crate) packages: Vec<PackageUpdate>,
}

/// パッケージ更新の出所（nix closure か Homebrew cask か）。catch-up 集約の同一性キーの一部。
///
/// nix と brew は同名パッケージ（例 `firefox`: nixpkgs の firefox と cask の firefox）を別物として記録する
/// 設計であり、表示時集約（[`super::aggregate`]）が `name` だけで畳むと old/new・declared・notes_url が後勝ちで
/// 誤表示される。出所を同一性キーへ含めるため、wire（記録）にも source を残す。TOML 値は lowercase
/// （`nix`/`brew`）。旧スキーマ（source 無し）の後方互換は [`PackageUpdate::source`] の `serde(default)` が担う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PackageSource {
    /// nix eval 由来（宣言パッケージの name→version 差分）。
    Nix,
    /// Homebrew tap 由来（cask/formula の版差分）。
    Brew,
}

impl Default for PackageSource {
    /// 旧スキーマ（source field を持たない既存 TOML）の deserialize 既定。
    ///
    /// source 導入前に記録されたエントリは出所を持たない。既定を `Nix` に倒すのは、旧運用の宣言パッケージ
    /// 記録が主に nix eval 由来であり、同名衝突（nix/brew firefox）の現実的頻度が低いことによる保守的既定で
    /// ある。新規記録は record 経路が常に明示 source を書くため、この既定は後方互換読み出しにのみ効く。
    fn default() -> Self {
        PackageSource::Nix
    }
}

impl PackageSource {
    /// dedup・集約の決定論キーで使う安定文字列を返す（serde wire 文字列と一致）。
    pub(crate) fn as_stable_key(&self) -> &'static str {
        match self {
            PackageSource::Nix => "nix",
            PackageSource::Brew => "brew",
        }
    }
}

/// 適用後要約を「実際に適用した target に対応する出所」だけへ絞る domain policy。
///
/// `dotfiles update home`（zsh catch-up / daemon の home step）は home-manager だけを switch するが、CI 履歴は
/// `home.packages`（nix）に加えて `environment.systemPackages`（nix）と brew cask（brew）も含む。home だけ適用
/// した直後に全体履歴を要約すると、未適用の cask（Firefox 等）まで適用済みのように `pending-summary` へ出る
/// （finding 3368653947）。これを避け、適用した target に対応する出所だけを残す。
///
/// 出所判別の粒度は wire の [`PackageSource`]（nix / brew）に限られる。`home` 適用は home-manager の nix
/// パッケージなので **brew cask を除外**して nix 出所だけを残す（`environment.systemPackages` も nix 出所のため
/// 同列に残るが、cask の誤通知という finding の中核は塞ぐ。nix の home/system 分離は wire スキーマに無く本 filter の
/// 対象外）。`darwin` 適用は systemPackages（nix）+ cask（brew）の双方を適用するため全出所を残す。target 省略
/// （全体適用）も全出所を残す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackageSourceFilter {
    /// 全出所を残す（全体適用 / darwin 適用）。
    All,
    /// nix 出所だけ残す（home 適用。brew cask を除外して未適用 cask の誤通知を防ぐ）。
    NixOnly,
}

impl Default for PackageSourceFilter {
    /// 既定は全出所（利用者 `show` や全体適用の要約は絞らない）。
    fn default() -> Self {
        PackageSourceFilter::All
    }
}

impl PackageSourceFilter {
    /// この filter が指定 [`PackageSource`] の package を要約へ含めてよいかを返す。
    ///
    /// `All` は常に真。`NixOnly` は nix 出所だけ真（brew は偽）。集約後の package 絞り込みで使う。
    pub(crate) fn includes(&self, source: PackageSource) -> bool {
        match self {
            PackageSourceFilter::All => true,
            PackageSourceFilter::NixOnly => matches!(source, PackageSource::Nix),
        }
    }
}

/// 1 アプリ/パッケージの version 差分と構造化変更リスト（TOML `[[update.package]]` に対応）。
///
/// `old` / `new` は `added` / `removed` で片側が `None` になりうるため `Option` で保持する。
/// `declared` は宣言アプリ（`show` 既定で表示）か低レベル依存（既定で畳む）かの区別であり、
/// `change_items` は LLM 抽出済みの構造化変更（取得不能/未抽出なら空）を保持する。`source` は出所
/// （nix/brew）で、catch-up 集約の同一性キーに含める（同名 nix/brew パッケージを 1 件に潰さないため）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PackageUpdate {
    /// パッケージ/アプリ名。catch-up 集約の同一性キーの一部（`source` と対）。
    pub(crate) name: String,
    /// 更新前 version（`added` では `None`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) old: Option<String>,
    /// 更新後 version（`removed` では `None`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) new: Option<String>,
    /// version 差分の種別。
    pub(crate) change: ChangeKind,
    /// 宣言アプリなら `true`（`show` 既定で表示）、低レベル依存なら `false`。
    pub(crate) declared: bool,
    /// 更新の出所（nix/brew）。catch-up 集約の同一性キーの一部。旧スキーマ（source 無し）は
    /// `serde(default)` で [`PackageSource::Nix`] へ縮退する。
    #[serde(default)]
    pub(crate) source: PackageSource,
    /// リリースノート/changelog の URL（取得不能なら `None`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) notes_url: Option<String>,
    /// 構造化変更リスト（LLM 抽出。空なら変更概要なし）。
    #[serde(default, rename = "change_item")]
    pub(crate) change_items: Vec<ChangeItem>,
}

/// 1 件の構造化変更（TOML `[[update.package.change_item]]` に対応）。
///
/// `category` は閉集合 enum で severity 算出の根拠になり、`text` は日本語 1 行の概要、
/// `ref_url` はその変更の PR/issue/release URL（任意）。catch-up 集約の重複排除は
/// 決定論キー `(name, category, ref_url, text)` で行うため、本型の `category` / `ref_url` / `text` が
/// 同一性に効く（同一 category・同一 `ref_url` でも `text` が異なれば別の変更として保持される）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ChangeItem {
    /// 変更カテゴリ（severity 算出と表示グルーピングの根拠）。
    pub(crate) category: ChangeCategory,
    /// 簡潔な 1 行概要（日本語）。表示時はプレーン表示する契約（injection 耐性）。
    pub(crate) text: String,
    /// その変更の参照 URL。TOML key は予約語回避のため `ref`。
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    pub(crate) ref_url: Option<String>,
}

/// version 差分の種別（閉集合）。TOML 値は snake/lower 表現に一致させる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ChangeKind {
    /// version が上がった。
    Upgraded,
    /// version が下がった。
    Downgraded,
    /// 新規追加された。
    Added,
    /// 削除された。
    Removed,
}

/// 構造化変更のカテゴリ（閉集合）。TOML 値は kebab-case（`default-change` 等）に一致させる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ChangeCategory {
    /// 破壊的変更。
    Breaking,
    /// セキュリティ修正。
    Security,
    /// 新機能。
    Feature,
    /// バグ修正。
    Fix,
    /// 非推奨化。
    Deprecation,
    /// デフォルト挙動変更。
    DefaultChange,
}

impl ChangeCategory {
    /// dedup・集約の決定論キーで使う安定文字列を返す。
    ///
    /// 返す値は serde の wire 文字列（TOML 値、kebab-case）と一致させ、wire とキーの一貫性を保つ。
    /// `Debug` 派生表現に依存しないことが不変条件である: `Debug` は安定契約ではなく variant 名変更等の
    /// リファクタで表現が変わりうるため、同一入力で dedup キーが変化して集約結果が非決定的になる。
    /// この明示 match を唯一の安定キー源とし、variant 追加時はここに対応値を追加する。
    pub(crate) fn as_stable_key(&self) -> &'static str {
        match self {
            ChangeCategory::Breaking => "breaking",
            ChangeCategory::Security => "security",
            ChangeCategory::Feature => "feature",
            ChangeCategory::Fix => "fix",
            ChangeCategory::Deprecation => "deprecation",
            ChangeCategory::DefaultChange => "default-change",
        }
    }
}

/// エントリ全体の重要度（閉集合）。変更カテゴリ集合から機械算出する（[`super::severity`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Severity {
    /// security 変更を含む。
    Critical,
    /// `ChangeCategory::Breaking`（破壊的変更）または `Deprecation`（非推奨化）を含む（[`super::severity`]）。
    /// severity は `change_items` の category 集合のみから算出するため、パッケージ単位の
    /// `ChangeKind::Removed`（パッケージ削除）はこの severity に影響しない（別レイヤの差分種別）。
    Major,
    /// 機能追加/修正のみ。
    Minor,
    /// 該当する変更がない。
    None,
}

#[cfg(test)]
mod tests {
    //! wire 型の TOML 直列化が プラン確定スキーマ（field 名・enum 値・`ref` rename）に一致することを
    //! バイト固定する。encode/decode 実体は adapter（`adapters/toml_store.rs`）だが、serde 契約は domain の
    //! 不変条件としてここで固定する。

    use super::*;

    fn sample_entry() -> UpdateEntry {
        UpdateEntry {
            at: "2026-06-05T18:00:11Z".to_string(),
            nixpkgs_old: "a1b2c3d".to_string(),
            nixpkgs_new: "e4f5g6h".to_string(),
            reference: "darwinConfigurations.ci".to_string(),
            severity: Severity::Critical,
            overall: "1アプリ更新: 🔒1 ✨1".to_string(),
            packages: vec![PackageUpdate {
                name: "neovim".to_string(),
                old: Some("0.10.2".to_string()),
                new: Some("0.11.0".to_string()),
                change: ChangeKind::Upgraded,
                declared: true,
                source: PackageSource::Nix,
                notes_url: Some(
                    "https://github.com/neovim/neovim/releases/tag/v0.11.0".to_string(),
                ),
                change_items: vec![
                    ChangeItem {
                        category: ChangeCategory::Security,
                        text: "セキュリティ修正".to_string(),
                        ref_url: Some("https://github.com/neovim/neovim/pull/1".to_string()),
                    },
                    ChangeItem {
                        category: ChangeCategory::Feature,
                        text: "新機能".to_string(),
                        ref_url: None,
                    },
                ],
            }],
        }
    }

    #[derive(Serialize)]
    struct HistoryDocument {
        #[serde(rename = "update")]
        updates: Vec<UpdateEntry>,
    }

    #[test]
    fn entry_serializes_to_plan_toml_schema() -> crate::Result<()> {
        let document = HistoryDocument {
            updates: vec![sample_entry()],
        };

        let rendered = toml::to_string(&document)?;

        let expected = "\
[[update]]
at = \"2026-06-05T18:00:11Z\"
nixpkgs_old = \"a1b2c3d\"
nixpkgs_new = \"e4f5g6h\"
reference = \"darwinConfigurations.ci\"
severity = \"critical\"
overall = \"1アプリ更新: 🔒1 ✨1\"

[[update.package]]
name = \"neovim\"
old = \"0.10.2\"
new = \"0.11.0\"
change = \"upgraded\"
declared = true
source = \"nix\"
notes_url = \"https://github.com/neovim/neovim/releases/tag/v0.11.0\"

[[update.package.change_item]]
category = \"security\"
text = \"セキュリティ修正\"
ref = \"https://github.com/neovim/neovim/pull/1\"

[[update.package.change_item]]
category = \"feature\"
text = \"新機能\"
";
        assert_eq!(rendered, expected);
        Ok(())
    }

    #[test]
    fn downgraded_change_kind_serializes_and_round_trips_as_downgraded() -> crate::Result<()> {
        // 閉集合 enum の `Downgraded` が TOML 値 `downgraded` に一致し、往復で保存されることを固定する。
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Wrap {
            change: ChangeKind,
        }
        let rendered = toml::to_string(&Wrap {
            change: ChangeKind::Downgraded,
        })?;
        assert_eq!(rendered, "change = \"downgraded\"\n");
        let parsed: Wrap = toml::from_str(&rendered)?;
        assert_eq!(parsed.change, ChangeKind::Downgraded);
        Ok(())
    }

    #[test]
    fn package_source_serializes_lowercase_and_round_trips() -> crate::Result<()> {
        // 出所 enum が TOML 値 `nix`/`brew` に一致し、往復で保存されることを固定する。
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Wrap {
            source: PackageSource,
        }
        let rendered = toml::to_string(&Wrap {
            source: PackageSource::Brew,
        })?;
        assert_eq!(rendered, "source = \"brew\"\n");
        let parsed: Wrap = toml::from_str(&rendered)?;
        assert_eq!(parsed.source, PackageSource::Brew);
        Ok(())
    }

    #[test]
    fn package_source_filter_includes_by_target_source() {
        // finding 3368653947: NixOnly（home 部分適用）は nix を含み brew を除外する。All は両方含む。
        assert!(PackageSourceFilter::All.includes(PackageSource::Nix));
        assert!(PackageSourceFilter::All.includes(PackageSource::Brew));
        assert!(PackageSourceFilter::NixOnly.includes(PackageSource::Nix));
        assert!(
            !PackageSourceFilter::NixOnly.includes(PackageSource::Brew),
            "home 部分適用は brew cask を除外する"
        );
        // 既定は All（利用者 show / 全体適用は絞らない）。
        assert_eq!(PackageSourceFilter::default(), PackageSourceFilter::All);
    }

    #[test]
    fn package_without_source_field_defaults_to_nix() -> crate::Result<()> {
        // 後方互換: source 導入前に記録された `[[update.package]]`（source field 無し）も読めること、
        // かつ `serde(default)` で `PackageSource::Nix` へ縮退することを固定する。
        let toml = "\
name = \"neovim\"
change = \"upgraded\"
declared = true
";
        let parsed: PackageUpdate = toml::from_str(toml)?;
        assert_eq!(parsed.source, PackageSource::Nix);
        assert_eq!(parsed.name, "neovim");
        Ok(())
    }

    #[test]
    fn entry_round_trips_through_toml() -> crate::Result<()> {
        #[derive(Serialize, Deserialize)]
        struct Document {
            #[serde(rename = "update")]
            updates: Vec<UpdateEntry>,
        }

        let original = Document {
            updates: vec![sample_entry()],
        };
        let rendered = toml::to_string(&original)?;
        let parsed: Document = toml::from_str(&rendered)?;

        assert_eq!(parsed.updates, original.updates);
        Ok(())
    }
}
