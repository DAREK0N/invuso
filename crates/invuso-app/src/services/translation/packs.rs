//! Downloadable translation packs (AP-21b, SET-07; decision 10.2): none
//! ships with the app, the user chooses which to download.
//!
//! Each pack is an Opus-MT model built by `scripts/opus-mt/export.py` and
//! published as assets of one release of the public `invuso-models`
//! repository (the code repository is private, its assets need a login). The only
//! network access is the download the user starts (AGENTS.md 7.6); every
//! file is checked against the size and SHA-256 sum recorded here, so a
//! changed or broken download is never loaded.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where the release assets live; file names are `<pair>.<file>`.
const RELEASE_URL: &str =
    "https://github.com/DAREK0N/invuso-models/releases/download/translation-packs-1";

/// A slow mobile connection must not abort a 140 MB download, a dead one
/// must not hang forever.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// One file of a pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackFile {
    pub name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

/// One language pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pack {
    /// ISO 639-1 codes.
    pub source: &'static str,
    pub target: &'static str,
    pub files: &'static [PackFile],
}

impl Pack {
    /// `ja-de`: directory and asset prefix.
    pub fn id(&self) -> String {
        format!("{}-{}", self.source, self.target)
    }

    /// Download size in bytes.
    pub fn size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// The packs on offer, from `scripts/opus-mt/export.py` (Helsinki-NLP
/// Opus-MT, Apache-2.0 according to their model cards).
pub const PACKS: &[Pack] = &[
    Pack {
        source: "ja",
        target: "de",
        files: &[
            PackFile {
                name: "config.json",
                size: 106,
                sha256: "e3134763380aea7f1f7836ed7d435f06f7e0207448c0d153748a1be3f963a5e8",
            },
            PackFile {
                name: "decoder.onnx",
                size: 89_617_530,
                sha256: "50cddeb38d65a1e018d76beb276e0ad097d70462db591ecda0db63406f341c25",
            },
            PackFile {
                name: "encoder.onnx",
                size: 51_162_889,
                sha256: "50822a927e4836190c42aa791bddb0183fe28424025dfe67fa9ad667682e7cbf",
            },
            PackFile {
                name: "source.tsv",
                size: 989_449,
                sha256: "eb6e4857effbe33a1701d97e9523ffb4be2994d3b57a8f4eb0100a56688bfb79",
            },
            PackFile {
                name: "vocab.txt",
                size: 626_811,
                sha256: "4622fedaf7a7aa82c2284f2dd432fa6eec71080942ebe0ca21750bc2d430c314",
            },
        ],
    },
    Pack {
        source: "ja",
        target: "en",
        files: &[
            PackFile {
                name: "config.json",
                size: 106,
                sha256: "e2f494462ba41090212614dc0937f42880c75b694eca4876e2a4913cd36aa614",
            },
            PackFile {
                name: "decoder.onnx",
                size: 88_382_902,
                sha256: "d3df796bf95858884396fbb8580ad18ebfa78b8ed58340bec821ff37b0981f8b",
            },
            PackFile {
                name: "encoder.onnx",
                size: 50_547_977,
                sha256: "45475b27169ead7cdd63c4e4af648006650274c39227121afbef69f51d884ba6",
            },
            PackFile {
                name: "source.tsv",
                size: 983_127,
                sha256: "3de861aecc0dd209c00c779cacbc5749774b041cc1984d341925b9e1f2261961",
            },
            PackFile {
                name: "vocab.txt",
                size: 581_288,
                sha256: "c4f3f266be5944e845ce69843ae4ac0e826e6f01d625df5d02e2b78b8eafec28",
            },
        ],
    },
];

/// The pack translating `source` into `target`, if one is on offer.
pub fn find(source: &str, target: &str) -> Option<&'static Pack> {
    PACKS
        .iter()
        .find(|p| p.source == source && p.target == target)
}

#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("no storage: {0}")]
    Storage(String),
    /// The server or the connection; the screen says it was the download.
    #[error("{0}")]
    Download(String),
    #[error("{0} is damaged (checksum differs)")]
    Damaged(&'static str),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Directory of installed packs.
fn packs_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("translation")
}

/// Where `pack` lives once installed.
pub fn pack_dir(data_dir: &Path, pack: &Pack) -> PathBuf {
    packs_dir(data_dir).join(pack.id())
}

/// Whether every file of `pack` is in place with its size; checksums were
/// verified when it was downloaded.
pub fn is_installed(data_dir: &Path, pack: &Pack) -> bool {
    let dir = pack_dir(data_dir, pack);
    pack.files.iter().all(|file| {
        std::fs::metadata(dir.join(file.name)).is_ok_and(|meta| meta.len() == file.size)
    })
}

/// Removes an installed pack.
pub fn delete(data_dir: &Path, pack: &Pack) -> Result<(), PackError> {
    let dir = pack_dir(data_dir, pack);
    match std::fs::remove_dir_all(&dir) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

/// Downloads `pack`, reporting `(bytes done, bytes total)`. Files go to a
/// staging directory first, so an aborted download never looks installed.
/// Returns early with `Ok(false)` once `cancelled` says so.
pub fn download(
    data_dir: &Path,
    pack: &Pack,
    progress: &mut dyn FnMut(u64, u64),
    cancelled: &dyn Fn() -> bool,
) -> Result<bool, PackError> {
    let staging = packs_dir(data_dir).join(format!(".{}-download", pack.id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_body(Some(READ_TIMEOUT))
        .build()
        .into();

    let total = pack.size();
    let mut done = 0_u64;
    progress(done, total);
    for file in pack.files {
        let url = format!("{RELEASE_URL}/{}.{}", pack.id(), file.name);
        let mut response = agent
            .get(&url)
            .call()
            .map_err(|e| PackError::Download(e.to_string()))?;
        let mut body = response
            .body_mut()
            .with_config()
            .limit(file.size + 1)
            .reader();
        let mut out = std::fs::File::create(staging.join(file.name))?;
        let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
        let mut buffer = vec![0_u8; 64 * 1024];
        let mut written = 0_u64;
        loop {
            if cancelled() {
                drop(out);
                let _ = std::fs::remove_dir_all(&staging);
                return Ok(false);
            }
            let read = body
                .read(&mut buffer)
                .map_err(|e| PackError::Download(e.to_string()))?;
            if read == 0 {
                break;
            }
            digest.update(&buffer[..read]);
            out.write_all(&buffer[..read])?;
            written += read as u64;
            done += read as u64;
            progress(done.min(total), total);
        }
        out.sync_all()?;
        if written != file.size || hex(digest.finish().as_ref()) != file.sha256 {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(PackError::Damaged(file.name));
        }
    }

    let dir = pack_dir(data_dir, pack);
    delete(data_dir, pack)?;
    std::fs::rename(&staging, &dir)?;
    Ok(true)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_consistent() {
        for pack in PACKS {
            assert_eq!(pack.files.len(), 5, "{}", pack.id());
            for file in pack.files {
                assert_eq!(file.sha256.len(), 64);
                assert!(file.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            }
            assert!(pack.size() > 100_000_000);
        }
        assert_eq!(find("ja", "de").map(Pack::id).as_deref(), Some("ja-de"));
        assert!(find("ko", "de").is_none());
    }

    #[test]
    fn installed_means_every_file_with_its_size() {
        let data = std::env::temp_dir().join(format!("invuso-packs-{}", std::process::id()));
        let pack = &PACKS[0];
        assert!(!is_installed(&data, pack));
        let dir = pack_dir(&data, pack);
        std::fs::create_dir_all(&dir).unwrap();
        for file in pack.files {
            let f = std::fs::File::create(dir.join(file.name)).unwrap();
            f.set_len(file.size).unwrap();
        }
        assert!(is_installed(&data, pack));
        std::fs::File::create(dir.join("vocab.txt")).unwrap();
        assert!(!is_installed(&data, pack));
        delete(&data, pack).unwrap();
        assert!(!dir.exists());
        // Deleting what is not there is fine.
        delete(&data, pack).unwrap();
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn hex_is_lower_case() {
        assert_eq!(hex(&[0x0a, 0xff]), "0aff");
    }
}
