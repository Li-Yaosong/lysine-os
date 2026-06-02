use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct MrnaFile {
    pub name: String,
    pub version: String,
    pub release: u32,
    pub description: String,
    pub homepage: Option<String>,
    pub license: String,
    pub depends: Option<Depends>,
    pub features: Option<serde_yaml::Value>,
    pub sources: Vec<Source>,
    pub patches: Option<Vec<String>>,
    pub build: Option<Build>,
    pub outputs: Option<serde_yaml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Depends {
    pub build: Option<Vec<String>>,
    pub runtime: Option<Vec<String>>,
    pub check: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Source {
    pub url: String,
    pub hash: Option<String>,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Build {
    pub prepare: Option<String>,
    pub compile: Option<String>,
    pub check: Option<String>,
    pub install: Option<String>,
}
