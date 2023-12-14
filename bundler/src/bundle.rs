use crate::permissionables::{permissions::Permissions, proposals::Proposals, sessions::Sessions};
use flate2::{write::GzEncoder, Compression};
use serde::Serialize;
use sqlx::MySqlPool;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};
use tar::Header;

#[derive(Debug, Serialize)]
struct WasmModule {
    pub entrypoint: String,
    pub module: String,
}

#[derive(Debug, Hash, Serialize)]
pub struct NoMetadata;

#[derive(Debug, Serialize)]
struct Manifest<Metadata>
where
    Metadata: Serialize,
{
    revision: String,
    roots: Vec<String>,
    wasm: Vec<WasmModule>,
    metadata: Metadata,
}

trait FromByteSlice {
    fn from_bytes(slice: &[u8]) -> Self;
}

impl FromByteSlice for Header {
    fn from_bytes(slice: &[u8]) -> Self {
        let mut header = Self::new_gnu();
        header.set_size(slice.len() as u64);
        header.set_cksum();
        header
    }
}

pub struct Bundle<Metadata>
where
    Metadata: Serialize,
{
    manifest: Manifest<Metadata>,
    proposals: Proposals,
    sessions: Sessions,
    permissions: Permissions,
}

const BUNDLE_PREFIX: &str = "diamond/data";

impl<Metadata> Bundle<Metadata>
where
    Metadata: Hash + Serialize,
{
    pub fn new(
        metadata: Metadata,
        proposals: Proposals,
        sessions: Sessions,
        permissions: Permissions,
    ) -> Self {
        let mut hasher = DefaultHasher::new();
        metadata.hash(&mut hasher);
        proposals.hash(&mut hasher);
        sessions.hash(&mut hasher);
        permissions.hash(&mut hasher);
        let hash = hasher.finish();

        Self {
            manifest: Manifest {
                revision: format!("{}:{}", crate::built_info::PKG_VERSION, hash),
                roots: vec![BUNDLE_PREFIX.to_string()],
                wasm: vec![],
                metadata,
            },
            proposals,
            sessions,
            permissions,
        }
    }

    pub async fn fetch(metadata: Metadata, ispyb_pool: &MySqlPool) -> Result<Self, sqlx::Error> {
        let proposals = Proposals::fetch(ispyb_pool).await?;
        let sessions = Sessions::fetch(ispyb_pool).await?;
        let permissions = Permissions::fetch(ispyb_pool).await?;
        Ok(Self::new(metadata, proposals, sessions, permissions))
    }

    pub fn revision(&self) -> &str {
        &self.manifest.revision
    }

    pub fn to_tar_gz(&self) -> Result<Vec<u8>, anyhow::Error> {
        let mut bundle_builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::best()));

        let manifest = serde_json::to_vec(&self.manifest)?;
        let mut manifest_header = Header::from_bytes(&manifest);
        bundle_builder.append_data(&mut manifest_header, ".manifest", manifest.as_slice())?;

        let proposals = serde_json::to_vec(&self.proposals)?;
        let mut proposals_header = Header::from_bytes(&proposals);
        bundle_builder.append_data(
            &mut proposals_header,
            format!("{BUNDLE_PREFIX}/users/proposals/data.json"),
            proposals.as_slice(),
        )?;

        let sessions = serde_json::to_vec(&self.sessions)?;
        let mut sessions_header = Header::from_bytes(&sessions);
        bundle_builder.append_data(
            &mut sessions_header,
            format!("{BUNDLE_PREFIX}/users/sessions/data.json"),
            sessions.as_slice(),
        )?;

        let permissions = serde_json::to_vec(&self.permissions)?;
        let mut permissions_header = Header::from_bytes(&permissions);
        bundle_builder.append_data(
            &mut permissions_header,
            format!("{BUNDLE_PREFIX}/users/permissions/data.json"),
            permissions.as_slice(),
        )?;

        Ok(bundle_builder.into_inner()?.finish()?)
    }
}
