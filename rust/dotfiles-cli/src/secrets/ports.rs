//! `dotfiles secrets` application 層が外部境界へ要求する port。
//!
//! application はこの module の trait だけに依存し、実機 YubiKey と test stub の具体的な
//! 入出力差分は adapter 側に閉じる。
//!
//! ## 設計制約
//!
//! - port は `domain` にのみ依存可能。`support` 型はシグネチャに使えない
//! - TTY 判定・prompt 文言・入力形式の詳細は adapter 所有
//! - 非対話条件チェックも境界が行い、application は use case 順序だけを所有する
//! - DTO（`EnrollmentBytes` 等）は port に置かない。adapter と application の共有型は
//!   secrets module ルートに `pub(crate)` で定義する

use zeroize::Zeroizing;

use crate::Result;

use super::domain::PivObjectId;
use super::EnrollmentBytes;

/// application use case が利用する外部 I/O 境界。
///
/// 実機 adapter と test stub は同じ device 操作順序をこの trait で共有する。
/// 非対話条件・device 取得・secret 入力・出力はすべてこの trait を通す。
pub trait SecretsBoundary {
    type Device: SecretDevice;

    fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device>;
    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
    ) -> Result<Self::Device>;

    /// 非対話時に serial が必須であることを確認する。
    ///
    /// `serial` が `None` かつ非対話実行の場合は `error_message` で失敗する。
    /// TTY 判定は adapter 内部で行い、port シグネチャには露出しない。
    fn require_serial(&self, serial: Option<u32>, error_message: &'static str) -> Result<()>;

    /// 非対話時に必須 option が指定されていることを確認する。
    ///
    /// `enabled` が `false` かつ非対話実行の場合は `option_name` を含む error で失敗する。
    fn require_option(&self, enabled: bool, option_name: &'static str) -> Result<()>;

    /// `--stdin` が pipe/redirect 入力を指していることを確認する。
    ///
    /// stdin が TTY の場合は error で失敗する。
    fn require_stdin_pipe(&self) -> Result<()>;

    /// `--stdin-json` が pipe/redirect 入力を指していることを確認する。
    ///
    /// `enabled` が `true` かつ stdin が TTY の場合は error で失敗する。
    fn require_stdin_json_pipe(&self, enabled: bool) -> Result<()>;

    /// stdout が pipe/redirect を向いていることを確認する。
    ///
    /// stdout が TTY の場合は secret 書き込みを拒否して error で失敗する。
    fn require_stdout_pipe(&self) -> Result<()>;

    /// echo なしの prompt で YubiKey PIN を読み、zeroize 保護済み bytes として返す。
    fn read_yubikey_pin_bytes(&self) -> Result<Zeroizing<Vec<u8>>>;

    /// echo なしの prompt で 1 行を読み、zeroize 保護済み bytes として返す。
    fn read_hidden_bytes(&self, prompt: &str, limit: usize) -> Result<Zeroizing<Vec<u8>>>;

    /// 表示 prompt で 1 行を読み、zeroize 保護済み bytes として返す。
    fn read_visible_line_bytes(&self, prompt: &str, limit: usize) -> Result<Zeroizing<Vec<u8>>>;

    /// stdin から 1 secret を読み、zeroize 保護済み bytes として返す。
    fn read_stdin_bytes(&self, limit: usize) -> Result<Zeroizing<Vec<u8>>>;

    /// stdin JSON から enrollment secret set の 3 field を読み、bytes として返す。
    fn read_enrollment_json_bytes(
        &self,
        input_limit: usize,
        field_limit: usize,
    ) -> Result<EnrollmentBytes>;

    /// 復号済み secret bytes を stdout へ書き込む。stdout が TTY の場合は停止する。
    fn write_secret_to_stdout(&self, bytes: &[u8]) -> Result<()>;

    /// summary を JSON として stdout へ出力する。
    fn write_report(&self, value: &impl serde::Serialize) -> Result<()>;

    /// TTY で次の YubiKey を更新するか yes/no で確認し、応答を返す。
    ///
    /// stdin が TTY でない場合は false を返す。prompt 文言は adapter 側で保持する。
    fn prompt_continue_rotation(&self) -> Result<bool>;
}

/// device 境界が必要とする YubiKey device の取得契約。
///
/// 実プロセス境界はこの contract を通じてだけ device を開き、実機 discovery と CLI 統合テストの
/// 代替 device 実装は同じ取得順序をこの trait で共有する。`SecretDevice`（port）と serial 値だけに
/// 依存し、terminal 待機や interrupt policy のような実装詳細は実装側に閉じる。
pub trait SecretDeviceFactory {
    type Device: SecretDevice;

    /// 通常操作対象 device を serial 指定または対話選択で開く。
    fn open_device(&mut self, serial: Option<u32>) -> Result<Self::Device>;

    /// spare 登録対象 device を開く。
    ///
    /// spare が primary と別 serial であることの確認は実装側が担う。
    fn open_spare_device(
        &mut self,
        spare_serial: Option<u32>,
        primary_serial: Option<u32>,
    ) -> Result<Self::Device>;
}

/// storage 操作が必要とする device API。
///
/// 実機 YubiKey と fake test double はこの最小操作を共有する。
pub trait SecretDevice {
    /// device 固有の serial。AEAD additional data にも含める。
    fn serial(&self) -> u32;
    /// secret storage 用 PIV key が存在するか確認する。
    fn key_exists(&mut self) -> Result<bool>;
    /// secret storage 用 PIV key 生成に必要な device 固有条件を確認する。
    fn check_key_generation_preconditions(&mut self) -> Result<()>;
    /// setup で永続書き込みを始める前に management key 認証可否を確認する。
    fn check_management_auth_preconditions(&mut self) -> Result<()>;
    /// secret storage 用 PIV key を device 内で生成する。
    fn generate_key(&mut self) -> Result<()>;
    /// PIV data object を読み出す。
    ///
    /// object が存在しない場合は `None` を返す。
    fn read_object(&mut self, object_id: PivObjectId) -> Result<Option<Vec<u8>>>;
    /// PIV data object に caller 所有の mutable bytes を保存する。
    fn write_object(&mut self, object_id: PivObjectId, value: &mut [u8]) -> Result<()>;
    /// content encryption key を device の public key で wrap する。
    fn wrap_key(&mut self, key: &[u8]) -> Result<Vec<u8>>;
    /// private key operation 前に application 側の PIN 入力境界を通す必要がある状態を表す。
    fn requires_pin_input(&self) -> bool;
    /// private key operation の前に、入力済み PIN で PIV session を検証する。
    fn verify_pin(&mut self, pin: &[u8]) -> Result<()>;
    /// wrapped content encryption key を device 境界内で unwrap して返す。
    ///
    /// 戻り値は zeroize 保護済みにし、呼び出し元が Drop した時点で content encryption key がヒープ上にゼロ化されることを保証する。
    fn unwrap_key(&mut self, wrapped_key: &[u8]) -> Result<Zeroizing<Vec<u8>>>;
}
