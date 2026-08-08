//! nightly bump PR を無人 auto-merge してよいかを機械判定する純粋規則。
//!
//! この module は I/O を持たず、CI が収集した「PR の全 commit を base..head で union した変更パス集合」と
//! 「base / head の `flake.lock` 内容」を受け取り、許可された bump だけかを決める。判定する不変条件は 2 つ。
//!
//! 1. **変更パス限定**: PR が触ってよいのは `flake.lock` と `docs/update-history/**` だけで、nightly PR が
//!    workflow / コードを自己改変して無人で main へ入る経路を塞ぐ。net diff ではなく **全 commit の union** を
//!    検査するのは、途中 commit で逸脱パスを足して最終 head で消す add-then-remove を防ぐためであり、
//!    `--squash` マージ運用の有無に依存しない。union パス集合の収集は CLI 側の責務で、本 module は与えられた
//!    集合を判定する。
//!
//! 2. **lock 差分限定**: `flake.lock` の差分は、[`EXPECTED_LOCK_INPUT_SOURCES`] に期待取得先を持つ node の
//!    **rev 変更のみ**。取得先の同一性（owner/repo 厳密一致、type / url / host / dir、node 追加削除、
//!    node 間 wiring、`flake` フラグ）は厳密一致を要求する。唯一の例外は推移 input の `ref` で、親 flake の
//!    宣言に従って動くため無条件に許可する（[`SourceCoords::ignoring_reference`]）。加えて、期待取得先を持つ
//!    input の rev 変更が少なくとも 1 件あることを要求し、実体のない空 bump を無人 merge させない。
//!
//! どちらかに違反すれば [`verify_bump`] は違反理由を載せた `Err` を返し、CLI は非 0 で終了する。

use std::collections::BTreeSet;

use anyhow::{Context, anyhow, bail};
use serde_json::Value;

use crate::Result;

/// nightly bump が触ってよい唯一のファイル（`docs/update-history/**` は prefix 判定）。
const ALLOWED_LOCK_PATH: &str = "flake.lock";
/// nightly が記録を追記してよい履歴ディレクトリ prefix。
const ALLOWED_HISTORY_PREFIX: &str = "docs/update-history/";

/// lock の全 node について期待する取得先の表（lock node の input 名 → 期待 owner/repo）。
///
/// 本表は「bump してよい input を選ぶ表」ではなく、各 node が「どこから取得されるべきか」だけを固定する
/// 実在 input の写しである。owner/repo は **厳密一致**で照合し、本表に期待値を持たない node の `locked` が
/// 動けば取得先同一性を検証できない変更として fail する（fail-closed）。
///
/// **保守義務**: `flake.nix` へ input を追加・削除・rename したら、同じ差分で本表も更新すること。更新漏れは
/// `cargo xtask check static` の `nightly_lock_input_sources_match_expected_table` が実 `flake.lock` と突合
/// して止める。上流 flake の input graph が変わって本表と乖離した場合の復旧手順は
/// `docs/automation/nightly-lock-bump.md` を正本とする。
const EXPECTED_LOCK_INPUT_SOURCES: [(&str, &str, &str); 10] = [
    ("nixpkgs", "NixOS", "nixpkgs"),
    ("darwin", "LnL7", "nix-darwin"),
    ("home-manager", "nix-community", "home-manager"),
    ("rust-overlay", "oxalica", "rust-overlay"),
    ("nix-homebrew", "zhaofengli-wip", "nix-homebrew"),
    ("brew-src", "Homebrew", "brew"),
    ("homebrew-homebrew-core", "homebrew", "homebrew-core"),
    ("homebrew-homebrew-cask", "homebrew", "homebrew-cask"),
    ("homebrew-azure-bicep", "Azure", "homebrew-bicep"),
    ("homebrew-hashicorp-tap", "hashicorp", "homebrew-tap"),
];

/// 1 つの lock node の同一性を決める source 座標（rev を除く）。
///
/// rev だけが変わったことを確かめるため、rev 以外（owner / repo / type / ref / url / host / dir / flake
/// フラグ）をまとめて比較する。期待取得先を持つ root input はこの座標が完全一致した上で rev だけ異なることを
/// 要求し、期待取得先を持たない node は rev を含む全フィールドが一致することを要求する。
///
/// `host` と `dir` を座標へ含めるのは、owner/repo/type/ref/rev がすべて期待値どおりでも fetch 先が動きうる
/// ためである。github flake ref の `host` は取得先ホスト（既定 `github.com`）を GitHub Enterprise 等へ
/// 上書きでき、`dir` は subflake のサブディレクトリを変える。
#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceCoords {
    owner: Option<String>,
    repo: Option<String>,
    node_type: Option<String>,
    reference: Option<String>,
    url: Option<String>,
    /// 取得先ホスト（github ref の `host`、既定 github.com）。GitHub Enterprise 等への drift を検出するため。
    host: Option<String>,
    /// subflake のサブディレクトリ指定（`dir`）。fetch 対象パスを変えうるため座標へ含める。
    dir: Option<String>,
    flake: Option<bool>,
}

impl SourceCoords {
    /// `ref`（同一 repo 内の tag / branch 指定）を落とした座標を返す。
    ///
    /// 推移 input の `ref` は親 flake の宣言に従って動くため、比較対象から外す。本メソッドは親 node の rev が
    /// 実際に動いたかを参照せず、`ref` の前後関係（前進 / 後退）も判定しない。落とすのは `ref` だけで、
    /// 取得先 repo そのものを決める owner / repo / type / url / host / dir は残す。caller responsibility:
    /// 本メソッドで比較する場合でも owner/repo の期待値厳密一致を別途要求すること。
    fn ignoring_reference(&self) -> Self {
        Self {
            reference: None,
            ..self.clone()
        }
    }
}

/// 与えられた変更パス集合と base/head の `flake.lock` 内容から、無人 auto-merge 可否を判定する。
///
/// `changed_paths` は PR の base..head union 変更パス（repo ルート相対、`/` 区切り）。`old_lock` / `new_lock`
/// は base / head の `flake.lock` の生 JSON。許可外パス・許可外 lock 差分があれば、最初に見つけた違反理由を
/// 載せた `Err` を返す。すべて許可範囲内なら `Ok(())`。CLI はこの結果で終了コードを決める。
///
/// caller responsibility: `changed_paths` は base..head の **全 commit** の union（途中 commit を含む）で
/// あること。net diff（両端 tree 比較）や head 単独の差分を渡すと、途中 commit 混入（add-then-remove）の
/// 検出という不変条件が崩れる。
pub(crate) fn verify_bump(
    changed_paths: &BTreeSet<String>,
    old_lock: &str,
    new_lock: &str,
) -> Result<()> {
    verify_changed_paths(changed_paths)?;
    verify_lock_diff(old_lock, new_lock)?;
    Ok(())
}

/// 変更パス集合が `flake.lock` と `docs/update-history/**` だけかを検査する。
fn verify_changed_paths(changed_paths: &BTreeSet<String>) -> Result<()> {
    for path in changed_paths {
        if path == ALLOWED_LOCK_PATH || path.starts_with(ALLOWED_HISTORY_PREFIX) {
            continue;
        }
        bail!(
            "disallowed path in nightly bump PR: {path} \
             (allowed: {ALLOWED_LOCK_PATH}, {ALLOWED_HISTORY_PREFIX}**)"
        );
    }
    Ok(())
}

/// `flake.lock` の base→head 差分が「期待取得先が一致する input の rev 変更だけ」かを検査する。
fn verify_lock_diff(old_lock: &str, new_lock: &str) -> Result<()> {
    let old: Value = serde_json::from_str(old_lock).context("base flake.lock is not valid JSON")?;
    let new: Value = serde_json::from_str(new_lock).context("head flake.lock is not valid JSON")?;

    // version と root（input ワイヤリング）は不変でなければならない。これらが変われば input の追加・削除や
    // lock フォーマット変更を意味し、rev bump の範囲を超える。
    if old.get("version") != new.get("version") {
        bail!("flake.lock version changed; nightly bump must not change lock format");
    }
    if old.get("root") != new.get("root") {
        bail!("flake.lock root inputs wiring changed; nightly bump must not add/remove inputs");
    }

    let old_nodes = nodes(&old, "base")?;
    let new_nodes = nodes(&new, "head")?;

    // node 集合（input 名）は完全一致でなければならない。追加・削除はいずれも未許可。
    let old_names: BTreeSet<&str> = old_nodes.keys().map(|k| k.as_str()).collect();
    let new_names: BTreeSet<&str> = new_nodes.keys().map(|k| k.as_str()).collect();
    if old_names != new_names {
        let added: Vec<&&str> = new_names.difference(&old_names).collect();
        let removed: Vec<&&str> = old_names.difference(&new_names).collect();
        bail!(
            "flake.lock node set changed (added: {added:?}, removed: {removed:?}); \
             nightly bump must not add or remove inputs"
        );
    }

    // root input（`flake.nix` が直接宣言する input）と推移 input を区別する。root input の `original` は
    // 本 repo の `flake.nix` 由来で、nightly PR は `flake.nix` を変更できない（許可パス外）ため不変を要求
    // できる。推移 input の `original` は親 flake 由来なので、親 bump で `ref` が動きうる。
    let root_inputs = root_input_node_names(&new, "head")?;

    let mut bumped_rev_count = 0usize;
    for (name, old_node) in old_nodes {
        // node 名集合の一致は直前に検査済みのため `get` は必ず `Some`。安全側として欠落時も fail にする。
        let Some(new_node) = new_nodes.get(name) else {
            bail!("flake.lock node `{name}` missing on head despite equal node set");
        };
        if verify_node(
            name,
            old_node,
            new_node,
            root_inputs.contains(name.as_str()),
        )? {
            bumped_rev_count += 1;
        }
    }

    // lock が実際に bump されていること（期待取得先を持つ input の rev 変更が少なくとも 1 件ある）を要求する。逸脱 lock
    // 変更が無くても、rev 変更ゼロ（docs/update-history だけ変える等の実体のない nightly PR）は無人 auto-merge
    // させない。これにより「許可パス内・逸脱 lock 変更なし」だが lock 無更新の空 bump PR を fail させる。
    if bumped_rev_count == 0 {
        bail!(
            "nightly bump PR changes no input rev; flake.lock is not actually bumped \
             (expected at least one rev change in an input with a known expected source)"
        );
    }
    Ok(())
}

/// `flake.lock` の `nodes` オブジェクトを取り出す。
fn nodes<'a>(lock: &'a Value, label: &str) -> Result<&'a serde_json::Map<String, Value>> {
    lock.get("nodes")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("{label} flake.lock has no nodes object"))
}

/// root node が直接指す input node 名の集合（= 本 repo の `flake.nix` が宣言する input）を取り出す。
///
/// この集合に属する node は `original`（宣言座標）が `flake.nix` そのものであり、nightly PR は許可パス上
/// `flake.nix` を変更できないため base→head で不変でなければならない。集合外の node は親 flake が宣言する
/// 推移 input であり、親を bump すると `ref` が動きうる。この区別が `original` 変更の許可範囲を決める。
///
/// root node の input 値が node 名の文字列でない（`follows` 等）場合は fail-closed で `Err` にする。
/// 判定不能を「推移 input 扱い」に倒すと `original` 変更の許可範囲を誤って広げるため、許可側へ倒さない。
fn root_input_node_names<'a>(lock: &'a Value, label: &str) -> Result<BTreeSet<&'a str>> {
    let root_name = lock
        .get("root")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("{label} flake.lock has no root node name"))?;
    let root_node = nodes(lock, label)?
        .get(root_name)
        .ok_or_else(|| anyhow!("{label} flake.lock has no `{root_name}` node"))?;
    let inputs = root_node
        .get("inputs")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("{label} flake.lock root node has no inputs object"))?;
    inputs
        .iter()
        .map(|(input, target)| {
            target.as_str().ok_or_else(|| {
                anyhow!(
                    "{label} flake.lock root input `{input}` is not a node name string; \
                     cannot tell root inputs from transitive inputs"
                )
            })
        })
        .collect()
}

/// 単一 node の base→head 差分を許可規則に照らす。期待取得先を持つ input の rev が実際に変わったら `true` を返す。
///
/// [`EXPECTED_LOCK_INPUT_SOURCES`] に期待取得先を持つ node は source 座標（rev 以外）一致を要求し、rev は
/// 変わってよい。期待取得先を持たない node は rev を含め全フィールド一致を要求する。`locked` を持たない node
/// （`root`）は source 座標を持たないため、node 同士の厳密一致だけを要求する。戻り値の `bool` は「期待取得先を
/// 持つ input の rev が変わったか」で、呼び出し側が lock 実 bump（rev 変更 1 件以上）の有無を集計するのに使う。
/// 期待取得先を持たない node や rev 不変の input は `false` を返す。
///
/// `is_root_input` は当該 node が本 repo の `flake.nix` 直下の宣言かどうか。`true` なら `original` と
/// source 座標（`ref` を含む）の完全一致を要求する。`false`（推移 input）なら `ref` の差分を**方向を問わず
/// 無条件に**許容し、版を下げる方向の `ref` 変更もここは通る。`ref` 以外の owner / repo / type / url /
/// host / dir の差分は推移 input でも許容しない。caller responsibility: `is_root_input` は head 側 lock の
/// root node から導出すること（base 側の古い wiring で判定しない）。
fn verify_node(
    name: &str,
    old_node: &Value,
    new_node: &Value,
    is_root_input: bool,
) -> Result<bool> {
    let old_locked = old_node.get("locked");
    let new_locked = new_node.get("locked");

    // locked を持たない node（root 等）は version/root 検査側で扱う。source 座標が無いものは
    // node 同士を厳密一致で比較し、いかなる差分も未許可とする。
    let (Some(old_locked), Some(new_locked)) = (old_locked, new_locked) else {
        if old_node != new_node {
            bail!("flake.lock node `{name}` changed but has no locked source; not an allowed bump");
        }
        return Ok(false);
    };

    // inputs（node 間ワイヤリング）は rev bump では変わらない。
    if old_node.get("inputs") != new_node.get("inputs") {
        bail!("flake.lock node `{name}` input wiring changed; not an allowed bump");
    }
    // node 直下の `flake` フラグ（その input を flake として評価するか）は取得・評価の意味を変えるため、
    // rev bump では不変を要求する。`locked` 内の `flake` とは別フィールドなので個別に検査する。
    if old_node.get("flake") != new_node.get("flake") {
        bail!("flake.lock node `{name}` flake flag changed; not an allowed bump");
    }

    let Some((_, owner, repo)) = EXPECTED_LOCK_INPUT_SOURCES
        .iter()
        .find(|(n, _, _)| *n == name)
    else {
        // 期待取得先を持たない node: source identity を照合できないため、original も locked も rev を含めて
        // 不変でなければならない。
        if old_node.get("original") != new_node.get("original") {
            bail!(
                "flake.lock node `{name}` original source declaration changed; not an allowed bump"
            );
        }
        if old_locked != new_locked {
            bail!(
                "flake.lock node `{name}` has no expected source identity entry but its locked source changed; \
                 add its expected owner/repo to the lock input source table first; not an allowed bump"
            );
        }
        return Ok(false);
    };

    verify_original(name, old_node, new_node, is_root_input)?;

    // 期待取得先を持つ input: source 座標（rev 以外）一致 + owner/repo が期待値に厳密一致。rev は変わってよい。
    // 推移 input だけは親由来の `ref` 差分を通すため、`ref` を除いた座標で比較する。
    let old_coords = source_coords(old_locked);
    let new_coords = source_coords(new_locked);
    let coords_match = if is_root_input {
        old_coords == new_coords
    } else {
        old_coords.ignoring_reference() == new_coords.ignoring_reference()
    };
    if !coords_match {
        bail!(
            "flake.lock node `{name}` source coordinates changed (not just rev); not an allowed bump"
        );
    }
    match (new_coords.owner.as_deref(), new_coords.repo.as_deref()) {
        (Some(o), Some(r)) if o == *owner && r == *repo => {}
        other => bail!(
            "flake.lock node `{name}` owner/repo {other:?} does not match \
             expected source {owner}/{repo}; not an allowed bump"
        ),
    }
    // content swap 防御: 期待取得先が一致していても、rev が **変わらないまま** narHash / lastModified だけが
    // 動く（= 同一 rev の取得物すり替え）変更は許可しない。nightly bump の正当な変更は「rev が進み、
    // それに伴って narHash / lastModified も整合的に更新される」ものだけである。rev 不変で内容ハッシュや
    // 取得時刻だけが変われば、source 座標も rev も同じに見えるのに固定対象の内容が差し替わっており、
    // 許可された rev bump の意味を超える。よって rev 変化を伴わない narHash / lastModified の変更は fail。
    verify_locked_integrity(name, old_locked, new_locked)
}

/// 期待取得先を持つ input の `original`（input が宣言する原座標）差分を許可規則に照らす。
///
/// root input の `original` は本 repo の `flake.nix` の宣言そのものであり、nightly PR の許可パスに
/// `flake.nix` は含まれないため base→head で変わりえない。よって完全一致を要求する。
///
/// 推移 input（親 flake が宣言する input）は、親を bump すると親側の宣言が動くため `ref`（同一 repo 内の
/// tag / branch 指定）が動きうる。本関数はこの `ref` 差分を **方向を問わず無条件に** 許可する。親 node の
/// rev が実際に動いたかは参照せず、`ref` の前進 / 後退も判定しないため、後退も親 bump を伴わない `ref`
/// 書き換えも通る。owner / repo / type / url / host / dir / flake など取得先そのものを決めるフィールドの
/// 差分は許可しない。取得先 repo の差し替えは呼び出し側の owner/repo 厳密一致検査と合わせて二重に塞ぐ。
fn verify_original(
    name: &str,
    old_node: &Value,
    new_node: &Value,
    is_root_input: bool,
) -> Result<()> {
    let old_original = old_node.get("original");
    let new_original = new_node.get("original");
    if old_original == new_original {
        return Ok(());
    }
    if is_root_input {
        bail!(
            "flake.lock node `{name}` is a root input declared by flake.nix but its \
             original source declaration changed; not an allowed bump"
        );
    }
    let (Some(old_rest), Some(new_rest)) = (
        original_without_reference(old_original),
        original_without_reference(new_original),
    ) else {
        bail!(
            "flake.lock node `{name}` original source declaration is missing or not an object; \
             not an allowed bump"
        );
    };
    if old_rest != new_rest {
        bail!(
            "flake.lock node `{name}` original source declaration changed beyond `ref`; \
             not an allowed bump"
        );
    }
    Ok(())
}

/// `original` オブジェクトから `ref` を除いたフィールド集合を複製する。object でなければ `None`。
fn original_without_reference(original: Option<&Value>) -> Option<serde_json::Map<String, Value>> {
    let mut fields = original?.as_object()?.clone();
    fields.remove("ref");
    Some(fields)
}

/// 期待取得先が一致した input の `locked` 整合を検査する: rev 変化と narHash / lastModified 変化を連動させる。
/// rev が変わったら `true` を返す（呼び出し側が lock 実 bump の有無を集計する）。
///
/// `rev` が変わらないのに `narHash` または `lastModified` だけが変われば、同一 rev のまま固定対象の内容が
/// すり替わった content swap であり、rev bump の許可範囲を超えるため fail にする。
///
/// `locked.rev` は base / head 双方で文字列として存在することを必須とする。欠落や非文字列を許すと双方 `None`
/// で「rev 変化なし」と誤認し、rev を欠いた lock が guard を素通りする。rev が変わった場合も head 側に
/// lock identity（`narHash` 非空文字列 + `lastModified` 整数）が存在することを要求し、identity を削った
/// 壊れた lock を「実 bump」として通さない（fail-closed）。
fn verify_locked_integrity(name: &str, old_locked: &Value, new_locked: &Value) -> Result<bool> {
    let str_field =
        |locked: &Value, key: &str| locked.get(key).and_then(Value::as_str).map(str::to_string);
    let require_rev = |locked: &Value, label: &str| -> Result<String> {
        str_field(locked, "rev").ok_or_else(|| {
            anyhow!(
                "flake.lock node `{name}` bumpable input is missing a string `locked.rev` \
                 on {label}; rev-less or broken-rev lock is not an allowed bump"
            )
        })
    };
    let old_rev = require_rev(old_locked, "base")?;
    let new_rev = require_rev(new_locked, "head")?;
    let rev_changed = old_rev != new_rev;
    if rev_changed {
        // rev が動いた = 実 bump。ただし head 側に GitHub input の lock identity（narHash 文字列 +
        // lastModified 整数）が無い/型崩れしている lock は、rev だけ進めて identity を削った壊れた lock であり
        // auto-merge へ通さない（fail-closed）。base..head の base 側で既に壊れている可能性に依存しないよう、
        // 必ず head（適用される側）の identity を要求する。
        require_lock_identity(name, new_locked)?;
        return Ok(true);
    }
    // rev 不変。narHash / lastModified が動いていれば content swap として fail。
    if old_locked.get("narHash") != new_locked.get("narHash") {
        bail!(
            "flake.lock node `{name}` narHash changed while rev is unchanged \
             (content swap at the same rev); not an allowed bump"
        );
    }
    if old_locked.get("lastModified") != new_locked.get("lastModified") {
        bail!(
            "flake.lock node `{name}` lastModified changed while rev is unchanged \
             (content swap at the same rev); not an allowed bump"
        );
    }
    Ok(false)
}

/// rev bump 後の `locked` が GitHub input の lock identity を保っているかを fail-closed 検証する。
///
/// 正当な GitHub flake input の lock node は `narHash`（fixed-output 同一性）を非空文字列で、`lastModified`
/// （取得時刻）を整数で持つ。rev を進めながらこれらを削除・非文字列化・非整数化した lock は、固定対象の同一性
/// 証跡を欠いており「許可された rev bump」の意味を満たさない。欠落・型崩れ・空 narHash を明示的に fail にし、
/// identity を欠いた lock を static checks success にしない。
fn require_lock_identity(name: &str, locked: &Value) -> Result<()> {
    match locked.get("narHash").and_then(Value::as_str) {
        Some(hash) if !hash.is_empty() => {}
        _ => bail!(
            "flake.lock node `{name}` bumpable input is missing a non-empty string \
             `locked.narHash` on head; rev bump without lock identity is not an allowed bump"
        ),
    }
    if locked.get("lastModified").and_then(Value::as_i64).is_none() {
        bail!(
            "flake.lock node `{name}` bumpable input is missing an integer \
             `locked.lastModified` on head; rev bump without lock identity is not an allowed bump"
        );
    }
    Ok(())
}

/// `locked` オブジェクトから rev を除いた source 座標を取り出す。
fn source_coords(locked: &Value) -> SourceCoords {
    let str_field = |key: &str| locked.get(key).and_then(Value::as_str).map(str::to_string);
    SourceCoords {
        owner: str_field("owner"),
        repo: str_field("repo"),
        node_type: str_field("type"),
        reference: str_field("ref"),
        url: str_field("url"),
        host: str_field("host"),
        dir: str_field("dir"),
        flake: locked.get("flake").and_then(Value::as_bool),
    }
}

#[cfg(test)]
mod tests {
    //! 許可パス限定と「期待取得先が一致する input の rev だけが動いた lock」判定を固定する。許可外パス・
    //! input 追加削除・期待取得先を持たない node の rev 変更・source 座標改変・owner/repo すり替えが
    //! すべて fail すること、および framework input の rev bump と推移 input の親由来 `ref` 差分が
    //! 通ることを確認する。

    use super::*;

    fn paths(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// 最小構成の lock（root input の nixpkgs と darwin）。`{rev}` を差し替えて使う。
    ///
    /// `narHash` は rev に連動して整合的に動く前提なので rev から決定論的に導出し、rev だけ動かせば narHash も
    /// 揃うようにする。content swap 検査（rev 不変で narHash だけ変える）は専用 test で別途 lock を組む。
    /// bump 対象 input の rev bump 後の lock identity 検査（[`require_lock_identity`]）が要求する `lastModified`
    /// （整数）も持たせ、正当な GitHub input lock node の形を模す。
    fn lock_with(nixpkgs_rev: &str, darwin_rev: &str) -> String {
        format!(
            r#"{{
  "nodes": {{
    "darwin": {{
      "inputs": {{ "nixpkgs": ["nixpkgs"] }},
      "locked": {{ "owner": "LnL7", "repo": "nix-darwin", "rev": "{darwin_rev}", "narHash": "sha256-{darwin_rev}", "lastModified": 1700000000, "type": "github" }},
      "original": {{ "owner": "LnL7", "repo": "nix-darwin", "type": "github" }}
    }},
    "nixpkgs": {{
      "locked": {{ "owner": "NixOS", "repo": "nixpkgs", "rev": "{nixpkgs_rev}", "narHash": "sha256-{nixpkgs_rev}", "lastModified": 1700000000, "type": "github" }},
      "original": {{ "owner": "NixOS", "ref": "nixpkgs-unstable", "repo": "nixpkgs", "type": "github" }}
    }},
    "root": {{ "inputs": {{ "darwin": "darwin", "nixpkgs": "nixpkgs" }} }}
  }},
  "root": "root",
  "version": 7
}}"#
        )
    }

    /// nix-homebrew（root input）と brew-src（その推移 input）だけを持つ lock。
    ///
    /// 親 flake を bump したときに推移 input の `original.ref` と rev が動く実ケース（brew-src が
    /// `5.1.1` → `6.0.13` へ進む）を再現するために使う。
    fn lock_with_transitive(nix_homebrew_rev: &str, brew_rev: &str, brew_ref: &str) -> String {
        format!(
            r#"{{
  "nodes": {{
    "brew-src": {{
      "flake": false,
      "locked": {{ "owner": "Homebrew", "repo": "brew", "rev": "{brew_rev}", "narHash": "sha256-{brew_rev}", "lastModified": 1700000000, "type": "github" }},
      "original": {{ "owner": "Homebrew", "ref": "{brew_ref}", "repo": "brew", "type": "github" }}
    }},
    "nix-homebrew": {{
      "inputs": {{ "brew-src": "brew-src" }},
      "locked": {{ "owner": "zhaofengli-wip", "repo": "nix-homebrew", "rev": "{nix_homebrew_rev}", "narHash": "sha256-{nix_homebrew_rev}", "lastModified": 1700000000, "type": "github" }},
      "original": {{ "owner": "zhaofengli-wip", "repo": "nix-homebrew", "type": "github" }}
    }},
    "root": {{ "inputs": {{ "nix-homebrew": "nix-homebrew" }} }}
  }},
  "root": "root",
  "version": 7
}}"#
        )
    }

    /// 推移 input の `locked` にも `ref` を持つ lock。
    ///
    /// [`lock_with_transitive`] は `original` にだけ `ref` を持つため、[`SourceCoords::ignoring_reference`] が
    /// no-op になり、緩和を厳密比較へ戻しても等値比較が成立してしまう（緩和の有無をテストが区別できない）。
    /// tag 指定つき input のように `locked.ref` が実体を持つ node を模して、`ref` 緩和の許可側・拒否側の
    /// 境界をこの fixture で固定する。
    fn lock_with_transitive_locked_ref(
        nix_homebrew_rev: &str,
        brew_rev: &str,
        brew_ref: &str,
    ) -> String {
        format!(
            r#"{{
  "nodes": {{
    "brew-src": {{
      "flake": false,
      "locked": {{ "owner": "Homebrew", "repo": "brew", "ref": "{brew_ref}", "rev": "{brew_rev}", "narHash": "sha256-{brew_rev}", "lastModified": 1700000000, "type": "github" }},
      "original": {{ "owner": "Homebrew", "ref": "{brew_ref}", "repo": "brew", "type": "github" }}
    }},
    "nix-homebrew": {{
      "inputs": {{ "brew-src": "brew-src" }},
      "locked": {{ "owner": "zhaofengli-wip", "repo": "nix-homebrew", "rev": "{nix_homebrew_rev}", "narHash": "sha256-{nix_homebrew_rev}", "lastModified": 1700000000, "type": "github" }},
      "original": {{ "owner": "zhaofengli-wip", "repo": "nix-homebrew", "type": "github" }}
    }},
    "root": {{ "inputs": {{ "nix-homebrew": "nix-homebrew" }} }}
  }},
  "root": "root",
  "version": 7
}}"#
        )
    }

    /// 期待取得先表に無い node（`mystery`）を含む lock。期待取得先を持たない node の rev 変更が fail する
    /// ことの検査に使う。
    fn lock_with_unlisted_input(mystery_rev: &str) -> String {
        format!(
            r#"{{
  "nodes": {{
    "mystery": {{
      "locked": {{ "owner": "someone", "repo": "mystery", "rev": "{mystery_rev}", "narHash": "sha256-{mystery_rev}", "lastModified": 1700000000, "type": "github" }},
      "original": {{ "owner": "someone", "repo": "mystery", "type": "github" }}
    }},
    "nixpkgs": {{
      "locked": {{ "owner": "NixOS", "repo": "nixpkgs", "rev": "aaaa", "narHash": "sha256-aaaa", "lastModified": 1700000000, "type": "github" }},
      "original": {{ "owner": "NixOS", "ref": "nixpkgs-unstable", "repo": "nixpkgs", "type": "github" }}
    }},
    "root": {{ "inputs": {{ "mystery": "mystery", "nixpkgs": "nixpkgs" }} }}
  }},
  "root": "root",
  "version": 7
}}"#
        )
    }

    #[test]
    fn accepts_allowed_paths() -> Result<()> {
        verify_changed_paths(&paths(&[
            "flake.lock",
            "docs/update-history/2026-06.toml",
            "docs/update-history/2026-07.toml",
        ]))
    }

    #[test]
    fn rejects_workflow_path() {
        let err = verify_changed_paths(&paths(&[
            "flake.lock",
            ".github/workflows/nightly-update.yml",
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("disallowed path"), "{err}");
    }

    #[test]
    fn rejects_ruleset_and_source_paths() {
        assert!(verify_changed_paths(&paths(&[".github/rulesets/nightly.json"])).is_err());
        assert!(verify_changed_paths(&paths(&["rust/xtask/src/ci.rs"])).is_err());
        // docs/update-history directory 自体（末尾 / 無し）は prefix にマッチしないため未許可。
        assert!(verify_changed_paths(&paths(&["docs/update-history"])).is_err());
    }

    #[test]
    fn accepts_nixpkgs_rev_bump_only() -> Result<()> {
        let old = lock_with("aaaa", "dddd");
        let new = lock_with("bbbb", "dddd");
        verify_bump(&paths(&["flake.lock"]), &old, &new)
    }

    #[test]
    fn accepts_framework_rev_bump() -> Result<()> {
        // 方針変更の固定: nightly は `nix flake update` で全 input を bump するため、framework input
        // （ここでは darwin）単独の rev bump も通す。除外運用は、除外に対応する有人 bump 経路が無く
        // 「更新されない」と同義になり、据え置き input と前進 input の組み合わせで `dotfiles update` を
        // 停止させた。
        let old = lock_with("aaaa", "dddd");
        let new = lock_with("aaaa", "eeee");
        verify_bump(&paths(&["flake.lock"]), &old, &new)
    }

    #[test]
    fn rejects_rev_bump_of_node_without_expected_source() {
        // 表が実在 input の写しになった後も fail-closed であることの固定: 期待取得先を持たない node の
        // locked が動けば、取得先同一性を照合できない変更として fail する。
        let old = lock_with_unlisted_input("1111");
        let new = lock_with_unlisted_input("2222");
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(
            err.to_string()
                .contains("has no expected source identity entry"),
            "{err}"
        );
    }

    #[test]
    fn accepts_transitive_input_ref_bump_from_parent() -> Result<()> {
        // 親 flake（nix-homebrew）を bump すると、その推移 input（brew-src）の `original.ref` と rev が
        // 親側の宣言に従って動く。これは正当な bump なので通す（実例: brew 5.1.1 → 6.0.13）。
        let old = lock_with_transitive("1111", "aaaa", "5.1.1");
        let new = lock_with_transitive("2222", "bbbb", "6.0.13");
        verify_bump(&paths(&["flake.lock"]), &old, &new)
    }

    #[test]
    fn accepts_transitive_input_locked_ref_bump_from_parent() -> Result<()> {
        // `ref` 緩和の許可側境界: `locked.ref` が実体を持つ推移 input でも、親 bump に伴う `ref` の前進は
        // 通す。`SourceCoords::ignoring_reference` を厳密比較（`self.clone()`）へ戻すとこの test は
        // `source coordinates changed` で fail するため、緩和の有無をテストが区別できる。
        let old = lock_with_transitive_locked_ref("1111", "aaaa", "5.1.1");
        let new = lock_with_transitive_locked_ref("2222", "bbbb", "6.0.13");
        assert!(old.contains(r#""ref": "5.1.1", "rev": "aaaa""#), "{old}");
        verify_bump(&paths(&["flake.lock"]), &old, &new)
    }

    #[test]
    fn rejects_transitive_input_locked_host_drift_despite_ref_relaxation() {
        // `ref` 緩和の拒否側境界: 緩和で落とすのは `ref` だけであり、同じ推移 input でも `host` のように
        // 取得先を決める座標が動けば fail する。`ignoring_reference` が `ref` 以外まで落とす実装へ広がれば
        // この test が緑のまま通ってしまうため、緩和の範囲をここで固定する。
        let old = lock_with_transitive_locked_ref("1111", "aaaa", "5.1.1");
        let new = lock_with_transitive_locked_ref("2222", "bbbb", "6.0.13").replace(
            r#""owner": "Homebrew", "repo": "brew", "ref": "6.0.13", "rev": "bbbb""#,
            r#""owner": "Homebrew", "repo": "brew", "ref": "6.0.13", "host": "github.example.com", "rev": "bbbb""#,
        );
        assert!(
            new.contains("github.example.com"),
            "host 注入が効いていること"
        );
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(
            err.to_string().contains("source coordinates changed"),
            "{err}"
        );
    }

    #[test]
    fn rejects_transitive_input_owner_change_in_original() {
        // 推移 input で許可するのは `ref` の差分だけ。`original` の取得先 repo（owner）まで差し替われば
        // `verify_original` が fail させる。どのガードが働いたかを一意に固定する（OR 判定にしない）。
        let old = lock_with_transitive("1111", "aaaa", "5.1.1");
        let new = lock_with_transitive("2222", "bbbb", "6.0.13")
            .replace(r#""owner": "Homebrew""#, r#""owner": "evil""#);
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(
            err.to_string()
                .contains("original source declaration changed beyond `ref`"),
            "{err}"
        );
    }

    #[test]
    fn rejects_transitive_input_locked_owner_swap_with_intact_original() {
        // `original` は正当なまま `locked.owner` だけをすり替える経路。`verify_original` は通過するため、
        // 推移 input の `locked` 側 source 座標比較（`ignoring_reference` 経由）が唯一のガードになる。
        // この経路を通すテストが無いと、`locked` 側比較を落としても全 test が緑のままになる。
        let old = lock_with_transitive("1111", "aaaa", "5.1.1");
        let new = lock_with_transitive("2222", "bbbb", "6.0.13").replace(
            r#""owner": "Homebrew", "repo": "brew", "rev": "bbbb""#,
            r#""owner": "evil", "repo": "brew", "rev": "bbbb""#,
        );
        assert!(
            new.contains(r#""owner": "Homebrew", "ref": "6.0.13", "repo": "brew""#),
            "original は正当なまま（改変は locked 側だけ）"
        );
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(
            err.to_string().contains("source coordinates changed"),
            "{err}"
        );
    }

    #[test]
    fn root_input_names_reject_missing_root_key() -> Result<()> {
        // fail-closed 分岐 1/4: `root` キーが無ければ root input と推移 input を区別できない。
        let lock: Value =
            serde_json::from_str(r#"{ "nodes": { "root": { "inputs": {} } }, "version": 7 }"#)?;
        let err = root_input_node_names(&lock, "head").unwrap_err();
        assert!(err.to_string().contains("has no root node name"), "{err}");
        Ok(())
    }

    #[test]
    fn root_input_names_reject_missing_root_node() -> Result<()> {
        // fail-closed 分岐 2/4: `root` が指す node が nodes に無い。
        let lock: Value = serde_json::from_str(
            r#"{ "nodes": { "nixpkgs": {} }, "root": "root", "version": 7 }"#,
        )?;
        let err = root_input_node_names(&lock, "head").unwrap_err();
        assert!(err.to_string().contains("has no `root` node"), "{err}");
        Ok(())
    }

    #[test]
    fn root_input_names_reject_root_node_without_inputs() -> Result<()> {
        // fail-closed 分岐 3/4: root node に `inputs` object が無い。
        let lock: Value =
            serde_json::from_str(r#"{ "nodes": { "root": {} }, "root": "root", "version": 7 }"#)?;
        let err = root_input_node_names(&lock, "head").unwrap_err();
        assert!(
            err.to_string().contains("root node has no inputs object"),
            "{err}"
        );
        Ok(())
    }

    #[test]
    fn root_input_names_reject_non_string_root_input_value() -> Result<()> {
        // fail-closed 分岐 4/4: トップレベル `inputs.X.follows` は `flake.nix` 上正当な記法で、その lock では
        // root input 値が node 名文字列ではなく follows path 配列になる。判定不能を「推移 input 扱い」へ倒すと
        // `original` 変更の許可範囲を誤って広げるため、許可側へ倒さず `Err` にする。
        let lock: Value = serde_json::from_str(
            r#"{ "nodes": { "root": { "inputs": { "nixpkgs": ["darwin", "nixpkgs"] } } }, "root": "root", "version": 7 }"#,
        )?;
        let err = root_input_node_names(&lock, "head").unwrap_err();
        assert!(
            err.to_string().contains("is not a node name string"),
            "{err}"
        );
        Ok(())
    }

    #[test]
    fn rejects_lock_whose_root_input_uses_top_level_follows() {
        // 分岐 4/4 の実経路固定: そうした lock を渡した nightly PR は毎回 hard fail する（無人 merge しない）。
        let follows_root =
            r#""root": { "inputs": { "darwin": "darwin", "nixpkgs": ["darwin", "nixpkgs"] } }"#;
        let plain_root = r#""root": { "inputs": { "darwin": "darwin", "nixpkgs": "nixpkgs" } }"#;
        let old = lock_with("aaaa", "dddd").replace(plain_root, follows_root);
        let new = lock_with("bbbb", "dddd").replace(plain_root, follows_root);
        assert!(new.contains(follows_root), "follows 置換が効いていること");
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(
            err.to_string().contains("is not a node name string"),
            "{err}"
        );
    }

    #[test]
    fn verify_original_rejects_missing_original_on_transitive_input() -> Result<()> {
        // 推移 input の `original` が base 側で欠落している場合、`ref` 差分だけかを判定できないため fail。
        let old: Value = serde_json::from_str(r#"{ "locked": {} }"#)?;
        let new: Value = serde_json::from_str(
            r#"{ "original": { "owner": "Homebrew", "repo": "brew", "type": "github" } }"#,
        )?;
        let err = verify_original("brew-src", &old, &new, false).unwrap_err();
        assert!(
            err.to_string().contains("missing or not an object"),
            "{err}"
        );
        Ok(())
    }

    #[test]
    fn verify_original_rejects_non_object_original_on_transitive_input() -> Result<()> {
        // `original` が object でない（flake ref 文字列等）場合も `ref` 以外の差分を判定できないため fail。
        let old: Value = serde_json::from_str(
            r#"{ "original": { "owner": "Homebrew", "repo": "brew", "type": "github" } }"#,
        )?;
        let new: Value = serde_json::from_str(r#"{ "original": "github:Homebrew/brew" }"#)?;
        let err = verify_original("brew-src", &old, &new, false).unwrap_err();
        assert!(
            err.to_string().contains("missing or not an object"),
            "{err}"
        );
        Ok(())
    }

    #[test]
    fn rejects_root_input_original_ref_change() {
        // root input（`flake.nix` の宣言）の `original.ref` は nightly PR では変わりえない。推移 input 向けの
        // `ref` 緩和が root input へ波及していないことを固定する。
        let old = lock_with_transitive("1111", "aaaa", "5.1.1");
        let new = lock_with_transitive("2222", "bbbb", "6.0.13").replace(
            r#""original": { "owner": "zhaofengli-wip", "repo": "nix-homebrew", "type": "github" }"#,
            r#""original": { "owner": "zhaofengli-wip", "ref": "evil", "repo": "nix-homebrew", "type": "github" }"#,
        );
        assert_ne!(
            new,
            lock_with_transitive("2222", "bbbb", "6.0.13"),
            "root input の original 改変が lock 本文を変えていること"
        );
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(
            err.to_string().contains("root input declared by flake.nix"),
            "{err}"
        );
    }

    #[test]
    fn rejects_node_flake_flag_change() {
        // node 直下の `flake` フラグは「その input を flake として評価するか」を決め、取得・評価の意味を
        // 変える。rev bump の範囲外なので fail にする。
        let old = lock_with_transitive("1111", "aaaa", "5.1.1");
        let new = lock_with_transitive("2222", "bbbb", "6.0.13")
            .replace(r#""flake": false"#, r#""flake": true"#);
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(err.to_string().contains("flake flag changed"), "{err}");
    }

    #[test]
    fn rejects_unexpected_input_addition() {
        let old = lock_with("aaaa", "dddd");
        // head に未宣言 input を 1 つ足す。
        let new = old.replace(
            r#""root": {"#,
            r#""evil": { "locked": { "owner": "x", "repo": "y", "rev": "1", "type": "github" }, "original": {} },
    "root": {"#,
        );
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(err.to_string().contains("node set changed"), "{err}");
    }

    #[test]
    fn rejects_source_url_change_on_allowed_input() {
        let old = lock_with("aaaa", "dddd");
        // nixpkgs の owner を別 fork へ差し替える（rev も動かす）。
        let new = lock_with("bbbb", "dddd").replace(
            r#""owner": "NixOS", "repo": "nixpkgs", "rev": "bbbb""#,
            r#""owner": "evil", "repo": "nixpkgs", "rev": "bbbb""#,
        );
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(
            err.to_string().contains("source coordinates changed")
                || err.to_string().contains("owner/repo"),
            "{err}"
        );
    }

    #[test]
    fn rejects_original_declaration_change() {
        let old = lock_with("aaaa", "dddd");
        // nixpkgs の original ref を差し替える。
        let new = lock_with("bbbb", "dddd")
            .replace(r#""ref": "nixpkgs-unstable""#, r#""ref": "nixos-25.05""#);
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(
            err.to_string()
                .contains("original source declaration changed"),
            "{err}"
        );
    }

    #[test]
    fn rejects_narhash_swap_with_unchanged_rev() {
        // N4 退行固定: bump 対象 input でも rev 不変のまま narHash だけ差し替える content swap は fail。
        // owner/repo/type/ref/url/rev はすべて同一に見えるが、固定 rev の取得物がすり替わっている。
        let old = lock_with("aaaa", "dddd");
        let new = lock_with("aaaa", "dddd").replace(
            r#""rev": "aaaa", "narHash": "sha256-aaaa""#,
            r#""rev": "aaaa", "narHash": "sha256-EVILSWAP""#,
        );
        // 置換が効いて old != new であることを前提に検査する。
        assert_ne!(old, new, "narHash 置換が lock 本文を変えていること");
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(
            err.to_string()
                .contains("narHash changed while rev is unchanged"),
            "{err}"
        );
    }

    #[test]
    fn rejects_last_modified_swap_with_unchanged_rev() {
        // N4 退行固定（lastModified 版）: rev 不変で lastModified だけ動く（同一 rev の取得時刻すり替え）も fail。
        // nixpkgs（bump 対象 input）の lastModified だけを base/head で別値に差し替える（darwin 側は据え置き）。
        let old = lock_with("aaaa", "dddd").replace(
            r#""rev": "aaaa", "narHash": "sha256-aaaa", "lastModified": 1700000000"#,
            r#""rev": "aaaa", "narHash": "sha256-aaaa", "lastModified": 100"#,
        );
        let new = lock_with("aaaa", "dddd").replace(
            r#""rev": "aaaa", "narHash": "sha256-aaaa", "lastModified": 1700000000"#,
            r#""rev": "aaaa", "narHash": "sha256-aaaa", "lastModified": 999"#,
        );
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(
            err.to_string()
                .contains("lastModified changed while rev is unchanged"),
            "{err}"
        );
    }

    #[test]
    fn accepts_narhash_change_when_rev_also_bumps() -> Result<()> {
        // 正当な bump: rev が進めば narHash も連動して動く。これは許可（content swap ではない）。
        // lock_with は narHash を rev から導出するため、rev を変えれば narHash も変わる。
        let old = lock_with("aaaa", "dddd");
        let new = lock_with("bbbb", "dddd");
        // narHash が実際に変わっていること（連動更新の確認）。
        assert!(new.contains("sha256-bbbb"), "narHash は rev に連動する");
        verify_bump(&paths(&["flake.lock"]), &old, &new)
    }

    #[test]
    fn rejects_allowed_input_with_missing_rev() {
        // E 退行固定: bump 対象 input（nixpkgs）の `locked.rev` が削除された lock は guard が見逃さず fail にする。
        // base/head とも rev を持たないと旧実装は `old_rev == new_rev`（ともに None）で「変化なし」扱いとなり、
        // narHash/lastModified も一致すれば素通りした。rev 欠落の壊れた lock を無人 merge へ通さない。
        let old =
            lock_with("aaaa", "dddd").replace(r#""rev": "aaaa", "narHash": "sha256-aaaa", "#, "");
        let new = old.clone();
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(
            err.to_string().contains("missing a string `locked.rev`"),
            "{err}"
        );
    }

    #[test]
    fn rejects_allowed_input_with_non_string_rev() {
        // E 退行固定: bump 対象 input の `locked.rev` が文字列以外（ここでは数値）へ壊された lock も fail にする。
        // `as_str()` が None を返すため rev 欠落と同様に「変化なし」誤認の抜けになる。非文字列 rev も明示 fail。
        let old = lock_with("aaaa", "dddd").replace(r#""rev": "aaaa""#, r#""rev": 12345"#);
        let new = old.clone();
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(
            err.to_string().contains("missing a string `locked.rev`"),
            "{err}"
        );
    }

    #[test]
    fn rejects_host_drift_on_allowed_input() {
        // host 退行固定: bump 対象 input（nixpkgs）の owner/repo/type/ref/rev をすべて期待値どおりに保ったまま、
        // `locked.host` だけを GitHub Enterprise 等へ差し替えると取得先 host が github.com から逸脱する。
        // host を source 座標に含めることで、rev を動かしても host drift を「source coordinates changed」で fail
        // にする（owner/repo 厳密一致の信頼境界を host まで拡張）。
        let old = lock_with("aaaa", "dddd");
        // nixpkgs に host を足し（base には無い→head で github.example.com を注入）、rev も動かす。
        let new = lock_with("bbbb", "dddd").replace(
            r#""owner": "NixOS", "repo": "nixpkgs", "rev": "bbbb""#,
            r#""owner": "NixOS", "repo": "nixpkgs", "host": "github.example.com", "rev": "bbbb""#,
        );
        assert_ne!(old, new, "host 注入が lock 本文を変えていること");
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(
            err.to_string().contains("source coordinates changed"),
            "{err}"
        );
    }

    #[test]
    fn rejects_rev_bump_with_missing_narhash() {
        // identity 退行固定: bump 対象 input の rev は動くが head 側で `locked.narHash` が削除された lock は、
        // rev 変化だけで「実 bump」として通してはならない。lock identity（fixed-output 同一性）を欠いた壊れた
        // lock を fail-closed で fail にする。
        let old = lock_with("aaaa", "dddd");
        // head: nixpkgs の rev を bbbb へ進めつつ narHash を削除する。
        let new = lock_with("bbbb", "dddd").replace(
            r#""rev": "bbbb", "narHash": "sha256-bbbb", "#,
            r#""rev": "bbbb", "#,
        );
        assert_ne!(old, new);
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(
            err.to_string().contains("missing a non-empty string"),
            "{err}"
        );
    }

    #[test]
    fn rejects_rev_bump_with_non_string_narhash() {
        // identity 退行固定: rev は動くが head の narHash が文字列以外（数値）へ壊れた lock も fail。
        let old = lock_with("aaaa", "dddd");
        let new =
            lock_with("bbbb", "dddd").replace(r#""narHash": "sha256-bbbb""#, r#""narHash": 12345"#);
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(
            err.to_string().contains("missing a non-empty string"),
            "{err}"
        );
    }

    #[test]
    fn rejects_rev_bump_with_missing_last_modified() {
        // identity 退行固定: rev は動くが head 側で `locked.lastModified` が削除（または非整数化）された lock も
        // fail。rev だけ進めて取得時刻 identity を削った lock を auto-merge へ通さない。
        let old = lock_with("aaaa", "dddd");
        let new = lock_with("bbbb", "dddd").replace(
            r#""narHash": "sha256-bbbb", "lastModified": 1700000000, "#,
            r#""narHash": "sha256-bbbb", "#,
        );
        assert_ne!(old, new);
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(err.to_string().contains("missing an integer"), "{err}");
    }

    #[test]
    fn rejects_version_change() {
        let old = lock_with("aaaa", "dddd");
        let new = lock_with("bbbb", "dddd").replace(r#""version": 7"#, r#""version": 8"#);
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(err.to_string().contains("version changed"), "{err}");
    }

    #[test]
    fn rejects_node_inputs_wiring_change() {
        // bump 対象 input（nixpkgs）の rev は動いてよいが、node 間 inputs ワイヤリング改変は rev bump 範囲外。
        // darwin の inputs.nixpkgs follows を別 node 名へ差し替える（rev は据え置き）。
        let old = lock_with("aaaa", "dddd");
        let new = lock_with("bbbb", "dddd").replace(
            r#""inputs": { "nixpkgs": ["nixpkgs"] }"#,
            r#""inputs": { "nixpkgs": ["evil"] }"#,
        );
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(err.to_string().contains("input wiring changed"), "{err}");
    }

    #[test]
    fn rejects_node_deletion() {
        // head で bump 対象 input（nixpkgs）node を削除する。node 集合不一致として fail（追加と同様に削除も未許可）。
        let old = lock_with("aaaa", "dddd");
        // nixpkgs node ブロックと root の参照を削り、node 集合を変える。
        let new = r#"{
  "nodes": {
    "darwin": {
      "inputs": { "nixpkgs": ["nixpkgs"] },
      "locked": { "owner": "LnL7", "repo": "nix-darwin", "rev": "dddd", "type": "github" },
      "original": { "owner": "LnL7", "repo": "nix-darwin", "type": "github" }
    },
    "root": { "inputs": { "darwin": "darwin" } }
  },
  "root": "root",
  "version": 7
}"#
        .to_string();
        let err = verify_bump(&paths(&["flake.lock"]), &old, &new).unwrap_err();
        assert!(err.to_string().contains("node set changed"), "{err}");
    }

    #[test]
    fn rejects_nightly_pr_with_no_input_rev_change() {
        // N6 退行固定: 許可パス内・逸脱 lock 変更なしでも、input の rev が 1 件も動いていない（lock 実
        // bump 無し）nightly PR は fail させる。ここでは lock が完全に無変更（docs/update-history だけ変える PR を
        // 模す）。
        let lock = lock_with("aaaa", "dddd");
        let err = verify_bump(
            &paths(&["flake.lock", "docs/update-history/2026-06.toml"]),
            &lock,
            &lock,
        )
        .unwrap_err();
        assert!(err.to_string().contains("changes no input rev"), "{err}");
    }

    #[test]
    fn accepts_when_at_least_one_input_rev_changes() -> Result<()> {
        // N6: 期待取得先を持つ input（nixpkgs）の rev が 1 件でも動いていれば lock 実 bump として pass する。
        let old = lock_with("aaaa", "dddd");
        let new = lock_with("bbbb", "dddd");
        verify_bump(&paths(&["flake.lock"]), &old, &new)
    }

    #[test]
    fn rejects_no_change_is_fine_but_disallowed_path_fails_even_with_clean_lock() {
        // lock が無変更でも、union に逸脱パスが 1 つあれば fail（途中 commit 混入の検出）。
        let lock = lock_with("aaaa", "dddd");
        let err = verify_bump(
            &paths(&["flake.lock", ".github/workflows/x.yml"]),
            &lock,
            &lock,
        )
        .unwrap_err();
        assert!(err.to_string().contains("disallowed path"), "{err}");
    }
}
