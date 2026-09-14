//! Generate distinct mTLS node bundles and atomically publish a new directory.
use crate::provision_files::{self as files, Child};
use crate::{ClusterId, Config, Member, NodeId, Security};
use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use rustix::fs::{self, AtFlags, Mode, RenameFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::File,
    io,
    net::SocketAddr,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

pub struct Provisioned {
    pub directory: PathBuf,
    pub nodes: Vec<ProvisionedNode>,
}
pub struct ProvisionedNode {
    pub node: NodeId,
    pub settings: PathBuf,
}
#[derive(Clone)]
pub struct NodeSettings {
    pub routing: Config,
    pub security: Security,
    pub bind: SocketAddr,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsFile {
    version: u8,
    cluster: ClusterId,
    local: NodeId,
    members: Vec<Member>,
    bind: SocketAddr,
    ca: String,
    certificate: String,
    key: String,
}
impl NodeSettings {
    /// Load a closed bounded configuration and regular credentials from its pinned directory.
    pub fn load(path: &Path) -> io::Result<Self> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let filename = path.file_name().and_then(|s| s.to_str()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid node settings path")
        })?;
        let directory = files::open_directory(parent)?;
        let bytes = files::read(&directory, filename, 65_536, false)?;
        let settings: SettingsFile = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if settings.version != 1 || settings.bind.port() == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid node settings version or bind",
            ));
        }
        let routing = Config::new(settings.cluster, settings.local, settings.members)
            .map_err(io::Error::other)?;
        let ca = files::read(&directory, &settings.ca, 16_384, false)?;
        let certificate = files::read(&directory, &settings.certificate, 16_384, false)?;
        let key = Zeroizing::new(files::read(&directory, &settings.key, 8192, true)?);
        let security = Security::from_der(&ca, &certificate, &key)?;
        if routing
            .member(routing.local())
            .is_none_or(|m| m.certificate_sha256 != security.leaf_fingerprint)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "node credential identity mismatch",
            ));
        }
        Ok(Self {
            routing,
            security,
            bind: settings.bind,
        })
    }
}
struct Credential {
    certificate: Vec<u8>,
    key: Zeroizing<Vec<u8>>,
}
struct Staging {
    parent: File,
    name: String,
    directory: File,
    children: Vec<Child>,
    published: bool,
}
impl Staging {
    fn publish(&mut self, name: &str) -> io::Result<()> {
        self.directory.sync_all()?;
        if !files::matches_entry(&self.parent, self.name.as_ref(), &self.directory) {
            return Err(io::Error::other("cluster staging directory was replaced"));
        }
        fs::renameat_with(
            &self.parent,
            &self.name,
            &self.parent,
            name,
            RenameFlags::NOREPLACE,
        )?;
        self.published = true;
        self.parent.sync_all()?;
        Ok(())
    }
}
impl Drop for Staging {
    fn drop(&mut self) {
        if self.published {
            return;
        }
        for child in &self.children {
            child.clean();
            if files::matches_entry(&self.directory, child.name.as_ref(), &child.directory) {
                let _ = fs::unlinkat(&self.directory, &child.name, AtFlags::REMOVEDIR);
            }
        }
        if files::matches_entry(&self.parent, self.name.as_ref(), &self.directory) {
            let _ = fs::unlinkat(&self.parent, &self.name, AtFlags::REMOVEDIR);
        }
    }
}

/// Publish complete private bundles to a NEW directory; never overwrite an existing destination.
/// CA signing material stays in memory and is never included in runtime node bundles.
/// The existing parent must be trusted and not group/other-writable. This does not
/// defend against another process running under the same account modifying that parent.
pub fn provision(
    directory: &Path,
    cluster: ClusterId,
    mut nodes: Vec<(NodeId, SocketAddr)>,
) -> io::Result<Provisioned> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "provision requires 1 through 16 unique nodes and nonzero unique endpoints",
        )
    };
    if nodes.is_empty() || nodes.len() > 16 {
        return Err(invalid());
    }
    nodes.sort_by(|a, b| a.0.cmp(&b.0));
    let mut identities = BTreeSet::new();
    let mut addresses = BTreeSet::new();
    for (id, address) in &nodes {
        if address.port() == 0 || !identities.insert(id.clone()) || !addresses.insert(*address) {
            return Err(invalid());
        }
    }
    let parent_path = directory
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = directory
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(invalid)?;
    files::basename(name)?;
    let parent = files::open_directory(parent_path)?;
    if parent.metadata()?.mode() & 0o022 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "cluster parent must not be writable by other users or groups",
        ));
    }
    match fs::statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "cluster destination already exists",
            ));
        }
        Err(error) if error == rustix::io::Errno::NOENT => (),
        Err(error) => return Err(error.into()),
    }
    let key = KeyPair::generate().map_err(io::Error::other)?;
    let mut parameters =
        CertificateParams::new(vec!["fern-cluster-ca.invalid".into()]).map_err(io::Error::other)?;
    parameters.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    parameters.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca = parameters
        .self_signed(&key)
        .map_err(io::Error::other)?
        .der()
        .to_vec();
    let issuer = Issuer::new(parameters, key);
    let mut credentials = Vec::new();
    let mut members = Vec::new();
    for (index, (id, address)) in nodes.iter().enumerate() {
        let tls_name = format!("node-{index}.fern.invalid");
        let key = KeyPair::generate().map_err(io::Error::other)?;
        let mut parameters =
            CertificateParams::new(vec![tls_name.clone()]).map_err(io::Error::other)?;
        parameters.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        parameters.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        let certificate = parameters
            .signed_by(&key, &issuer)
            .map_err(io::Error::other)?
            .der()
            .to_vec();
        members.push(Member {
            id: id.clone(),
            endpoint: address.to_string(),
            tls_name,
            certificate_sha256: Sha256::digest(&certificate).into(),
        });
        credentials.push(Credential {
            certificate,
            key: Zeroizing::new(key.serialize_der()),
        });
    }
    Config::new(cluster.clone(), nodes[0].0.clone(), members.clone()).map_err(io::Error::other)?;
    let mut random = [0u8; 16];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut random)
        .map_err(|_| io::Error::other("cluster staging randomness failed"))?;
    let staging_name = format!(
        ".fern-cluster-{}",
        random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    fs::mkdirat(&parent, &staging_name, Mode::from_bits_truncate(0o700))?;
    let opened = files::open_child(&parent, &staging_name)?;
    let mut staging = Staging {
        parent,
        name: staging_name,
        directory: opened,
        children: Vec::new(),
        published: false,
    };
    let mut published = Vec::new();
    for (index, ((id, address), credential)) in nodes.iter().zip(credentials).enumerate() {
        let child_name = format!("node-{index}");
        let child = Child::create(&staging.directory, child_name.clone())?;
        staging.children.push(child);
        let child = staging.children.last_mut().unwrap();
        child.write("ca.der", &ca)?;
        child.write("certificate.der", &credential.certificate)?;
        child.write("key.der", &credential.key)?;
        let settings = SettingsFile {
            version: 1,
            cluster: cluster.clone(),
            local: id.clone(),
            members: members.clone(),
            bind: *address,
            ca: "ca.der".into(),
            certificate: "certificate.der".into(),
            key: "key.der".into(),
        };
        child.write(
            "node.json",
            &serde_json::to_vec_pretty(&settings).map_err(io::Error::other)?,
        )?;
        child.directory.sync_all()?;
        published.push(ProvisionedNode {
            node: id.clone(),
            settings: directory.join(child_name).join("node.json"),
        });
    }
    staging.publish(name)?;
    Ok(Provisioned {
        directory: directory.into(),
        nodes: published,
    })
}

#[cfg(test)]
#[path = "provision_tests.rs"]
mod tests;
