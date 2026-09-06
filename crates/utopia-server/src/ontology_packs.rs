//! Prebuilt ontology packs: the starting point when creating a KB.
//!
//! Creating a KB **lays down nothing by default**: after 0009 removed the built-in entity
//! classes, 0010 and `#125` removed the seed relations, and 0011 moved `mapped_to` to the
//! semantic layer, the seeding mechanism was retired entirely. So these packs aren't a
//! "supplement" — they're the ontology's **entire source**.
//!
//! Those original ten seed relations had no type signature at all, while the extraction
//! prompt does support signatures — `- buys_from (employee|team → *)`. Without a signature,
//! direction can only be conveyed by prose, and prose can't constrain direction. Of
//! schema.org's 1521 properties, 1488 carry domain + range: direction is **declared**, not
//! described. See `docs/decisions/0008`.
//!
//! **Files are embedded in the binary**, not downloaded at runtime: the README promises the
//! whole system can run in a fully offline intranet environment, and fetching at runtime
//! would break that promise. Source files are stored gzipped (1.7 MB → 316 KB), decompressed
//! in [`bytes`].

use utopia_core::{AppError, AppResult};

/// One optional prebuilt ontology.
///
/// `classes` / `properties` are **display numbers counted on the day they were fetched**,
/// for the create-KB UI; how much actually gets built is governed by the plan the import
/// returns — the projection only covers constructs consumable right now.
pub struct Pack {
    pub id: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    /// The filename passed to `owl_import`. **Format is determined by extension**
    /// (`RdfFormat::detect`), so the real suffix must be preserved here.
    pub filename: &'static str,
    pub classes: u32,
    pub properties: u32,
    gz: &'static [u8],
}

pub const PACKS: &[Pack] = &[
    Pack {
        id: "schema-org",
        name: "schema.org",
        summary: "People, organizations, products, events, creative works",
        filename: "schema-org.ttl",
        classes: 1010,
        properties: 1676,
        gz: include_bytes!("../packs/schema-org.ttl.gz"),
    },
    Pack {
        id: "w3c-org",
        name: "W3C Org",
        summary: "Departments, posts, memberships, reporting lines",
        filename: "w3c-org.ttl",
        classes: 13,
        properties: 34,
        gz: include_bytes!("../packs/w3c-org.ttl.gz"),
    },
    Pack {
        id: "prov-o",
        name: "PROV-O",
        summary: "Provenance: who produced what, when, from which source",
        filename: "prov-o.ttl",
        classes: 49,
        properties: 69,
        gz: include_bytes!("../packs/prov-o.ttl.gz"),
    },
    Pack {
        id: "foaf",
        name: "FOAF",
        summary: "People and social relations",
        filename: "foaf.rdf",
        classes: 12,
        properties: 62,
        gz: include_bytes!("../packs/foaf.rdf.gz"),
    },
    Pack {
        id: "iof-core",
        name: "IOF Core",
        summary: "Industrial manufacturing",
        filename: "iof-core.rdf",
        classes: 294,
        properties: 75,
        gz: include_bytes!("../packs/iof-core.rdf.gz"),
    },
];

pub fn get(id: &str) -> Option<&'static Pack> {
    PACKS.iter().find(|p| p.id == id)
}

/// Decompresses the source. **Decompressed fresh on every call** — creating a KB is a
/// low-frequency action, not worth keeping 1.7 MB resident for.
pub fn bytes(pack: &Pack) -> AppResult<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(pack.gz)
        .read_to_end(&mut out)
        .map_err(|e| AppError::Other(anyhow::anyhow!("本体包 {} 解压失败：{e}", pack.id)))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every pack decompresses, and what comes out isn't an empty file.
    /// `include_bytes!` guarantees the file exists, but not that it's valid gzip.
    #[test]
    fn every_pack_decompresses() {
        for p in PACKS {
            let b = bytes(p).unwrap_or_else(|e| panic!("{}: {e}", p.id));
            assert!(b.len() > 10_000, "{} 解出来只有 {} 字节", p.id, b.len());
        }
    }

    /// The filename suffix determines format detection; get it wrong and the whole pack
    /// gets fed into the parser as the wrong syntax.
    #[test]
    fn filenames_carry_a_format_suffix() {
        for p in PACKS {
            assert!(
                p.filename.ends_with(".ttl") || p.filename.ends_with(".rdf"),
                "{} 的文件名没有可判定的后缀：{}",
                p.id,
                p.filename
            );
        }
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = PACKS.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "包 id 有重复");
    }
}
