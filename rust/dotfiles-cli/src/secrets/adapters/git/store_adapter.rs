//! `PasswordStorePort` を `~/.password-store` の filesystem 観測へ接続する adapter。
//!
//! `$HOME` を解決して `~/.password-store` の存在確認と、clone 後 store root の識別ファイル
//! （`.gpg-id`）の有無・recipient 行・復号確認用サンプル `*.gpg` entry を観測し、domain 値
//! （`PasswordStoreReadiness`）へ翻訳する。`.gpg-id` は regular file のときだけ有効な識別ファイルとして
//! 扱い、symlink・directory・special file は `.gpg-id` として認めない（link を辿らない）。symlink の
//! `.gpg-id` を辿ると store 外の path（例: `/dev/zero` で hang/OOM、外部の復号可能ファイルで偽の可読性成功）
//! へ抜けるため、symlinked `.gpg-id` は「有効な `.gpg-id` 不在」として可読性確認を失敗させる。
//! recipient 形式妥当性・復号可否・store 既存時の停止可否といった業務規則は domain
//! （`PasswordStoreReadiness::parse_recipients` ほか）と keyring 照合へ残す。ここでは filesystem 走査だけを
//! 担い、`.gpg-id` の中身解釈や `pass` CLI への無条件シェルアウトはしない。

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::{
    Result,
    secrets::{
        adapters::git::password_store_path,
        domain::pass_restore::{PASSWORD_STORE_GPG_ID, PasswordStoreReadiness},
    },
};

/// `~/.password-store` の filesystem 観測を `PasswordStorePort` 契約へ翻訳する adapter。
#[derive(Default)]
pub(super) struct PasswordStoreAdapter;

impl PasswordStoreAdapter {
    /// `~/.password-store` が既に存在するか（path として存在するか）を確認する。
    ///
    /// `Path::exists` は symlink を辿るため壊れた（dangling）symlink を「不在」と誤判定する。ここでは
    /// `symlink_metadata`（link を辿らない）で判定し、壊れた可能性のある symlink として存在する path も
    /// 「存在」とみなす。これにより、dangling symlink になった `~/.password-store` を既存 store ガードが
    /// 見落として上書き clone しないようにする。
    pub(super) fn password_store_exists(&self) -> Result<bool> {
        Ok(path_exists_including_broken_symlink(&password_store_path()?))
    }

    /// clone 先 store root を走査し、`.gpg-id` の有無・recipient 行・サンプル entry を
    /// `PasswordStoreReadiness` へ翻訳する。
    ///
    /// `.gpg-id` は regular file のときだけ「存在」と判定する。`symlink_metadata`（link を辿らない）で
    /// metadata を取り、`file_type().is_file()` が真の場合だけ有効な `.gpg-id` として扱い、symlink・
    /// directory・special file は `.gpg-id` 不在とみなす（link を辿らない）。これにより symlinked `.gpg-id`
    /// は可読性確認を「有効な `.gpg-id` 不在」で失敗させ、store 外の path へ抜けない。`.gpg-id` の各行は
    /// 空行・`#` コメントを除いて未 trim のまま recipient 候補として渡し、形式妥当性の判定は domain へ委ねる。
    /// サンプル entry は store 内の最初に見つかった regular file の `*.gpg` を 1 件だけ返し、復号確認
    /// （keyring 照合）の対象にする。entry が 1 件も無い store でも `.gpg-id` 妥当性までは検証できる。
    pub(super) fn inspect_password_store(&self) -> Result<PasswordStoreReadiness> {
        let store_root = password_store_path()?;
        let gpg_id_path = store_root.join(PASSWORD_STORE_GPG_ID);
        let gpg_id_present = is_regular_file(&gpg_id_path);
        let gpg_id_recipients = if gpg_id_present {
            read_gpg_id_recipients(&gpg_id_path)?
        } else {
            Vec::new()
        };
        let sample_entry = find_sample_entry(&store_root);
        Ok(PasswordStoreReadiness {
            gpg_id_present,
            gpg_id_recipients,
            sample_entry,
        })
    }
}

/// `.gpg-id` の読み取り上限（byte）。`.gpg-id` は recipient fingerprint/user-id を数行持つだけの小さな
/// テキストであり、これを超える入力は異常（巨大ファイルや、走査後に差し替わった endless file）として
/// 拒否する。dev/ino 照合の安全読取りと併せ、`/dev/zero` のような無限長 source の読み込みで
/// hang/OOM する経路を断つ。
const GPG_ID_MAX_BYTES: u64 = 64 * 1024;

/// path が regular file か（symlink を辿らずに）を判定する。`symlink_metadata` は link を辿らないため、
/// symlink・directory・special file はいずれも `file_type().is_file()` が偽になり、regular file だけを真とする。
fn is_regular_file(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_file())
}

/// path が存在するか（壊れた symlink も含めて）を判定する。`symlink_metadata` は link を辿らないため、
/// dangling symlink でも metadata 取得に成功し、その path を「存在」とみなせる。
fn path_exists_including_broken_symlink(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

/// `.gpg-id` の各行を読み、空行と `#` コメントを除いた行（未 trim）を recipient 候補として返す。
///
/// 走査時点で `symlink_metadata`（link を辿らない）を取り、final component が regular file であること・
/// size が [`GPG_ID_MAX_BYTES`] 以内であることを確認してから `File::open` し、開いた fd の fstat
/// （`metadata()`）の `dev`/`ino` が走査時点と一致することを照合する。走査↔open の間に final component が
/// 別 inode（store 外を指す symlink や special file）へ差し替えられていれば、`File::open` がその symlink を辿り
/// fd の dev/ino が走査時点と一致しない（または special file で `is_file` が偽になる）ため検出して停止する。
/// これは `O_NOFOLLOW`（final component のみ保護）と同等の保護範囲を `std::os::unix::fs::MetadataExt` だけで
/// 達成し、libc/OS 数値ハードコードへ依存しない。
///
/// 照合済みの fd からだけ読み、path を再 open しない。さらに `metadata().len()` が [`GPG_ID_MAX_BYTES`] を
/// 超える場合は読まずに拒否し、読取り自体も上限 + 1 byte で打ち切ることで、`len` を 0 と詐称する special file
/// （`/dev/zero` 等）経由の endless read（hang/OOM）と巨大ファイルの読み込みを断つ。
fn read_gpg_id_recipients(gpg_id_path: &Path) -> Result<Vec<String>> {
    use std::io::Read;

    // 走査時点の metadata は link を辿らずに取得する。final component が regular file 以外（symlink/dir/
    // special file）なら、その時点で停止する。
    let pre = gpg_id_path
        .symlink_metadata()
        .context("failed to stat password-store .gpg-id (refusing to follow a symlink)")?;
    if !pre.file_type().is_file() {
        anyhow::bail!("password-store .gpg-id is not a regular file; refusing to follow it");
    }
    if pre.len() > GPG_ID_MAX_BYTES {
        anyhow::bail!(
            "password-store .gpg-id is unexpectedly large (> {GPG_ID_MAX_BYTES} bytes); refusing to read it"
        );
    }
    // open は final component が走査後に symlink へ差し替えられていればそれを辿る。直後に fd の fstat を取り、
    // 走査時点の dev/ino と一致しなければ TOCTOU として停止する（読まない）。
    let file = std::fs::File::open(gpg_id_path).context("failed to open password-store .gpg-id")?;
    let post = file
        .metadata()
        .context("failed to stat password-store .gpg-id")?;
    if !post.file_type().is_file() || pre.dev() != post.dev() || pre.ino() != post.ino() {
        anyhow::bail!(
            "password-store .gpg-id changed between stat and open (possible symlink swap); refusing to read it"
        );
    }
    // size cap を `metadata().len()` だけに頼らず、read 自体も上限 + 1 byte で打ち切る（special file 等で
    // len が 0 報告でも endless に読めない）。上限を超えたら異常として拒否する。
    let mut contents = String::new();
    let read = (&file)
        .take(GPG_ID_MAX_BYTES + 1)
        .read_to_string(&mut contents)
        .context("failed to read password-store .gpg-id")?;
    if read as u64 > GPG_ID_MAX_BYTES {
        anyhow::bail!(
            "password-store .gpg-id is unexpectedly large (> {GPG_ID_MAX_BYTES} bytes); refusing to read it"
        );
    }
    Ok(contents
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .map(str::to_owned)
        .collect())
}

/// store tree から最初に見つかった regular file の `*.gpg` entry の path を 1 件だけ返す（無ければ `None`）。
///
/// サンプルにするのは `file_type.is_file()` が真（= regular file）の `*.gpg` だけであり、symlink の `*.gpg` は
/// 選ばない。`DirEntry::file_type()` は link を辿らないため symlink は dir でも file でもなく、後段の
/// `std::fs::read` が link を辿って cloned store の外（例: `/dev/zero` で hang/OOM、外部の復号可能ファイルで
/// 偽の可読性成功）へ抜ける経路を断つ。directory branch も `file_type.is_dir()` が symlinked dir で偽になるため
/// symlinked directory を辿らない。除外するのは Git 管理ディレクトリ `.git` だけであり、それ以外の
/// dot-directory（例: `.aws/credentials.gpg`）は走査対象に含める。pass entry が dot-directory 配下にしか無い
/// store でもサンプル entry を取りこぼさず、authoritative な復号確認を確実に行うためである。復号確認に使う
/// サンプルを 1 件得るための浅い走査であり、全 entry の列挙はしない。`read_dir` が失敗したディレクトリは
/// （探索全体を中断せず）読み飛ばして走査を継続する。これにより、一過性の I/O 失敗で「サンプル entry なし」と
/// 誤判定し、authoritative な復号確認を取りこぼすことを防ぐ。
fn find_sample_entry(store_root: &Path) -> Option<PathBuf> {
    let mut stack = vec![store_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                // 除外するのは `.git` のみ。他の dot-directory（`.aws` など）は pass entry を含みうるため走査する。
                let is_git_dir = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name == ".git");
                if !is_git_dir {
                    stack.push(path);
                }
            } else if file_type.is_file()
                && path.extension().and_then(|ext| ext.to_str()) == Some("gpg")
            {
                // regular file の `*.gpg` だけをサンプルにする。`file_type.is_file()` は symlink で偽になるため、
                // symlink の `*.gpg` は選ばない。後段の復号確認（`verify_can_decrypt`）も symlink_metadata で
                // regular file と dev/ino を確認してから `File::open` し fd の dev/ino 一致を照合して読むため、
                // 走査〜読取り間の symlink 差し替えで store 外へ抜けない。
                return Some(path);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    //! filesystem adapter の安全読取り（symlink_metadata で regular file と dev/ino を確認 → `File::open` →
    //! fd の dev/ino 一致照合 + size cap）の単体テスト。temp-dir dev 依存を増やさないため、
    //! `std::env::temp_dir()` 配下に一意 directory を std だけで作成し、test 終了時に削除する。

    use std::path::PathBuf;

    use super::{GPG_ID_MAX_BYTES, read_gpg_id_recipients};

    /// test 専用の一意 temp directory（drop 時に再帰削除）。
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            let unique = format!(
                "dotfiles-store-adapter-{tag}-{}-{:?}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock after epoch")
                    .as_nanos()
            );
            let path = std::env::temp_dir().join(unique);
            std::fs::create_dir(&path).expect("create unique temp dir");
            Self { path }
        }

        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    /// regular file の `.gpg-id` は空行・`#` コメントを除いた recipient 行を未 trim で返す。
    #[test]
    fn read_gpg_id_recipients_reads_regular_file() {
        let dir = TempDir::new("regular");
        let gpg_id = dir.path().join(".gpg-id");
        std::fs::write(&gpg_id, "# comment\n\nABCDEF0123\n  Trailing Spaces  \n")
            .expect("write .gpg-id");

        let recipients = read_gpg_id_recipients(&gpg_id).expect("read recipients");
        assert_eq!(recipients, vec!["ABCDEF0123", "  Trailing Spaces  "]);
    }

    /// final component が symlink の `.gpg-id` は `symlink_metadata` の `is_file()` 判定が偽になるため、
    /// link 先を読まずに失敗する。
    #[test]
    fn read_gpg_id_recipients_rejects_symlink() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new("symlink");
        let target = dir.path().join("real-recipients");
        std::fs::write(&target, "ABCDEF0123\n").expect("write symlink target");
        let gpg_id = dir.path().join(".gpg-id");
        symlink(&target, &gpg_id).expect("create symlink .gpg-id");

        let result = read_gpg_id_recipients(&gpg_id);
        assert!(
            result.is_err(),
            "symlinked .gpg-id must be refused, not followed"
        );
    }

    /// `GPG_ID_MAX_BYTES` を超える `.gpg-id` は読まずに拒否する。
    #[test]
    fn read_gpg_id_recipients_rejects_oversize_file() {
        let dir = TempDir::new("oversize");
        let gpg_id = dir.path().join(".gpg-id");
        let oversized = vec![b'A'; (GPG_ID_MAX_BYTES as usize) + 16];
        std::fs::write(&gpg_id, oversized).expect("write oversize .gpg-id");

        let result = read_gpg_id_recipients(&gpg_id);
        assert!(result.is_err(), "oversize .gpg-id must be refused");
    }
}
