//! password-store-remote の保管側 provisioning 順序を固定し、値取得と検証を port/domain 境界へ閉じる。

use anyhow::Context;

use crate::Result;
use crate::secrets::{
    domain::{
        bws::{BwsLookupResolution, BwsProjectName, BwsSecretName},
        commands::ProvisionPasswordStoreRemoteCommand,
        pass_restore::PasswordStoreRemote,
    },
    ports,
};

/// private `password-store` repository の clone URL を BWS project `dotfiles-secret-recovery` へ
/// create または既存照合する provisioning use case。
///
/// 設計「初期登録手順」step3 が定める `password-store-remote` の保管コマンドを、`gpg-backup register`
/// と対称な順序制御として固定する。BWS への登録には BWS access token を使い、この token を
/// hidden prompt（TTY）/ pipe（stdin）から保護値として取得したうえで、project name から project ID
/// を解決（0件なら作成、1件なら使用、複数件なら停止）し、既存 `password-store-remote` secret の有無を
/// 確認する。不在なら input port から clone URL を取得して create し、ちょうど 1 件存在する場合は
/// configured origin から導ける権威的 repository identity と一致するときだけ既存値を使用する。
/// origin が無く照合できない既存値は fail-closed で停止する。
/// secret の複数件は domain failure として停止する。
///
/// この command は YubiKey storage を読まない。provisioning で使う登録用 token は
/// `BwsAccessTokenInputPort` で受け取り、YubiKey へ保存しない。YubiKey へ保存する `bws-access-token` は
/// 復旧時の read 用最小権限 token を別経路で用意する。token は実 credential のため secret として保護経路で
/// 扱い、argv / log / 永続ファイルへ出さない。
///
/// 順序を application に固定するのは「token 取得・project/secret の解決を済ませ、既存値がある場合は
/// 入力・保存へ進まない」停止条件の責務境界を保護するためである。clone URL は private repo の SSH clone URL であって秘密
/// 情報ではないため、保護 buffer・非表示入力・zeroize を使わず非秘匿の値として扱う。設定済み
/// password-store origin がある場合は repository identity として SSH/HTTPS GitHub URL を許容し、BWS 登録値は
/// application/domain 側で `git@github.com:<owner>/<repo>.git` へ正規化する。origin が無い場合だけ
/// controlling TTY の可視対話入力から clone URL を取得し、`PasswordStoreRemote::parse` の URL 形式検証
/// （domain rule）へ通してから create へ渡す。既存値がある場合は provenance marker の後付け更新や
/// 値更新へ進まず、その BWS secret を使用するか停止するかだけを決める。configured origin が観測できない
/// 既存値は権威的 identity と照合できないため停止する。
pub(crate) async fn run_provision_password_store_remote<A, B, U>(
    command: ProvisionPasswordStoreRemoteCommand,
    token_input: &A,
    bws_client: &B,
    store: &impl ports::PasswordStorePort,
    url_input: &U,
) -> Result<()>
where
    A: ports::BwsAccessTokenInputPort,
    B: ports::BwsClientPort,
    U: ports::PasswordStoreRemoteInputPort,
{
    let _ = command;
    // BWS 登録用 access token を hidden prompt / pipe から保護値として取得し、復旧 project を解決する。
    // provisioning command は YubiKey storage を読まず、YubiKey 保存用の復旧 token とは分離する。
    let access_token = token_input
        .read_bws_access_token_for_provisioning()
        .context("`pass-remote register` failed while reading `bws-access-token (create/use)`")?;
    let project_name = BwsProjectName::DOTFILES_SECRET_RECOVERY;
    let project_candidates = bws_client
        .list_bws_projects(&access_token)
        .await
        .with_context(|| {
            format!(
                "`pass-remote register` failed while resolving BWS project `{}`",
                project_name.as_str()
            )
        })?;
    let project_id = match project_name.resolve_lookup(project_candidates) {
        BwsLookupResolution::Missing => bws_client
            .create_bws_project(&access_token, project_name)
            .await
            .with_context(|| {
                format!(
                    "`pass-remote register` failed while creating BWS project `{}`",
                    project_name.as_str()
                )
            })?,
        BwsLookupResolution::Unique(project_id) => project_id,
        BwsLookupResolution::Ambiguous => {
            anyhow::bail!("multiple bws projects matched: {}", project_name.as_str())
        }
    };

    // 既存 password-store-remote secret の候補を取得する。0件は create、1件は使用、複数件は停止。
    // `resolve_id` は 0件/複数件をどちらも `Err` にするため、create 経路と取り違えないよう同名候補の
    // 件数を数える。対象同一性の exact match は domain helper に委ね、application は create/use/停止の
    // 分岐だけを扱う。
    let secret_name = BwsSecretName::PasswordStoreRemote;
    let candidates = bws_client
        .list_bws_secrets(&access_token, &project_id)
        .await
        .with_context(|| {
            format!(
                "`pass-remote register` failed while listing secret `{}` in project `{}`",
                secret_name.key(),
                project_id.as_str()
            )
        })?;
    match secret_name.resolve_lookup(candidates) {
        BwsLookupResolution::Missing => {
            // 不在: clone URL を input port から取得し、検証してから新規 create する。
            let remote = match store.configured_origin_remote()? {
                Some(remote) => PasswordStoreRemote::from_configured_origin(&remote)?,
                None => PasswordStoreRemote::parse(&url_input.read_password_store_remote_url()?)?,
            };
            bws_client
                .create_password_store_remote(&access_token, &project_id, &remote)
                .await
                .with_context(|| {
                    format!(
                        "`pass-remote register` failed while creating secret `{}` in project `{}`",
                        secret_name.key(),
                        project_id.as_str()
                    )
                })
                .map(|_id| ())
        }
        BwsLookupResolution::Unique(secret_id) => {
            let existing_remote = bws_client
                .fetch_password_store_remote(&access_token, &secret_id)
                .await
                .with_context(|| {
                    format!(
                        "`pass-remote register` failed while loading existing secret `{}`",
                        secret_name.key()
                    )
                })?;
            let Some(origin_remote) = store.configured_origin_remote()? else {
                anyhow::bail!(
                    "existing password-store-remote cannot be reused without a configured local origin"
                );
            };
            let expected_remote = PasswordStoreRemote::from_configured_origin(&origin_remote)?;
            if existing_remote != expected_remote {
                anyhow::bail!(
                    "existing password-store-remote does not match the configured local origin"
                );
            }
            Ok(())
        }
        BwsLookupResolution::Ambiguous => anyhow::bail!(
            "multiple password-store-remote secrets exist in the recovery project; refusing to provision"
        ),
    }
}

#[cfg(test)]
mod tests {
    //! provisioning の順序（BWS access token 取得→project 解決→secret 候補確認→create または既存使用）を
    //! mockall + Sequence で検証する単体テスト。
    //!
    //! token-input / url-input / bws backend を port mock で差し替え、BWS 登録に使う
    //! access token を BWS access token 入力経路から取得すること、未登録時に create が呼ばれること、
    //! ちょうど 1 件存在する場合は値入力・更新へ進まず既存 secret を使用すること、同名複数件で
    //! create へ進ませず停止すること、input port から取得した検証済み URL が create へ渡ること、
    //! 不正 URL が create より前に停止することを確認する。

    use crate::secrets::{
        domain::{
            bws::BwsSecretId, commands::ProvisionPasswordStoreRemoteCommand,
            pass_restore::PasswordStoreRemote,
        },
        ports,
        ports::ProtectedSecret,
    };

    use super::run_provision_password_store_remote;

    const REMOTE_URL: &str = "git@github.com:owner/password-store.git";

    fn material(bytes: &'static [u8]) -> ProtectedSecret {
        ProtectedSecret::from_test_bytes(bytes).expect("test secret")
    }

    fn project_candidate() -> Vec<
        crate::secrets::domain::bws::BwsLookupCandidate<crate::secrets::domain::bws::BwsProjectId>,
    > {
        vec![crate::secrets::domain::bws::BwsLookupCandidate {
            id: crate::secrets::domain::bws::BwsProjectId::new("project-1"),
            name: "dotfiles-secret-recovery".to_owned(),
        }]
    }

    fn command() -> ProvisionPasswordStoreRemoteCommand {
        ProvisionPasswordStoreRemoteCommand
    }

    fn store_without_origin() -> ports::MockPasswordStorePort {
        let mut store = ports::MockPasswordStorePort::new();
        store
            .expect_configured_origin_remote()
            .times(1)
            .returning(|| Ok(None));
        store
    }

    /// BWS access token を hidden prompt / pipe から取得する port mock を共通設定する。
    ///
    /// この mock は hidden prompt / pipe 相当の入力経路として BWS access token を返す。
    /// device/pin/storage port は構成へ一切渡さず、provisioning command が YubiKey storage を読まないことを固定する。
    fn token_input() -> ports::MockBwsAccessTokenInputPort {
        let mut token_input = ports::MockBwsAccessTokenInputPort::new();
        token_input
            .expect_read_bws_access_token_for_provisioning()
            .times(1)
            .returning(|| Ok(material(b"provisioning-token")));
        token_input
    }

    #[tokio::test]
    async fn provision_creates_secret_when_absent() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let token = token_input();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        // 同名 secret は未登録（list は空）。create 経路へ進む。
        bws.expect_list_bws_secrets()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(Vec::new()));

        // input port から URL を取得する。
        let mut url_input = ports::MockPasswordStoreRemoteInputPort::new();
        url_input
            .expect_read_password_store_remote_url()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(REMOTE_URL.to_owned()));

        bws.expect_create_password_store_remote()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|_, _, remote: &PasswordStoreRemote| remote.as_str() == REMOTE_URL)
            .returning(|_, _, _| Ok(BwsSecretId::new("new-id")));

        let store = store_without_origin();
        run_provision_password_store_remote(command(), &token, &bws, &store, &url_input).await
    }

    #[tokio::test]
    async fn provision_normalizes_existing_https_origin_to_ssh_bws_value() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let token = token_input();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        bws.expect_list_bws_secrets()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(Vec::new()));

        let mut store = ports::MockPasswordStorePort::new();
        store
            .expect_configured_origin_remote()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| {
                Ok(Some(
                    "https://github.com/owner/password-store.git".to_owned(),
                ))
            });

        let mut url_input = ports::MockPasswordStoreRemoteInputPort::new();
        url_input.expect_read_password_store_remote_url().times(0);

        bws.expect_create_password_store_remote()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|_, _, remote: &PasswordStoreRemote| remote.as_str() == REMOTE_URL)
            .returning(|_, _, _| Ok(BwsSecretId::new("new-id")));

        run_provision_password_store_remote(
            ProvisionPasswordStoreRemoteCommand,
            &token,
            &bws,
            &store,
            &url_input,
        )
        .await
    }

    #[tokio::test]
    async fn provision_creates_project_when_missing_before_secret_create() -> crate::Result<()> {
        let mut sequence = mockall::Sequence::new();
        let token = token_input();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(Vec::new()));
        bws.expect_create_bws_project()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|_, project_name| project_name.as_str() == "dotfiles-secret-recovery")
            .returning(|_, _| {
                Ok(crate::secrets::domain::bws::BwsProjectId::new(
                    "project-new",
                ))
            });
        bws.expect_list_bws_secrets()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, project_id| {
                assert_eq!(project_id.as_str(), "project-new");
                Ok(Vec::new())
            });

        let mut url_input = ports::MockPasswordStoreRemoteInputPort::new();
        url_input
            .expect_read_password_store_remote_url()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(REMOTE_URL.to_owned()));

        bws.expect_create_password_store_remote()
            .times(1)
            .in_sequence(&mut sequence)
            .withf(|_, project_id, remote: &PasswordStoreRemote| {
                project_id.as_str() == "project-new" && remote.as_str() == REMOTE_URL
            })
            .returning(|_, _, _| Ok(BwsSecretId::new("new-id")));

        let store = store_without_origin();
        run_provision_password_store_remote(command(), &token, &bws, &store, &url_input).await
    }

    /// 復旧 project が複数一致する場合は、secret 確認・URL 入力・create へ進ませず停止する。
    #[tokio::test]
    async fn provision_stops_when_duplicate_projects_exist() {
        let token = token_input();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects().times(1).returning(|_| {
            Ok(vec![
                crate::secrets::domain::bws::BwsLookupCandidate {
                    id: crate::secrets::domain::bws::BwsProjectId::new("project-1"),
                    name: "dotfiles-secret-recovery".to_owned(),
                },
                crate::secrets::domain::bws::BwsLookupCandidate {
                    id: crate::secrets::domain::bws::BwsProjectId::new("project-2"),
                    name: "dotfiles-secret-recovery".to_owned(),
                },
            ])
        });
        bws.expect_create_bws_project().times(0);
        bws.expect_list_bws_secrets().times(0);
        bws.expect_create_password_store_remote().times(0);

        let mut url_input = ports::MockPasswordStoreRemoteInputPort::new();
        url_input.expect_read_password_store_remote_url().times(0);

        let mut store = ports::MockPasswordStorePort::new();
        store.expect_configured_origin_remote().times(0);

        let result =
            run_provision_password_store_remote(command(), &token, &bws, &store, &url_input).await;

        assert!(
            result.is_err(),
            "duplicate recovery projects must stop password-store-remote provisioning"
        );
    }

    #[tokio::test]
    async fn provision_stops_when_existing_secret_cannot_be_verified_against_origin() {
        let token = token_input();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsSecretId::new("pass-id"),
                name: "password-store-remote".to_owned(),
            }])
        });
        bws.expect_fetch_password_store_remote()
            .times(1)
            .returning(|_, _| PasswordStoreRemote::parse(REMOTE_URL));
        let mut url_input = ports::MockPasswordStoreRemoteInputPort::new();
        url_input.expect_read_password_store_remote_url().times(0);
        let mut store = ports::MockPasswordStorePort::new();
        store
            .expect_configured_origin_remote()
            .times(1)
            .returning(|| Ok(None));

        bws.expect_create_password_store_remote().times(0);

        let result =
            run_provision_password_store_remote(command(), &token, &bws, &store, &url_input).await;

        assert_eq!(
            result
                .expect_err("existing secret without authoritative origin must fail closed")
                .to_string(),
            "existing password-store-remote cannot be reused without a configured local origin"
        );
    }

    #[tokio::test]
    async fn provision_uses_existing_secret_when_configured_remote_matches() -> crate::Result<()> {
        let token = token_input();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsSecretId::new("pass-id"),
                name: "password-store-remote".to_owned(),
            }])
        });
        bws.expect_fetch_password_store_remote()
            .times(1)
            .returning(|_, _| PasswordStoreRemote::parse(REMOTE_URL));
        let mut url_input = ports::MockPasswordStoreRemoteInputPort::new();
        url_input.expect_read_password_store_remote_url().times(0);
        let mut store = ports::MockPasswordStorePort::new();
        store
            .expect_configured_origin_remote()
            .times(1)
            .returning(|| Ok(Some(REMOTE_URL.to_owned())));

        bws.expect_create_password_store_remote().times(0);

        run_provision_password_store_remote(command(), &token, &bws, &store, &url_input).await
    }

    #[tokio::test]
    async fn provision_stops_when_existing_secret_mismatches_configured_origin() {
        let token = token_input();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsSecretId::new("pass-id"),
                name: "password-store-remote".to_owned(),
            }])
        });
        bws.expect_fetch_password_store_remote()
            .times(1)
            .returning(|_, _| PasswordStoreRemote::parse(REMOTE_URL));
        bws.expect_create_password_store_remote().times(0);

        let mut url_input = ports::MockPasswordStoreRemoteInputPort::new();
        url_input.expect_read_password_store_remote_url().times(0);
        let mut store = ports::MockPasswordStorePort::new();
        store
            .expect_configured_origin_remote()
            .times(1)
            .returning(|| Ok(Some("git@github.com:owner/other-store.git".to_owned())));

        let result =
            run_provision_password_store_remote(command(), &token, &bws, &store, &url_input).await;

        assert!(
            result.is_err(),
            "configured local origin mismatch must fail closed instead of accepting stale BWS state"
        );
    }

    #[tokio::test]
    async fn provision_does_not_update_existing_secret_when_reusing_verified_origin()
    -> crate::Result<()> {
        let token = token_input();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![crate::secrets::domain::bws::BwsLookupCandidate {
                id: crate::secrets::domain::bws::BwsSecretId::new("pass-id"),
                name: "password-store-remote".to_owned(),
            }])
        });
        bws.expect_fetch_password_store_remote()
            .times(1)
            .returning(|_, _| PasswordStoreRemote::parse(REMOTE_URL));
        bws.expect_create_password_store_remote().times(0);

        let mut url_input = ports::MockPasswordStoreRemoteInputPort::new();
        url_input.expect_read_password_store_remote_url().times(0);
        let mut store = ports::MockPasswordStorePort::new();
        store
            .expect_configured_origin_remote()
            .times(1)
            .returning(|| Ok(Some(REMOTE_URL.to_owned())));

        run_provision_password_store_remote(command(), &token, &bws, &store, &url_input).await
    }

    /// 入力した clone URL が domain rule で不正なとき、application の URL 検証で停止し、`create_*`
    /// へ進ませないことを検証する。
    ///
    #[tokio::test]
    async fn provision_stops_when_input_url_is_invalid() {
        let token = token_input();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        // 同名 secret は未登録 → create 経路へ進む。
        bws.expect_list_bws_secrets()
            .returning(|_, _| Ok(Vec::new()));

        // port から不正 clone URL（domain 妥当でない値）を入力する。
        const INVALID_URL: &str = "https://example.invalid/repo.git";
        let mut url_input = ports::MockPasswordStoreRemoteInputPort::new();
        url_input
            .expect_read_password_store_remote_url()
            .times(1)
            .returning(|| Ok(INVALID_URL.to_owned()));

        // URL 検証で停止するため、保存境界へは到達しない。
        bws.expect_create_password_store_remote().times(0);

        let store = store_without_origin();
        let result =
            run_provision_password_store_remote(command(), &token, &bws, &store, &url_input).await;

        assert!(
            result.is_err(),
            "invalid clone URL must stop provisioning before the BWS save boundary"
        );
    }

    /// 同名 secret が複数件存在する場合は、create へ進ませず停止することを検証する。
    #[tokio::test]
    async fn provision_stops_when_duplicate_secrets_exist() {
        let token = token_input();

        let mut bws = ports::MockBwsClientPort::new();
        bws.expect_list_bws_projects()
            .returning(|_| Ok(project_candidate()));
        bws.expect_list_bws_secrets().returning(|_, _| {
            Ok(vec![
                crate::secrets::domain::bws::BwsLookupCandidate {
                    id: crate::secrets::domain::bws::BwsSecretId::new("dup-1"),
                    name: "password-store-remote".to_owned(),
                },
                crate::secrets::domain::bws::BwsLookupCandidate {
                    id: crate::secrets::domain::bws::BwsSecretId::new("dup-2"),
                    name: "password-store-remote".to_owned(),
                },
            ])
        });
        // 複数件検出で create へ進ませない。
        bws.expect_create_password_store_remote().times(0);

        let mut url_input = ports::MockPasswordStoreRemoteInputPort::new();
        url_input.expect_read_password_store_remote_url().times(0);

        let mut store = ports::MockPasswordStorePort::new();
        store.expect_configured_origin_remote().times(0);

        let result =
            run_provision_password_store_remote(command(), &token, &bws, &store, &url_input).await;

        assert!(
            result.is_err(),
            "duplicate password-store-remote secrets must stop provisioning"
        );
    }
}
