use std::collections::HashMap;
use std::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};

/// Root mRNA document (internal IR).
#[derive(Debug, Clone, Deserialize)]
pub struct MrnaFile {
    #[serde(rename = "api-version")]
    pub api_version: u32,
    pub name: String,
    pub version: String,
    pub release: u32,
    pub description: String,
    pub homepage: Option<String>,
    pub license: String,
    pub maintainer: Option<String>,
    pub tags: Option<Vec<String>>,
    pub depends: Option<Depends>,
    pub features: Option<Features>,
    pub sources: Vec<Source>,
    pub patches: Option<Vec<PatchItem>>,
    pub build: Option<Build>,
    #[serde(rename = "post-install")]
    pub post_install: Option<String>,
    #[serde(rename = "post-remove")]
    pub post_remove: Option<String>,
    pub outputs: Option<Outputs>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Depends {
    pub build: Option<Vec<String>>,
    pub runtime: Option<Vec<String>>,
    pub check: Option<Vec<String>>,
}

impl Depends {
    pub fn all_dependency_strings(&self) -> impl Iterator<Item = (&str, &str)> {
        let sections: [(&str, &Option<Vec<String>>); 3] = [
            ("depends.build", &self.build),
            ("depends.runtime", &self.runtime),
            ("depends.check", &self.check),
        ];
        sections.into_iter().flat_map(|(section, list)| {
            list.as_ref()
                .into_iter()
                .flat_map(move |v| v.iter().map(move |s| (section, s.as_str())))
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Source {
    pub url: String,
    pub hash: Option<String>,
    pub signature: Option<String>,
    #[serde(rename = "key-id")]
    pub key_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Build {
    pub prepare: Option<String>,
    pub compile: Option<String>,
    pub check: Option<String>,
    pub install: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Features {
    #[serde(default)]
    pub default: Vec<String>,
    #[serde(default)]
    pub options: HashMap<String, FeatureOption>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeatureOption {
    pub description: String,
    pub depends: Option<Vec<String>>,
    pub cflags: Option<String>,
}

/// Sub-package split definitions.
#[derive(Debug, Clone, Deserialize)]
pub struct Outputs {
    #[serde(flatten)]
    pub entries: HashMap<String, OutputEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OutputEntry {
    pub description: String,
    pub files: Option<Vec<String>>,
}

/// Patch file reference (plain name or conditional map entry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchItem {
    Simple(String),
    Conditional {
        name: String,
        condition: Option<String>,
        severity: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
struct PatchMeta {
    condition: Option<String>,
    severity: Option<String>,
}

impl<'de> Deserialize<'de> for PatchItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct PatchItemVisitor;

        impl<'de> Visitor<'de> for PatchItemVisitor {
            type Value = PatchItem;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a patch file name or a map with patch metadata")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(PatchItem::Simple(v.to_string()))
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut name: Option<String> = None;
                let mut meta: Option<PatchMeta> = None;

                while let Some(key) = map.next_key::<String>()? {
                    if name.is_none() {
                        name = Some(key);
                        meta = Some(map.next_value()?);
                    } else {
                        return Err(de::Error::custom(
                            "patch map must contain exactly one patch file key",
                        ));
                    }
                }

                let name = name.ok_or_else(|| de::Error::custom("empty patch map"))?;
                let meta = meta.unwrap_or(PatchMeta {
                    condition: None,
                    severity: None,
                });

                Ok(PatchItem::Conditional {
                    name,
                    condition: meta.condition,
                    severity: meta.severity,
                })
            }
        }

        deserializer.deserialize_any(PatchItemVisitor)
    }
}
