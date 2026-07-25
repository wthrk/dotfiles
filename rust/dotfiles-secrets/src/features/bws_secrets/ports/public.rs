//! Cross-feature capability contracts owned by `bws_secrets`.
pub(crate) use super::bw::BwsClientPort;
#[cfg(test)]
pub(crate) use super::bw::MockBwsClientPort;
/// BWS capability の consumer が port 境界で受け渡す opaque lookup values。
///
/// これらは BWS feature が所有する public contract である。consumer は owner の private
/// domain module を import せず、この module だけを通じて候補と opaque ID を扱う。
pub use crate::features::bws_secrets::domain::bws::{
    BwsLookupCandidate, BwsProjectId, BwsProjectName, BwsSecretId, BwsSecretName, BwsSecretValue,
};
